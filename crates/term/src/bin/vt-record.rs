//! Record a program's output as an asciicast v2 file.
//!
//! Interactive (Unix only): `vt-record -o vim.cast -- vim`
//! Scripted: `vt-record -o vim.cast -s 100x30 --script vim.script -- vim -u NONE`
//!
//! A script has one step per line (`#` starts a comment):
//!   sleep 500          wait 500 ms
//!   send :q\r          send text; escapes: \r \n \t \e \\ \xHH
//!   resize 120x40      resize the PTY (recorded as an "r" event)

use std::io::{Read, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use proto::Resize;
use term::Emulator;
use term::cast::{CastWriter, Header};

const USAGE: &str =
    "usage: vt-record -o OUT.cast [-s COLSxROWS] [--script FILE] [--cwd DIR] -- CMD [ARGS...]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("vt-record: {e}");
            ExitCode::FAILURE
        }
    }
}

enum Step {
    Sleep(Duration),
    Send(Vec<u8>),
    Resize(u16, u16),
}

fn parse_size(s: &str) -> Result<(u16, u16), String> {
    let bad = || format!("bad size {s:?}, expected COLSxROWS");
    let (c, r) = s.split_once('x').ok_or_else(bad)?;
    Ok((c.parse().map_err(|_| bad())?, r.parse().map_err(|_| bad())?))
}

fn unescape(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut b = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
            continue;
        }
        match chars.next() {
            Some('r') => out.push(b'\r'),
            Some('n') => out.push(b'\n'),
            Some('t') => out.push(b'\t'),
            Some('e') => out.push(0x1b),
            Some('\\') => out.push(b'\\'),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).map_err(|_| format!("bad escape \\x{hex}"))?);
            }
            other => {
                return Err(format!(
                    "bad escape \\{}",
                    other.map(String::from).unwrap_or_default()
                ));
            }
        }
    }
    Ok(out)
}

fn parse_script(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
        let err = |e: String| format!("script line {}: {e}", i + 1);
        steps.push(match cmd {
            "sleep" => Step::Sleep(Duration::from_millis(
                arg.trim()
                    .parse()
                    .map_err(|_| err("sleep takes milliseconds".into()))?,
            )),
            "send" => Step::Send(unescape(arg).map_err(err)?),
            "resize" => {
                let (c, r) = parse_size(arg.trim()).map_err(err)?;
                Step::Resize(c, r)
            }
            _ => return Err(err(format!("unknown step {cmd:?}"))),
        });
    }
    Ok(steps)
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (mut out, mut size, mut script, mut cwd) = (None, None, None, None);
    let mut argv = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => out = Some(args.next().ok_or(USAGE)?),
            "-s" => size = Some(parse_size(&args.next().ok_or(USAGE)?)?),
            "--script" => script = Some(args.next().ok_or(USAGE)?),
            "--cwd" => cwd = Some(args.next().ok_or(USAGE)?),
            "--" => {
                argv.extend(args.by_ref());
                break;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => return Err(USAGE.into()),
        }
    }
    let out = out.ok_or(USAGE)?;
    if argv.is_empty() {
        return Err(USAGE.into());
    }
    let steps = match &script {
        Some(p) => Some(parse_script(
            &std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
        )?),
        None => None,
    };
    let interactive = steps.is_none();
    #[cfg(not(unix))]
    if interactive {
        return Err("interactive recording needs Unix; pass --script".into());
    }
    let (cols, rows) = size
        .or_else(|| interactive.then(tty_size).flatten())
        .unwrap_or((80, 24));

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.env("TERM", term::emulator::TERMINFO_NAME);
    cmd.env("COLORTERM", "truecolor");
    if std::env::var_os("LANG").is_none() {
        cmd.env("LANG", "en_US.UTF-8");
    }
    if let Some(d) = &cwd {
        cmd.cwd(d);
    } else if let Ok(d) = std::env::current_dir() {
        cmd.cwd(d);
    }
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
        pair.master.take_writer().map_err(|e| e.to_string())?,
    ));

    let file = std::fs::File::create(&out).map_err(|e| format!("{out}: {e}"))?;
    let cast = CastWriter::new(
        std::io::BufWriter::new(file),
        Header {
            width: cols,
            height: rows,
        },
        Some(&argv.join(" ")),
    )
    .map_err(|e| e.to_string())?;
    let cast = Arc::new(Mutex::new(cast));

    // Cleared before the program is stopped, so its dying words aren't recorded.
    let recording = Arc::new(AtomicBool::new(true));
    let (done_tx, done_rx) = mpsc::channel();
    {
        let cast = cast.clone();
        let writer = writer.clone();
        let recording = recording.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 64 * 1024];
            let mut stdout = std::io::stdout();
            // With no real terminal attached, an emulator answers the
            // program's queries (cursor position, device attributes...).
            // Its size isn't updated by `resize` steps.
            let mut responder = if interactive {
                None
            } else {
                Emulator::new(Resize {
                    cols,
                    rows,
                    cell_w: 8,
                    cell_h: 16,
                })
                .ok()
            };
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if !recording.load(Ordering::SeqCst) {
                            continue;
                        }
                        let _ = cast.lock().unwrap().output(&buf[..n]);
                        if let Some(emu) = &mut responder {
                            emu.write(&buf[..n]);
                            let reply = emu.take_outbox();
                            if !reply.is_empty() {
                                let _ = send(&writer, &reply);
                            }
                        }
                        if interactive {
                            let _ = stdout.write_all(&buf[..n]);
                            let _ = stdout.flush();
                        }
                    }
                }
            }
            let _ = done_tx.send(());
        });
    }

    match steps {
        Some(steps) => {
            for step in steps {
                match step {
                    Step::Sleep(d) => thread::sleep(d),
                    Step::Send(b) => {
                        // Logged as input, so tests can find the moments
                        // between steps.
                        let _ = cast.lock().unwrap().input(&b);
                        send(&writer, &b).map_err(|e| e.to_string())?
                    }
                    Step::Resize(c, r) => {
                        pair.master
                            .resize(PtySize {
                                rows: r,
                                cols: c,
                                pixel_width: 0,
                                pixel_height: 0,
                            })
                            .map_err(|e| e.to_string())?;
                        cast.lock()
                            .unwrap()
                            .resize(c, r)
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
            // Scripts usually end inside the program: stop it.
            recording.store(false, Ordering::SeqCst);
            let _ = child.kill();
            let deadline = Instant::now() + Duration::from_secs(3);
            while child.try_wait().map_err(|e| e.to_string())?.is_none() {
                if Instant::now() > deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
        None => {
            #[cfg(unix)]
            {
                let _raw = raw::RawMode::enable();
                thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    let mut stdin = std::io::stdin();
                    while let Ok(n) = stdin.read(&mut buf) {
                        if n == 0 || send(&writer, &buf[..n]).is_err() {
                            break;
                        }
                    }
                });
                let _ = child.wait();
            }
        }
    }

    let _ = done_rx.recv_timeout(Duration::from_secs(1));
    cast.lock().unwrap().flush().map_err(|e| e.to_string())?;
    drop(pair.master);
    eprintln!("vt-record: wrote {out} ({cols}x{rows})");
    Ok(())
}

fn send(w: &Mutex<Box<dyn Write + Send>>, data: &[u8]) -> std::io::Result<()> {
    let mut w = w.lock().unwrap();
    w.write_all(data)?;
    w.flush()
}

#[cfg(unix)]
fn tty_size() -> Option<(u16, u16)> {
    // SAFETY: TIOCGWINSZ only writes a winsize into the struct we pass.
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) } == 0;
    (ok && ws.ws_col > 0 && ws.ws_row > 0).then_some((ws.ws_col, ws.ws_row))
}

#[cfg(not(unix))]
fn tty_size() -> Option<(u16, u16)> {
    None
}

#[cfg(unix)]
mod raw {
    /// Puts stdin in raw mode until dropped.
    pub struct RawMode(Option<libc::termios>);

    impl RawMode {
        pub fn enable() -> Self {
            // SAFETY: plain termios calls on stdin with a struct we own.
            unsafe {
                let mut t: libc::termios = std::mem::zeroed();
                if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
                    return Self(None);
                }
                let saved = t;
                libc::cfmakeraw(&mut t);
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t);
                Self(Some(saved))
            }
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            if let Some(t) = &self.0 {
                // SAFETY: restores the attributes saved above.
                unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, t) };
            }
        }
    }
}
