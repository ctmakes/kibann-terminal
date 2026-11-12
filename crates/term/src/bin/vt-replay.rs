//! Play an asciicast v2 recording through the emulator and write the frames a
//! session would have sent, one JSON object per line: `{"t": ms, "frame": …}`.

use std::process::ExitCode;

use term::{cast, replay};

const USAGE: &str = "usage: vt-replay IN.cast [-o OUT.jsonl] [--cell WxH]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vt-replay: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut input, mut output, mut cell) = (None, None, replay::CELL);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => output = Some(args.next().ok_or(USAGE)?),
            "--cell" => {
                let v = args.next().ok_or(USAGE)?;
                let (w, h) = v.split_once('x').ok_or(USAGE)?;
                cell = (w.parse().map_err(|_| USAGE)?, h.parse().map_err(|_| USAGE)?);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ if input.is_none() => input = Some(a),
            _ => return Err(USAGE.into()),
        }
    }
    let input = input.ok_or(USAGE)?;
    let text = std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?;
    let (header, events) = cast::parse(&text).map_err(|e| format!("{input}: {e}"))?;
    let frames = replay::replay(header, &events, cell)?;
    let jsonl = replay::to_jsonl(&frames);
    match output {
        Some(path) => std::fs::write(&path, jsonl).map_err(|e| format!("{path}: {e}"))?,
        None => print!("{jsonl}"),
    }
    eprintln!("vt-replay: {} frames from {input}", frames.len());
    Ok(())
}
