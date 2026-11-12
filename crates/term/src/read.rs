//! Text out of a terminal for agents (`kibann pane read`), and the marks a
//! command prints when it finishes (`kibann pane run --wait`).

use proto::Resize;
use proto::wire::{MARK_OSC, ReadSource};

use crate::vt::Terminal;
use crate::vt::error::Result;
use crate::vt::fmt::{Format, Formatter};
use crate::vt::point::{Coordinate, Point};
use crate::vt::selection::Selection;

/// The terminal's text as plain lines, trailing spaces and blank lines
/// trimmed, keeping only the last `lines` when given.
pub fn text(
    term: &Terminal<'_, '_>,
    size: Resize,
    source: ReadSource,
    lines: Option<usize>,
) -> Result<String> {
    let mut f = Formatter::new(term)?;
    f.set_format(Format::Plain)?.set_trim(true)?;
    match source {
        ReadSource::Recent => {
            f.set_unwrap(true)?;
        }
        ReadSource::Visible => {
            let at =
                |x: u16, y: u16| term.grid_ref(Point::Active(Coordinate { x, y: u32::from(y) }));
            let start = at(0, 0)?;
            let end = at(size.cols.saturating_sub(1), size.rows.saturating_sub(1))?;
            let sel = Selection::new(start, end, false);
            // SAFETY: both ends come from this terminal's active screen, and
            // nothing changes the terminal while the formatter runs.
            unsafe { f.set_selection(Some(&sel))? };
        }
    }
    let bytes = f.format_alloc(None)?;
    let s = String::from_utf8_lossy(&bytes);
    let s = s.trim_end_matches('\n');
    Ok(match lines {
        Some(n) => {
            let all: Vec<&str> = s.split('\n').collect();
            all[all.len().saturating_sub(n)..].join("\n")
        }
        None => s.to_owned(),
    })
}

/// Finds `ESC ] 7770 ; <mark> ; <code> BEL` in output that arrives in
/// pieces. The emulator ignores this OSC, so the mark never shows.
#[derive(Default)]
pub struct MarkScanner {
    /// The start of a mark cut off at the end of the last piece.
    carry: Vec<u8>,
}

/// Longest mark sequence we look for; a longer one is ignored.
const MAX_MARK: usize = 128;

fn prefix() -> Vec<u8> {
    format!("\x1b]{MARK_OSC};").into_bytes()
}

impl MarkScanner {
    /// Scan the next piece of output, calling `found(mark, code)` for each
    /// complete mark.
    pub fn scan(&mut self, data: &[u8], mut found: impl FnMut(String, i32)) {
        if self.carry.is_empty() && !data.contains(&0x1b) {
            return;
        }
        let mut hay = std::mem::take(&mut self.carry);
        hay.extend_from_slice(data);
        let pre = prefix();
        let mut i = 0;
        while let Some(off) = hay[i..].iter().position(|&b| b == 0x1b) {
            let at = i + off;
            let rest = &hay[at..];
            if rest.len() < pre.len() && pre.starts_with(rest) {
                self.carry = rest.to_vec();
                return;
            }
            if !rest.starts_with(&pre) {
                i = at + 1;
                continue;
            }
            let body = &rest[pre.len()..];
            match body.iter().take(MAX_MARK).position(|&b| b == 0x07) {
                Some(end) => {
                    if let Some((mark, code)) = parse(&body[..end]) {
                        found(mark, code);
                    }
                    i = at + pre.len() + end + 1;
                }
                None if body.len() < MAX_MARK => {
                    self.carry = rest.to_vec();
                    return;
                }
                None => i = at + 1,
            }
        }
    }
}

fn parse(body: &[u8]) -> Option<(String, i32)> {
    let s = std::str::from_utf8(body).ok()?;
    let (mark, code) = s.rsplit_once(';')?;
    Some((mark.to_owned(), code.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(pieces: &[&[u8]]) -> Vec<(String, i32)> {
        let mut s = MarkScanner::default();
        let mut out = Vec::new();
        for p in pieces {
            s.scan(p, |m, c| out.push((m, c)));
        }
        out
    }

    #[test]
    fn marks_are_found_whole_or_in_pieces() {
        let whole = b"ok\x1b]7770;abc;0\x07\x1b[0m\x1b]7770;d;127\x07";
        assert_eq!(
            scan_all(&[whole]),
            vec![("abc".into(), 0), ("d".into(), 127)]
        );
        for cut in 1..whole.len() {
            let (a, b) = whole.split_at(cut);
            assert_eq!(scan_all(&[a, b]).len(), 2, "cut at {cut}");
        }
    }

    #[test]
    fn other_sequences_and_bad_marks_are_ignored() {
        assert!(scan_all(&[b"\x1b]777;x;1\x07\x1b]7770;nocode\x07"]).is_empty());
        // The command line as typed shows the printf, not the escape.
        assert!(scan_all(&[b"printf '\\033]7770;abc;%d\\007' $?"]).is_empty());
    }
}
