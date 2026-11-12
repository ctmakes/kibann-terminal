//! A running terminal: shell on a PTY, emulator thread, frame pacing.
//!
//! Threads:
//! - **session**: owns the [`Emulator`]; handles every [`Msg`] in order.
//! - **reader**: PTY output to the session thread.
//! - **writer**: bytes from the session thread to the PTY, so a program that
//!   stops reading input can't stall the emulator.
//! - **waiter**: reports the child's exit status.
//!
//! Pacing (see `plans/done/renderer.md`): a frame goes out right away if none went
//! out in the last 16 ms, otherwise at the 16 ms mark. Nothing is built while
//! more than one frame is unacknowledged; changes keep accumulating and the
//! next frame carries all of them.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use proto::wire::ReadSource;
use proto::{Frame, InputEvent, MouseAction, Resize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::cast::{CastWriter, Header};
use crate::emulator::{Emulator, TERMINFO_NAME};
use crate::selection::AUTOSCROLL_INTERVAL;

/// Minimum time between frames.
pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);
/// How much PTY output to process before checking whether a frame is due.
const OUTPUT_BUDGET: usize = 1 << 20;
/// Messages in flight to the session thread. Bounded so a flood of output
/// slows the reader down instead of growing memory.
const QUEUE: usize = 256;
/// How long to wait for the last output after the child exits. ConPTY may
/// never report the end of output on its own.
const EXIT_GRACE: Duration = Duration::from_millis(200);

/// What a session reports to its frontend.
// Nearly every event is a frame, moved straight into the sink; boxing it
// would only add an allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Event {
    Frame(Frame),
    /// The shell exited, with its exit code when known.
    Exited(Option<u32>),
}

pub type Sink = Box<dyn FnMut(Event) + Send>;

enum Msg {
    Output(Vec<u8>),
    OutputEnd,
    ChildExited(Option<u32>),
    Input(InputEvent),
    Resize(Resize),
    Ack(u64),
    RequestFull,
    Attach(Sink),
    Cwd(Sender<Option<PathBuf>>),
    Title(Sender<String>),
    Read(ReadSource, Option<usize>, Sender<String>),
    Shutdown,
}

/// Marks seen in the output (see [`crate::read::MarkScanner`]), oldest
/// first, at most [`MAX_MARKS`].
type Marks = Arc<Mutex<VecDeque<(String, i32)>>>;
const MAX_MARKS: usize = 64;

/// What to run. Defaults to the user's login shell.
#[derive(Default)]
pub struct Spawn {
    /// Program and arguments; `None` runs the default shell.
    pub argv: Option<Vec<String>>,
    pub cwd: Option<std::path::PathBuf>,
    pub env: Vec<(String, String)>,
}

/// Handle to a session. Dropping it kills the shell and stops the threads.
pub struct Session {
    tx: SyncSender<Msg>,
    marks: Marks,
    thread: Option<JoinHandle<()>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
}

impl Session {
    /// Start a shell on a new PTY.
    pub fn spawn(spawn: Spawn, size: Resize) -> io::Result<Self> {
        let pty = native_pty_system();
        let pair = pty.openpty(pty_size(size)).map_err(io::Error::other)?;

        let mut cmd = match &spawn.argv {
            Some(argv) if !argv.is_empty() => {
                let mut c = CommandBuilder::new(&argv[0]);
                c.args(&argv[1..]);
                c
            }
            _ => CommandBuilder::new_default_prog(),
        };
        for (k, _) in std::env::vars_os() {
            if k.to_str().is_some_and(is_foreign_terminal_var) {
                cmd.env_remove(&k);
            }
        }
        cmd.env("TERM", TERMINFO_NAME);
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "kibann");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        for (k, v) in &spawn.env {
            cmd.env(k, v);
        }
        if let Some(dir) = spawn.cwd.clone().or_else(home_dir) {
            cmd.cwd(dir);
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(io::Error::other)?;
        // Our copy of the slave must close, or reads never see the end.
        drop(pair.slave);
        let killer = child.clone_killer();
        let shell_pid = child.process_id();
        let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;

        let (tx, rx) = mpsc::sync_channel(QUEUE);
        spawn_reader(reader, tx.clone());
        {
            let tx = tx.clone();
            thread::Builder::new()
                .name("term-waiter".into())
                .spawn(move || {
                    let code = child.wait().ok().map(|s| s.exit_code());
                    let _ = tx.send(Msg::ChildExited(code));
                })?;
        }
        let pty_writer = spawn_writer(writer)?;
        let master = pair.master;
        let marks = Marks::default();
        let m = marks.clone();
        let thread = thread::Builder::new()
            .name("term-session".into())
            .spawn(move || {
                let pty = Pty { master, shell_pid };
                run(
                    rx,
                    size,
                    pty_writer,
                    Some(pty),
                    true,
                    debug_recorder(size),
                    m,
                );
            })?;
        Ok(Self {
            tx,
            marks,
            thread: Some(thread),
            killer: Some(killer),
        })
    }

    /// A session over caller-provided I/O, for tests and replay: `output`
    /// plays the role of the PTY, and bytes for the PTY go to `input`.
    pub fn with_io(
        size: Resize,
        output: impl Read + Send + 'static,
        input: impl Write + Send + 'static,
    ) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        spawn_reader(Box::new(output), tx.clone());
        let pty_writer = spawn_writer(Box::new(input))?;
        let marks = Marks::default();
        let m = marks.clone();
        let thread = thread::Builder::new()
            .name("term-session".into())
            .spawn(move || {
                run(rx, size, pty_writer, None, false, None, m);
            })?;
        Ok(Self {
            tx,
            marks,
            thread: Some(thread),
            killer: None,
        })
    }

    /// Send frames to `sink` from now on, starting with a full frame. Replaces
    /// any earlier sink (a reloaded frontend).
    pub fn attach(&self, sink: impl FnMut(Event) + Send + 'static) {
        self.send(Msg::Attach(Box::new(sink)));
    }

    pub fn input(&self, e: InputEvent) {
        self.send(Msg::Input(e));
    }

    pub fn resize(&self, size: Resize) {
        self.send(Msg::Resize(size));
    }

    /// The frontend painted every frame up to `seq`.
    pub fn ack(&self, seq: u64) {
        self.send(Msg::Ack(seq));
    }

    pub fn request_full(&self) {
        self.send(Msg::RequestFull);
    }

    /// The shell's working directory: what it last reported (OSC 7), else
    /// what the OS says for the foreground program or the shell. `None` when
    /// unknown (or the session is busy for more than a moment).
    pub fn cwd(&self) -> Option<PathBuf> {
        let (tx, rx) = mpsc::channel();
        self.send(Msg::Cwd(tx));
        rx.recv_timeout(Duration::from_millis(500)).ok().flatten()
    }

    /// The title the tab shows: the program's own, else its name.
    pub fn title(&self) -> String {
        let (tx, rx) = mpsc::channel();
        self.send(Msg::Title(tx));
        rx.recv_timeout(Duration::from_millis(500))
            .unwrap_or_default()
    }

    /// The pane's text (see [`crate::read::text`]). Empty if the session is
    /// gone or busy for more than a moment.
    pub fn read(&self, source: ReadSource, lines: Option<usize>) -> String {
        let (tx, rx) = mpsc::channel();
        self.send(Msg::Read(source, lines, tx));
        rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default()
    }

    /// The code printed with `mark`, once the output has carried it.
    pub fn mark(&self, mark: &str) -> Option<i32> {
        let marks = self.marks.lock().unwrap_or_else(|e| e.into_inner());
        marks.iter().rev().find(|(m, _)| m == mark).map(|(_, c)| *c)
    }

    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(k) = &mut self.killer {
            let _ = k.kill();
        }
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Variables describing the terminal (or multiplexer, or agent) that started
/// the app. Programs use them for feature detection, so inherited ones would
/// make them think they run somewhere else. Everything else, `EDITOR` and
/// `PATH` included, passes through.
pub fn is_foreign_terminal_var(name: &str) -> bool {
    const NAMES: &[&str] = &[
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "TERM_SESSION_ID",
        "LC_TERMINAL",
        "LC_TERMINAL_VERSION",
        "COLORFGBG",
        "VTE_VERSION",
        "WINDOWID",
        "TMUX",
        "TMUX_PANE",
        "STY",
        "WT_SESSION",
        "WT_PROFILE_ID",
        "TERMINAL_EMULATOR",
        "CLAUDECODE",
        "CLAUDE_CODE_ENTRYPOINT",
    ];
    const PREFIXES: &[&str] = &[
        "GHOSTTY_",
        "ITERM_",
        "KITTY_",
        "WEZTERM_",
        "ALACRITTY_",
        "KONSOLE_",
        "VSCODE_",
        "KIBANN_",
    ];
    NAMES.contains(&name) || PREFIXES.iter().any(|p| name.starts_with(p))
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(Into::into)
}

fn pty_size(s: Resize) -> PtySize {
    PtySize {
        rows: s.rows.max(1),
        cols: s.cols.max(1),
        pixel_width: s.cols.saturating_mul(s.cell_w),
        pixel_height: s.rows.saturating_mul(s.cell_h),
    }
}

fn spawn_reader(mut r: Box<dyn Read + Send>, tx: SyncSender<Msg>) {
    let _ = thread::Builder::new()
        .name("term-reader".into())
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match r.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(Msg::Output(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = tx.send(Msg::OutputEnd);
        });
}

fn spawn_writer(mut w: Box<dyn Write + Send>) -> io::Result<Sender<Vec<u8>>> {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::Builder::new()
        .name("term-writer".into())
        .spawn(move || {
            for data in rx {
                if w.write_all(&data).and_then(|_| w.flush()).is_err() {
                    break;
                }
            }
        })?;
    Ok(tx)
}

/// Pacing and backpressure state of the session thread.
struct Pacer {
    sink: Option<Sink>,
    /// Seq of the last frame sent and the last one acknowledged.
    sent: u64,
    acked: u64,
    /// Terminal changed since the last frame.
    dirty: bool,
    force_full: bool,
    last_frame: Option<Instant>,
}

impl Pacer {
    fn unacked(&self) -> u64 {
        self.sent.saturating_sub(self.acked)
    }

    fn can_send(&self) -> bool {
        self.sink.is_some() && self.unacked() <= 1
    }

    fn emit(&mut self, f: Frame, now: Instant) {
        self.sent = f.seq;
        self.last_frame = Some(now);
        if let Some(s) = &mut self.sink {
            s(Event::Frame(f));
        }
    }

    /// Send a frame if one is due. Returns when to check again, if waiting on the clock.
    fn tick(&mut self, emu: &mut Emulator, now: Instant) -> Option<Instant> {
        if !self.can_send() {
            return None;
        }
        // A frame captured at the start of a hold goes first.
        if let Some(f) = emu.take_captured() {
            self.emit(f, now);
            return None;
        }
        if !(self.dirty || self.force_full) || emu.held() {
            return None;
        }
        if let Some(t) = self.last_frame {
            let due = t + FRAME_INTERVAL;
            if now < due && !self.force_full {
                return Some(due);
            }
        }
        match emu.build_frame(self.force_full) {
            Ok(Some(f)) => self.emit(f, now),
            Ok(None) => {}
            Err(_) => return None,
        }
        self.dirty = false;
        self.force_full = false;
        None
    }
}

type Recorder = CastWriter<std::fs::File>;

/// `KIBANN_RECORD=path.cast` records the session for debugging: program
/// output, the bytes sent to it, and each input event as a marker.
fn debug_recorder(size: Resize) -> Option<Recorder> {
    let path = std::env::var_os("KIBANN_RECORD")?;
    let file = std::fs::File::create(&path).ok()?;
    let header = Header {
        width: size.cols,
        height: size.rows,
    };
    CastWriter::new(file, header, None).ok()
}

/// The real PTY side of a session.
struct Pty {
    master: Box<dyn MasterPty + Send>,
    shell_pid: Option<u32>,
}

impl Pty {
    /// The foreground program's process (group leader), else the shell's.
    fn foreground_pid(&self) -> Option<i32> {
        #[cfg(unix)]
        if let Some(pid) = self.master.process_group_leader() {
            return Some(pid);
        }
        self.shell_pid.and_then(|p| i32::try_from(p).ok())
    }

    fn cwd(&self) -> Option<PathBuf> {
        let fg = self.foreground_pid()?;
        crate::process::cwd(fg).or_else(|| {
            let shell = i32::try_from(self.shell_pid?).ok()?;
            crate::process::cwd(shell)
        })
    }
}

fn run(
    rx: Receiver<Msg>,
    size: Resize,
    pty: Sender<Vec<u8>>,
    master: Option<Pty>,
    report_exit: bool,
    mut rec: Option<Recorder>,
    marks: Marks,
) {
    let mut scanner = crate::read::MarkScanner::default();
    let Ok(mut emu) = Emulator::new(size) else {
        return;
    };
    let mut p = Pacer {
        sink: None,
        sent: 0,
        acked: 0,
        dirty: false,
        force_full: true,
        last_frame: None,
    };
    let mut output_done = false;
    // Exit code and when it arrived.
    let mut exit: Option<(Option<u32>, Instant)> = None;
    let mut exit_sent = false;
    let mut deadline: Option<Instant> = None;
    // Next autoscroll step while a selection drag is held past an edge.
    let mut autoscroll: Option<Instant> = None;

    loop {
        let exit_wait = exit
            .filter(|_| !output_done && !exit_sent)
            .map(|(_, t)| t + EXIT_GRACE);
        let wake = [deadline, emu.hold_deadline(), exit_wait, autoscroll]
            .into_iter()
            .flatten()
            .min();
        let first = match wake {
            Some(w) => match rx.recv_timeout(w.saturating_duration_since(Instant::now())) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            },
        };

        let mut budget = OUTPUT_BUDGET;
        let mut next = first;
        while let Some(m) = next.take() {
            match m {
                Msg::Output(data) => {
                    budget = budget.saturating_sub(data.len());
                    emu.write(&data);
                    p.dirty = true;
                    scanner.scan(&data, |mark, code| {
                        let mut m = marks.lock().unwrap_or_else(|e| e.into_inner());
                        if m.len() == MAX_MARKS {
                            m.pop_front();
                        }
                        m.push_back((mark, code));
                    });
                    if let Some(r) = &mut rec {
                        let _ = r.output(&data);
                    }
                }
                Msg::OutputEnd => output_done = true,
                Msg::ChildExited(code) => exit = Some((code, Instant::now())),
                Msg::Input(e) => {
                    if let Some(r) = &mut rec
                        && !matches!(
                            e,
                            InputEvent::Mouse {
                                action: MouseAction::Motion,
                                ..
                            } | InputEvent::Hover { .. }
                        )
                    {
                        let _ = r.marker(&serde_json::to_string(&e).unwrap_or_default());
                    }
                    let _ = emu.input(&e);
                    // Selection and scrolling change the screen with no output.
                    p.dirty = true;
                }
                Msg::Resize(s) => {
                    if let Some(r) = &mut rec {
                        let _ = r.resize(s.cols, s.rows);
                    }
                    if let Ok(true) = emu.resize(s) {
                        if let Some(m) = &master {
                            let _ = m.master.resize(pty_size(s));
                        }
                        p.dirty = true;
                        p.force_full = true;
                    }
                }
                Msg::Ack(seq) => p.acked = p.acked.max(seq.min(p.sent)),
                Msg::RequestFull => p.force_full = true,
                Msg::Attach(sink) => {
                    p.sink = Some(sink);
                    // Frames sent to the old sink will never be acked.
                    p.acked = p.sent;
                    p.force_full = true;
                    p.last_frame = None;
                    exit_sent = false;
                }
                Msg::Cwd(reply) => {
                    let dir = emu
                        .reported_dir()
                        .or_else(|| master.as_ref().and_then(Pty::cwd));
                    let _ = reply.send(dir);
                }
                Msg::Title(reply) => {
                    let _ = reply.send(emu.title());
                }
                Msg::Read(source, lines, reply) => {
                    let text = crate::read::text(emu.terminal(), emu.size(), source, lines);
                    let _ = reply.send(text.unwrap_or_default());
                }
                Msg::Shutdown => return,
            }
            if budget == 0 {
                break;
            }
            next = rx.try_recv().ok();
        }

        // The foreground program changes as programs start, exit or exec,
        // which comes with output; the lookup is a syscall or two.
        if p.dirty
            && let Some(m) = &master
        {
            emu.set_program_name(m.foreground_pid().and_then(crate::process::name));
        }
        // Paths in links are relative to the shell's directory. Only looked
        // up while hovering, and only when the shell doesn't report it.
        if emu.hovering()
            && emu.reported_dir().is_none()
            && let Some(m) = &master
        {
            emu.set_cwd(m.cwd());
        }

        let out = emu.take_outbox();
        if !out.is_empty() {
            if let Some(r) = &mut rec {
                let _ = r.input(&out);
                let _ = r.flush();
            }
            let _ = pty.send(out);
        }

        let now = Instant::now();
        if emu.expire_hold(now) {
            p.dirty = true;
        }
        if emu.autoscrolling() {
            let due = *autoscroll.get_or_insert(now + AUTOSCROLL_INTERVAL);
            if now >= due {
                let _ = emu.autoscroll_tick();
                p.dirty = true;
                autoscroll = Some(now + AUTOSCROLL_INTERVAL);
            }
        } else {
            autoscroll = None;
        }
        deadline = p.tick(&mut emu, now);

        if report_exit
            && let Some((code, at)) = exit
            && (output_done || now >= at + EXIT_GRACE)
            && !exit_sent
            && let Some(sink) = p.sink.as_mut()
        {
            // Make sure the last output is on screen first.
            if let Ok(Some(f)) = emu.build_frame(false) {
                p.sent = f.seq;
                sink(Event::Frame(f));
            }
            sink(Event::Exited(code));
            exit_sent = true;
        }
    }
}
