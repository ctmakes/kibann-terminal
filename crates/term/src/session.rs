//! A running terminal: shell on a PTY, emulator thread, frame pacing.
//!
//! Threads:
//! - **session**: owns the [`Emulator`]. Handles control messages (input,
//!   resize, acks) before PTY output, so typing never waits behind a flood
//!   of output, and publishes what queries need ([`Session::title`] and
//!   the like) after each pass, so they don't wait on it at all.
//! - **reader**: PTY output to the session thread.
//! - **writer**: bytes from the session thread to the PTY, so a program that
//!   stops reading input can't stall the emulator.
//! - **waiter**: reports the child's exit status.
//!
//! Frames go to any number of views (the app, a phone), paced by
//! [`crate::pacer`]. The screen state (size, scroll, selection, search,
//! hover) is the session's: the views share it, and the last resize wins.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{self as channel, Receiver};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use proto::wire::ReadSource;
use proto::{Frame, InputEvent, MouseAction, Resize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::cast::{CastWriter, Header};
use crate::emulator::{Emulator, TERMINFO_NAME};
use crate::pacer::Pacer;
pub use crate::pacer::{FRAME_INTERVAL, FrameSink, ViewId};
use crate::selection::AUTOSCROLL_INTERVAL;
use agent::{Agent, State as AgentState};

/// How much PTY output to process before checking whether a frame is due.
const OUTPUT_BUDGET: usize = 1 << 20;
/// Chunks of PTY output in flight to the session thread. Bounded so a
/// flood of output slows the reader down instead of growing memory.
const QUEUE: usize = 256;
/// How long to wait for the last output after the child exits. ConPTY may
/// never report the end of output on its own.
const EXIT_GRACE: Duration = Duration::from_millis(200);

/// What a session reports to its owner (see [`Session::on_event`]).
#[derive(Debug)]
pub enum Event {
    /// The shell exited, with its exit code when known.
    Exited(Option<u32>),
    /// What runs in the foreground changed (see [`Foreground`]).
    Foreground(Foreground),
}

/// What runs in the pane and where, as far as agent status and git
/// context care.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    /// A known agent and its state, read from its title and screen.
    pub agent: Option<(Agent, AgentState)>,
    /// The shell itself is in the foreground (nothing runs in it).
    pub at_shell: bool,
    /// The working directory, as [`Session::cwd`] finds it.
    pub cwd: Option<PathBuf>,
    /// Lines entered so far (Enter, or a newline typed or pasted). A
    /// quick command can start and end between two looks at the
    /// foreground; this still tells that one ran.
    pub entered: u64,
}

/// Whether `e` enters a line.
fn enters(e: &InputEvent) -> bool {
    use proto::KeyAction;
    match e {
        InputEvent::Key {
            action: KeyAction::Press | KeyAction::Repeat,
            code,
            composing: false,
            ..
        } => code == "Enter" || code == "NumpadEnter",
        InputEvent::Text { data } | InputEvent::Paste { data } => data.contains(['\r', '\n']),
        _ => false,
    }
}

/// How often at most to look at what runs in the pane (the foreground
/// program, its directory, an agent's state on screen) while output
/// arrives. The first output after a quiet spell gets a look at once, and
/// one more look follows when output stops.
const LOOK_INTERVAL: Duration = Duration::from_millis(250);
/// How often to look with no output, for what changes silently (a `cd` in
/// a script).
const LOOK_IDLE: Duration = Duration::from_secs(2);

pub type Sink = Box<dyn FnMut(Event) + Send>;

/// PTY output, on the bounded queue.
enum Output {
    Data(Vec<u8>),
    End,
}

/// Everything else, handled before any queued output.
enum Msg {
    ChildExited(Option<u32>),
    Input(InputEvent),
    Resize(Resize),
    Ack(ViewId, u64),
    RequestFull(ViewId),
    Theme(proto::theme::Theme),
    Events(Sink),
    View(ViewId, FrameSink),
    Unview(ViewId),
    Show(Vec<u8>),
    Read(ReadSource, Option<usize>, Sender<String>),
    Shutdown,
}

/// What the session thread last saw, for queries that shouldn't wait on
/// it.
#[derive(Default)]
struct Seen {
    title: String,
    /// The directory the shell reported (OSC 7), else the one the OS gave
    /// at the last look.
    cwd: Option<PathBuf>,
    /// The foreground program, when it isn't the shell, at the last look.
    program: Option<Vec<String>>,
    /// Marks seen in the output (see [`crate::read::MarkScanner`]), oldest
    /// first, at most [`MAX_MARKS`].
    marks: VecDeque<(String, i32)>,
}
type Shared = Arc<Mutex<Seen>>;
const MAX_MARKS: usize = 64;

fn seen(shared: &Shared) -> std::sync::MutexGuard<'_, Seen> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// What to run. Defaults to the user's login shell.
#[derive(Default)]
pub struct Spawn {
    /// Program and arguments; `None` runs the default shell.
    pub argv: Option<Vec<String>>,
    pub cwd: Option<std::path::PathBuf>,
    pub env: Vec<(String, String)>,
}

/// `argv` (the login shell when it's `None` or empty) with the environment
/// every pane's program gets: other terminals' variables gone, and ours.
pub fn command(argv: Option<&[String]>) -> CommandBuilder {
    let mut cmd = match argv {
        Some([program, args @ ..]) => {
            let mut c = CommandBuilder::new(program);
            c.args(args);
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
    cmd
}

/// Handle to a session. Dropping it kills the shell and stops the threads.
pub struct Session {
    tx: channel::Sender<Msg>,
    seen: Shared,
    thread: Option<JoinHandle<()>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    /// The name of the program the session started (its shell).
    shell: Option<String>,
}

impl Session {
    /// Start a shell on a new PTY.
    pub fn spawn(spawn: Spawn, size: Resize) -> io::Result<Self> {
        let pty = native_pty_system();
        let pair = pty.openpty(pty_size(size)).map_err(io::Error::other)?;

        let mut cmd = command(spawn.argv.as_deref());
        for (k, v) in &spawn.env {
            cmd.env(k, v);
        }
        let dir = spawn.cwd.clone().or_else(proto::paths::home_dir);
        if let Some(dir) = &dir {
            cmd.cwd(dir);
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(io::Error::other)?;
        // Our copy of the slave must close, or reads never see the end.
        drop(pair.slave);
        let killer = child.clone_killer();
        let shell_pid = child.process_id();
        // Started (exec'd) by now.
        let shell = shell_pid.and_then(|p| procinfo::name(i32::try_from(p).ok()?));
        let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;

        let (tx, rx) = channel::unbounded();
        let output = spawn_reader(reader);
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
        // Until the first look, the directory it started in.
        let seen = Shared::new(Mutex::new(Seen {
            cwd: dir,
            ..Seen::default()
        }));
        let shared = seen.clone();
        let thread = thread::Builder::new()
            .name("term-session".into())
            .spawn(move || {
                let pty = Pty { master, shell_pid };
                run(
                    Inbox { rx, output },
                    size,
                    pty_writer,
                    Some(pty),
                    true,
                    debug_recorder(size),
                    shared,
                );
            })?;
        Ok(Self {
            tx,
            seen,
            thread: Some(thread),
            killer: Some(killer),
            shell,
        })
    }

    /// A session over caller-provided I/O, for tests and replay: `output`
    /// plays the role of the PTY, and bytes for the PTY go to `input`.
    pub fn with_io(
        size: Resize,
        output: impl Read + Send + 'static,
        input: impl Write + Send + 'static,
    ) -> io::Result<Self> {
        let (tx, rx) = channel::unbounded();
        let output = spawn_reader(Box::new(output));
        let pty_writer = spawn_writer(Box::new(input))?;
        let seen = Shared::default();
        let shared = seen.clone();
        let thread = thread::Builder::new()
            .name("term-session".into())
            .spawn(move || {
                run(
                    Inbox { rx, output },
                    size,
                    pty_writer,
                    None,
                    false,
                    None,
                    shared,
                );
            })?;
        Ok(Self {
            tx,
            seen,
            thread: Some(thread),
            killer: None,
            shell: None,
        })
    }

    /// Report the exit and foreground changes to `sink` (replacing any
    /// earlier one). An exit before there is one waits for it.
    pub fn on_event(&self, sink: impl FnMut(Event) + Send + 'static) {
        self.send(Msg::Events(Box::new(sink)));
    }

    /// Send frames to `sink` as view `id`, starting with a full frame.
    /// Replaces an earlier view with the same id (a reloaded frontend).
    pub fn view(&self, id: ViewId, sink: impl FnMut(Frame) + Send + 'static) {
        self.send(Msg::View(id, Box::new(sink)));
    }

    /// Stop sending frames to view `id`.
    pub fn unview(&self, id: ViewId) {
        self.send(Msg::Unview(id));
    }

    pub fn input(&self, e: InputEvent) {
        self.send(Msg::Input(e));
    }

    pub fn resize(&self, size: Resize) {
        self.send(Msg::Resize(size));
    }

    /// View `id` painted every frame up to `seq`.
    pub fn ack(&self, id: ViewId, seq: u64) {
        self.send(Msg::Ack(id, seq));
    }

    /// View `id` missed a frame: it gets a full one next.
    pub fn request_full(&self, id: ViewId) {
        self.send(Msg::RequestFull(id));
    }

    /// Use `theme`'s colors; the screen and scrollback redraw in them.
    pub fn set_theme(&self, theme: proto::theme::Theme) {
        self.send(Msg::Theme(theme));
    }

    /// The shell's working directory: what it last reported (OSC 7), else
    /// what the OS says for the foreground program or the shell, as of at
    /// most [`LOOK_INTERVAL`] after the last output. `None` when unknown.
    pub fn cwd(&self) -> Option<PathBuf> {
        seen(&self.seen).cwd.clone()
    }

    /// The title the tab shows: the program's own, else its name.
    pub fn title(&self) -> String {
        seen(&self.seen).title.clone()
    }

    /// The name of the program the session started, its shell (`zsh`,
    /// `pwsh`), when known.
    pub fn shell(&self) -> Option<&str> {
        self.shell.as_deref()
    }

    /// The program running in the foreground, `argv[0]` first, when it
    /// isn't the shell itself. `None` at the prompt, or when unknown. As
    /// fresh as [`Session::cwd`].
    pub fn program(&self) -> Option<Vec<String>> {
        seen(&self.seen).program.clone()
    }

    /// Put `text` on the screen as if the program had printed it, without
    /// it reaching the program: Kibann's own notes in a pane.
    pub fn show(&self, text: &str) {
        self.send(Msg::Show(text.as_bytes().to_vec()));
    }

    /// The pane's text (see [`crate::read::text`]). `None` if the session
    /// is gone, or still busy with one pass after two seconds.
    pub fn read(&self, source: ReadSource, lines: Option<usize>) -> Option<String> {
        let (tx, rx) = mpsc::channel();
        self.send(Msg::Read(source, lines, tx));
        rx.recv_timeout(Duration::from_secs(2)).ok()
    }

    /// The code printed with `mark`, once the output has carried it.
    pub fn mark(&self, mark: &str) -> Option<i32> {
        let seen = seen(&self.seen);
        seen.marks
            .iter()
            .rev()
            .find(|(m, _)| m == mark)
            .map(|(_, c)| *c)
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

fn pty_size(s: Resize) -> PtySize {
    PtySize {
        rows: s.rows.max(1),
        cols: s.cols.max(1),
        pixel_width: s.cols.saturating_mul(s.cell_w),
        pixel_height: s.rows.saturating_mul(s.cell_h),
    }
}

fn spawn_reader(mut r: Box<dyn Read + Send>) -> Receiver<Output> {
    let (tx, rx) = channel::bounded(QUEUE);
    let _ = thread::Builder::new()
        .name("term-reader".into())
        .spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match r.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send(Output::Data(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let _ = tx.send(Output::End);
        });
    rx
}

/// The session thread's two queues: control messages, then output.
struct Inbox {
    rx: Receiver<Msg>,
    output: Receiver<Output>,
}

/// One message from an [`Inbox`].
enum In {
    Msg(Msg),
    Output(Output),
}

impl Inbox {
    /// Wait until `wake` (forever if `None`) for a message, control first;
    /// output only while `output` (the reader hasn't ended). `None` on
    /// timeout; `Err` once nothing can send any more.
    fn wait(&self, wake: Option<Instant>, output: bool) -> Result<Option<In>, ()> {
        if let Some(m) = self.next(output) {
            return Ok(Some(m));
        }
        let never = channel::never();
        let out = if output { &self.output } else { &never };
        let timeout = match wake {
            Some(w) => channel::after(w.saturating_duration_since(Instant::now())),
            None => channel::never(),
        };
        channel::select_biased! {
            recv(self.rx) -> m => m.map(|m| Some(In::Msg(m))).map_err(|_| ()),
            // A reader that's gone without saying so has ended too.
            recv(out) -> o => Ok(Some(In::Output(o.unwrap_or(Output::End)))),
            recv(timeout) -> _ => Ok(None),
        }
    }

    /// The next waiting message, control first; output only if `output`.
    fn next(&self, output: bool) -> Option<In> {
        if let Ok(m) = self.rx.try_recv() {
            return Some(In::Msg(m));
        }
        if !output {
            return None;
        }
        match self.output.try_recv() {
            Ok(o) => Some(In::Output(o)),
            Err(channel::TryRecvError::Empty) => None,
            Err(channel::TryRecvError::Disconnected) => Some(In::Output(Output::End)),
        }
    }
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

/// How long one pass of searching may take, and the pause between passes.
const FIND_SLICE: Duration = Duration::from_millis(4);

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
        let shell = self.shell_pid.and_then(|p| i32::try_from(p).ok())?;
        Some(procinfo::newest_descendant(shell).unwrap_or(shell))
    }

    /// What runs in the foreground, and its arguments when it isn't the
    /// shell.
    fn look(&self, emu: &Emulator) -> (Foreground, Option<Vec<String>>) {
        let fg = self.foreground_pid();
        let shell = self.shell_pid.and_then(|p| i32::try_from(p).ok());
        let at_shell = fg.is_some() && fg == shell;
        let argv = fg.and_then(procinfo::argv);
        let agent = fg.and_then(|pid| {
            let name = procinfo::name(pid).unwrap_or_default();
            agent::identify(&name, argv.as_deref().unwrap_or_default())
        });
        let agent = agent.map(|a| {
            let text = crate::read::text(
                emu.terminal(),
                emu.size(),
                proto::wire::ReadSource::Visible,
                None,
            )
            .unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            (a, agent::detect(a, &emu.title(), &lines))
        });
        let program = argv.filter(|a| !at_shell && !a.is_empty());
        let fg = Foreground {
            agent,
            at_shell,
            cwd: emu.reported_dir().or_else(|| self.cwd()),
            entered: 0,
        };
        (fg, program)
    }

    fn cwd(&self) -> Option<PathBuf> {
        let fg = self.foreground_pid()?;
        procinfo::cwd(fg).or_else(|| {
            let shell = i32::try_from(self.shell_pid?).ok()?;
            procinfo::cwd(shell)
        })
    }
}

/// Bytes the emulator has for the program (keys, replies to queries).
fn send_outbox(emu: &mut Emulator, pty: &Sender<Vec<u8>>, rec: &mut Option<Recorder>) {
    let out = emu.take_outbox();
    if !out.is_empty() {
        if let Some(r) = rec {
            let _ = r.input(&out);
            let _ = r.flush();
        }
        let _ = pty.send(out);
    }
}

fn run(
    inbox: Inbox,
    size: Resize,
    pty: Sender<Vec<u8>>,
    master: Option<Pty>,
    report_exit: bool,
    mut rec: Option<Recorder>,
    shared: Shared,
) {
    let mut scanner = crate::read::MarkScanner::default();
    let Ok(mut emu) = Emulator::new(size) else {
        return;
    };
    let mut p = Pacer::default();
    let mut owner: Option<Sink> = None;
    let mut output_done = false;
    // Exit code and when it arrived.
    let mut exit: Option<(Option<u32>, Instant)> = None;
    let mut exit_sent = false;
    let mut deadline: Option<Instant> = None;
    // Next autoscroll step while a selection drag is held past an edge.
    let mut autoscroll: Option<Instant> = None;
    // When to search on, while a search has more scrollback to look through.
    let mut find_due: Option<Instant> = None;
    // The next look at what runs in the pane, the last one, and what it saw.
    let mut look_due: Option<Instant> = master.as_ref().map(|_| Instant::now());
    let mut looked: Option<Instant> = None;
    let mut foreground: Option<Foreground> = None;
    let mut os_cwd: Option<PathBuf> = seen(&shared).cwd.clone();
    let mut entered: u64 = 0;

    loop {
        let exit_wait = exit
            .filter(|_| !output_done && !exit_sent)
            .map(|(_, t)| t + EXIT_GRACE);
        let wake = [deadline, exit_wait, autoscroll, look_due, find_due]
            .into_iter()
            .flatten()
            .min();
        let Ok(first) = inbox.wait(wake, !output_done) else {
            return;
        };

        let mut budget = OUTPUT_BUDGET;
        let mut next = first;
        while let Some(m) = next.take() {
            let control = matches!(m, In::Msg(_));
            match m {
                In::Output(Output::Data(data)) => {
                    budget = budget.saturating_sub(data.len());
                    emu.write(&data);
                    p.changed();
                    if let Some(due) = look_due {
                        let now = Instant::now();
                        let soon = looked.map_or(now, |t| (t + LOOK_INTERVAL).max(now));
                        look_due = Some(due.min(soon));
                    }
                    scanner.scan(&data, |mark, code| {
                        let marks = &mut seen(&shared).marks;
                        if marks.len() == MAX_MARKS {
                            marks.pop_front();
                        }
                        marks.push_back((mark, code));
                    });
                    if let Some(r) = &mut rec {
                        let _ = r.output(&data);
                    }
                }
                In::Output(Output::End) => output_done = true,
                In::Msg(Msg::ChildExited(code)) => exit = Some((code, Instant::now())),
                In::Msg(Msg::Input(e)) => {
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
                    if enters(&e) {
                        entered += 1;
                    }
                    let _ = emu.input(&e);
                    // Selection and scrolling change the screen with no output.
                    p.changed();
                }
                In::Msg(Msg::Resize(s)) => {
                    if let Some(r) = &mut rec {
                        let _ = r.resize(s.cols, s.rows);
                    }
                    if let Ok(true) = emu.resize(s) {
                        if let Some(m) = &master {
                            let _ = m.master.resize(pty_size(s));
                        }
                        p.changed_all();
                    }
                }
                In::Msg(Msg::Ack(id, seq)) => p.ack(id, seq),
                In::Msg(Msg::RequestFull(id)) => p.request_full(id),
                In::Msg(Msg::Theme(t)) => {
                    let _ = emu.set_theme(&t);
                    p.changed_all();
                }
                In::Msg(Msg::Events(sink)) => {
                    owner = Some(sink);
                    // It hears what runs now, even if an earlier look saw it.
                    foreground = None;
                    if look_due.is_some() {
                        look_due = Some(Instant::now());
                    }
                }
                In::Msg(Msg::View(id, sink)) => p.add_view(id, sink),
                In::Msg(Msg::Unview(id)) => p.remove_view(id),
                In::Msg(Msg::Show(data)) => {
                    emu.write(&data);
                    p.changed();
                }
                In::Msg(Msg::Read(source, lines, reply)) => {
                    let text = crate::read::text(emu.terminal(), emu.size(), source, lines);
                    let _ = reply.send(text.unwrap_or_default());
                }
                In::Msg(Msg::Shutdown) => return,
            }
            // What a key or a reply to the program sends goes out now,
            // not after the rest of the output in this pass.
            if control {
                send_outbox(&mut emu, &pty, &mut rec);
            }
            // Control messages keep coming through after the output budget.
            next = inbox.next(budget > 0 && !output_done);
        }

        // The foreground program changes as programs start, exit or exec,
        // which comes with output; the lookup is a syscall or two.
        if p.dirty()
            && let Some(m) = &master
        {
            emu.set_program_name(m.foreground_pid().and_then(procinfo::name));
        }
        // Paths in links are relative to the shell's directory. Only looked
        // up while hovering, and only when the shell doesn't report it.
        if emu.hovering()
            && emu.reported_dir().is_none()
            && let Some(m) = &master
        {
            emu.set_cwd(m.cwd());
        }

        send_outbox(&mut emu, &pty, &mut rec);

        let now = Instant::now();
        if emu.autoscrolling() {
            let due = *autoscroll.get_or_insert(now + AUTOSCROLL_INTERVAL);
            if now >= due {
                let _ = emu.autoscroll_tick();
                p.changed();
                autoscroll = Some(now + AUTOSCROLL_INTERVAL);
            }
        } else {
            autoscroll = None;
        }
        // Searching: a slice of work per pass (output and input go first),
        // and again shortly while the scrollback isn't all searched.
        if emu.finding() && (p.dirty() || find_due.is_some_and(|t| now >= t)) {
            let more = emu.find_work(FIND_SLICE);
            find_due = more.then(|| now + FIND_SLICE);
            p.changed();
        } else if !emu.finding() {
            find_due = None;
        }
        deadline = p.tick(&mut emu, now);

        let mut program = None;
        if let Some(due) = look_due
            && now >= due
            && let Some(m) = &master
        {
            look_due = Some(now + LOOK_IDLE);
            looked = Some(now);
            let (fg, argv) = m.look(&emu);
            os_cwd = fg.cwd.clone();
            program = Some(argv);
            let fg = Foreground { entered, ..fg };
            if foreground.as_ref() != Some(&fg) {
                foreground = Some(fg.clone());
                if let Some(sink) = owner.as_mut() {
                    sink(Event::Foreground(fg));
                }
            }
        }
        {
            let mut seen = seen(&shared);
            let title = emu.title();
            if seen.title != title {
                seen.title = title;
            }
            seen.cwd = emu.reported_dir().or_else(|| os_cwd.clone());
            if let Some(argv) = program {
                seen.program = argv;
            }
        }

        if report_exit
            && let Some((code, at)) = exit
            && (output_done || now >= at + EXIT_GRACE)
            && !exit_sent
            && let Some(sink) = owner.as_mut()
        {
            // The last output goes on screen first, to the views keeping
            // up (a captured frame before it).
            p.flush(&mut emu, now);
            sink(Event::Exited(code));
            exit_sent = true;
        }
    }
}
