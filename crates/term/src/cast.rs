//! asciicast v2 recordings: a JSON header line, then one `[time, code, data]`
//! line per event. Readable by asciinema's tools.
//!
//! <https://docs.asciinema.org/manual/asciicast/v2/>

use std::io::{self, Write};
use std::time::Instant;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CastEvent {
    /// Seconds since the start, program output.
    Output(f64, String),
    Input(f64, String),
    Resize(f64, u16, u16),
}

impl CastEvent {
    pub fn time(&self) -> f64 {
        match self {
            Self::Output(t, _) | Self::Input(t, _) | Self::Resize(t, ..) => *t,
        }
    }
}

/// Parse a whole recording. Markers and unknown event codes are skipped.
pub fn parse(text: &str) -> Result<(Header, Vec<CastEvent>), String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let first = lines.next().ok_or("empty recording")?;
    let h: Value = serde_json::from_str(first).map_err(|e| format!("header: {e}"))?;
    if h["version"] != 2 {
        return Err(format!("unsupported asciicast version {}", h["version"]));
    }
    let dim = |k: &str| {
        h[k].as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or(format!("header: bad {k}"))
    };
    let header = Header {
        width: dim("width")?,
        height: dim("height")?,
    };

    let mut events = Vec::new();
    for (i, line) in lines.enumerate() {
        let bad = |what: &str| format!("event {}: {what}", i + 1);
        let v: Value = serde_json::from_str(line).map_err(|e| bad(&e.to_string()))?;
        let t = v[0].as_f64().ok_or_else(|| bad("no time"))?;
        let data = v[2].as_str().ok_or_else(|| bad("no data"))?;
        match v[1].as_str() {
            Some("o") => events.push(CastEvent::Output(t, data.to_owned())),
            Some("i") => events.push(CastEvent::Input(t, data.to_owned())),
            Some("r") => {
                let (w, h) = data
                    .split_once('x')
                    .ok_or_else(|| bad("resize is not WxH"))?;
                let n = |s: &str| {
                    s.trim()
                        .parse::<u16>()
                        .map_err(|_| bad("resize is not WxH"))
                };
                events.push(CastEvent::Resize(t, n(w)?, n(h)?));
            }
            _ => {}
        }
    }
    Ok((header, events))
}

/// Turns a byte stream into UTF-8 text, holding back a sequence split
/// across reads. Invalid bytes become U+FFFD.
#[derive(Default)]
pub struct Utf8Stream {
    pending: Vec<u8>,
}

impl Utf8Stream {
    pub fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::new();
        let mut rest: &[u8] = &self.pending;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (ok, tail) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(ok).unwrap());
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{fffd}');
                            rest = &tail[n..];
                        }
                        // Incomplete sequence at the end: wait for more.
                        None => {
                            rest = tail;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }
}

/// Writes a recording as it happens.
pub struct CastWriter<W: Write> {
    w: W,
    start: Instant,
    out: Utf8Stream,
}

impl<W: Write> CastWriter<W> {
    pub fn new(mut w: W, header: Header, command: Option<&str>) -> io::Result<Self> {
        let mut h = json!({
            "version": 2,
            "width": header.width,
            "height": header.height,
            "env": { "TERM": crate::emulator::TERMINFO_NAME },
        });
        if let Some(c) = command {
            h["command"] = json!(c);
        }
        writeln!(w, "{h}")?;
        Ok(Self {
            w,
            start: Instant::now(),
            out: Utf8Stream::default(),
        })
    }

    fn event(&mut self, code: &str, data: &str) -> io::Result<()> {
        let t = (self.start.elapsed().as_secs_f64() * 1e6).round() / 1e6;
        writeln!(self.w, "{}", json!([t, code, data]))
    }

    pub fn output(&mut self, bytes: &[u8]) -> io::Result<()> {
        let s = self.out.push(bytes);
        if s.is_empty() {
            Ok(())
        } else {
            self.event("o", &s)
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> io::Result<()> {
        self.event("r", &format!("{cols}x{rows}"))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.w.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_split_across_reads() {
        let mut u = Utf8Stream::default();
        let bytes = "a日b".as_bytes();
        assert_eq!(u.push(&bytes[..2]), "a");
        assert_eq!(u.push(&bytes[2..]), "日b");
        assert_eq!(u.push(b"\xff!"), "\u{fffd}!");
    }

    #[test]
    fn write_then_parse() {
        let mut buf = Vec::new();
        {
            let mut w = CastWriter::new(
                &mut buf,
                Header {
                    width: 80,
                    height: 24,
                },
                Some("vim"),
            )
            .unwrap();
            w.output(b"hi\x1b[0m").unwrap();
            w.resize(100, 30).unwrap();
        }
        let (h, ev) = parse(std::str::from_utf8(&buf).unwrap()).unwrap();
        assert_eq!(
            h,
            Header {
                width: 80,
                height: 24
            }
        );
        assert!(matches!(&ev[0], CastEvent::Output(_, s) if s == "hi\x1b[0m"));
        assert!(matches!(ev[1], CastEvent::Resize(_, 100, 30)));
    }

    #[test]
    fn rejects_bad_header() {
        assert!(parse("{\"version\":1,\"width\":80,\"height\":24}").is_err());
        assert!(parse("").is_err());
    }
}
