//! Talking to the daemon: requests with replies, and a callback for frames and
//! exits. Frames are handed over as JSON text, not decoded, so a client that
//! passes them on (the app, to its webview) does no per-frame work.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use proto::git::PaneDir;
use proto::layout::{Action, Layout, PaneId, TabId};
use proto::status::PaneStatus;
use proto::wire::{ClientMsg, PROTOCOL_VERSION, RawServerMsg, Reply, ReqId, ServerMsg, write_msg};

/// How long a request may take. Spawning looks up a directory, which can
/// take up to half a second on a busy session.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Something the daemon sent that isn't a reply.
#[derive(Debug)]
pub enum ClientEvent<'a> {
    /// A frame for `session`, as JSON (a [`proto::Frame`]).
    Frame { session: PaneId, json: &'a str },
    /// The layout changed (after [`Client::watch`]).
    Layout(Layout),
    /// Every pane with an agent, whenever one changes (after
    /// [`Client::watch`]).
    Status(BTreeMap<PaneId, PaneStatus>),
    /// Every pane's directory and checkout, whenever one changes (after
    /// [`Client::watch`]).
    Git(BTreeMap<PaneId, PaneDir>),
    /// The connection closed: the daemon exited or crashed.
    Disconnected,
}

type Pending = Arc<Mutex<HashMap<ReqId, Sender<Reply>>>>;

pub struct Client {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    next_req: AtomicU32,
    /// Closes the connection both ways. The reader thread holds its own
    /// handle to the stream, so dropping the writer alone wouldn't.
    hangup: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(h) = &self.hangup {
            h();
        }
    }
}

impl Client {
    /// A client over any pair of streams; `on_event` runs on the reader thread.
    pub fn new(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        mut on_event: impl FnMut(ClientEvent<'_>) + Send + 'static,
    ) -> io::Result<Self> {
        let pending: Pending = Arc::default();
        {
            let pending = pending.clone();
            let mut reader = reader;
            thread::Builder::new()
                .name("daemon-client".into())
                .spawn(move || {
                    while let Ok(Some(body)) = proto::wire::read_raw(&mut reader) {
                        // A message this build doesn't know (from a newer
                        // daemon) is skipped, not taken as the end.
                        let Ok(msg) = serde_json::from_slice::<RawServerMsg>(&body) else {
                            continue;
                        };
                        match msg {
                            ServerMsg::Reply { req, reply } => {
                                if let Some(tx) = pending.lock().unwrap().remove(&req) {
                                    let _ = tx.send(reply);
                                }
                            }
                            ServerMsg::Frame { session, frame } => on_event(ClientEvent::Frame {
                                session,
                                json: frame.get(),
                            }),
                            ServerMsg::Layout { layout } => on_event(ClientEvent::Layout(layout)),
                            ServerMsg::Status { panes } => on_event(ClientEvent::Status(panes)),
                            ServerMsg::Git { panes } => on_event(ClientEvent::Git(panes)),
                        }
                    }
                    // Requests still waiting fail now rather than time out.
                    pending.lock().unwrap().clear();
                    on_event(ClientEvent::Disconnected);
                })?;
        }
        Ok(Self {
            writer: Mutex::new(Box::new(writer)),
            pending,
            next_req: AtomicU32::new(0),
            hangup: None,
        })
    }

    /// Run `f` when the client is dropped (and the connection closes):
    /// what keeps the other end alive, or shuts it down.
    pub fn on_drop(mut self, f: impl Fn() + Send + Sync + 'static) -> Self {
        self.hangup = Some(Box::new(f));
        self
    }

    pub fn send(&self, msg: &ClientMsg) -> io::Result<()> {
        write_msg(&mut *self.writer.lock().unwrap(), msg)
    }

    /// Send a request built around a fresh request number and wait for its reply.
    pub fn request(&self, make: impl FnOnce(ReqId) -> ClientMsg) -> io::Result<Reply> {
        let req = self.next_req.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(req, tx);
        if let Err(e) = self.send(&make(req)) {
            self.pending.lock().unwrap().remove(&req);
            return Err(e);
        }
        rx.recv_timeout(REQUEST_TIMEOUT).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => {
                self.pending.lock().unwrap().remove(&req);
                io::Error::new(io::ErrorKind::TimedOut, "the daemon didn't answer")
            }
            mpsc::RecvTimeoutError::Disconnected => {
                io::Error::new(io::ErrorKind::ConnectionAborted, "the daemon went away")
            }
        })
    }

    /// Who the daemon is: works with a daemon of any version.
    pub fn hello(&self) -> io::Result<Hello> {
        match self.request(|req| ClientMsg::Hello {
            req,
            version: PROTOCOL_VERSION,
        })? {
            Reply::Hello {
                version,
                build,
                busy,
            } => Ok(Hello {
                version,
                build,
                busy,
            }),
            other => Err(unexpected(other)),
        }
    }

    /// The layout now, and every change from then on as
    /// [`ClientEvent::Layout`].
    pub fn watch(&self) -> io::Result<Layout> {
        match self.request(|req| ClientMsg::Watch { req })? {
            Reply::Layout { layout } => Ok(layout),
            other => Err(unexpected(other)),
        }
    }

    /// The user is looking at `pane` through this client, or at none
    /// (see [`ClientMsg::Looking`]).
    pub fn looking(&self, pane: Option<PaneId>) -> io::Result<()> {
        self.send(&ClientMsg::Looking { pane })
    }

    pub fn layout(&self) -> io::Result<Layout> {
        match self.request(|req| ClientMsg::GetLayout { req })? {
            Reply::Layout { layout } => Ok(layout),
            other => Err(unexpected(other)),
        }
    }

    /// With no panes open, reopen the saved layout; returns how many tabs
    /// and panes came back (zeros: none).
    pub fn restore(&self) -> io::Result<(u32, u32)> {
        match self.request(|req| ClientMsg::Restore { req })? {
            Reply::Restored { tabs, panes } => Ok((tabs, panes)),
            other => Err(unexpected(other)),
        }
    }

    /// Change the layout; returns the pane and tab made or acted on.
    pub fn act(&self, action: Action) -> io::Result<(Option<PaneId>, Option<TabId>)> {
        match self.request(|req| ClientMsg::Act { req, action })? {
            Reply::Acted { pane, tab } => Ok((pane, tab)),
            other => Err(unexpected(other)),
        }
    }
}

/// A reply that isn't the one asked for, as an error.
pub fn unexpected(reply: Reply) -> io::Error {
    match reply {
        Reply::Error { message, .. } => io::Error::other(message),
        other => io::Error::other(format!("unexpected reply: {other:?}")),
    }
}

/// A daemon's answer to [`Client::hello`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hello {
    pub version: u32,
    /// Its fingerprint (`daemon::BUILD` where it was built).
    pub build: String,
    /// What runs in its panes besides their shells, one name a pane
    /// (`None`: a daemon too old to say).
    pub busy: Option<Vec<String>>,
}

/// How a running daemon compares with the one this program comes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fit {
    /// Built from the same code.
    Same,
    /// Speaks this protocol but its terminal code is another build's: it
    /// works, with the earlier terminal behaviour, until it's replaced.
    OtherBuild,
    /// Speaks another protocol: only [`Client::hello`] and
    /// [`Client::stop`] work with it.
    OtherProtocol,
}

impl Hello {
    /// How this daemon fits a program whose own daemon is `build`.
    pub fn fit(&self, build: &str) -> Fit {
        if self.version != PROTOCOL_VERSION {
            Fit::OtherProtocol
        } else if self.build != build {
            Fit::OtherBuild
        } else {
            Fit::Same
        }
    }

    /// Ending it ends no program: every shell is at its prompt.
    pub fn idle(&self) -> bool {
        self.busy.as_ref().is_some_and(|b| b.is_empty())
    }
}

/// "claude, vim and 2 more", for saying what ending the daemon would end.
pub fn list_programs(programs: &[String]) -> String {
    let mut names: Vec<&str> = Vec::new();
    for p in programs {
        if !names.contains(&p.as_str()) {
            names.push(p);
        }
    }
    match names.as_slice() {
        [] => "a program".to_owned(),
        [one] => (*one).to_owned(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}

/// How long to wait for a daemon we started to start listening.
const START_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a daemon may take to end its sessions and exit.
const STOP_TIMEOUT: Duration = Duration::from_secs(10);

pub use endpoint::lock_held;

/// Wait until no daemon holds `socket`'s lock.
fn wait_for_exit(socket: &Path, timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    while lock_held(socket) {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the terminal service didn't exit",
            ));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

impl Client {
    /// A client over `conn` that hangs it up when dropped.
    fn over(
        conn: endpoint::Conn,
        on_event: impl FnMut(ClientEvent<'_>) + Send + 'static,
    ) -> io::Result<Self> {
        let mut c = Client::new(conn.reader, conn.writer, on_event)?;
        c.hangup = Some(conn.hangup);
        Ok(c)
    }

    /// Connect to a running daemon.
    pub fn connect(
        socket: &Path,
        on_event: impl FnMut(ClientEvent<'_>) + Send + 'static,
    ) -> io::Result<Self> {
        Self::over(endpoint::connect(socket)?, on_event)
    }

    /// Connect, starting the daemon with `command` (program and
    /// arguments; `--socket PATH` is added) first if none is running.
    /// Two clients racing start at most one daemon: the second one to
    /// start finds the lock taken and exits, and both clients connect to
    /// the first.
    pub fn connect_or_start(
        socket: &Path,
        command: &[OsString],
        on_event: impl FnMut(ClientEvent<'_>) + Send + 'static,
    ) -> io::Result<Self> {
        if let Ok(c) = endpoint::connect(socket) {
            return Self::over(c, on_event);
        }
        // One that's ending still holds the lock, and a new one would
        // find it taken and give up.
        wait_for_exit(socket, STOP_TIMEOUT)?;
        start_daemon(socket, command)?;
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            match endpoint::connect(socket) {
                Ok(c) => return Self::over(c, on_event),
                Err(e) if Instant::now() >= deadline => return Err(e),
                Err(_) => thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    /// Ask the daemon on `socket` (of any version) to save its layout,
    /// end its sessions and exit, and wait until it has.
    pub fn stop(self, socket: &Path) -> io::Result<()> {
        self.send(&ClientMsg::Shutdown)?;
        drop(self);
        wait_for_exit(socket, STOP_TIMEOUT)
    }
}

/// Start `command` as the daemon for `socket`, on its own (closing the
/// app, or its terminal, doesn't end it), with output going to a log file
/// beside the socket.
pub fn start_daemon(socket: &Path, command: &[OsString]) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let (exe, args) = command
        .split_first()
        .ok_or_else(|| io::Error::other("no daemon command"))?;
    let dir = socket
        .parent()
        .ok_or_else(|| io::Error::other("socket path has no directory"))?;
    #[cfg(unix)]
    proto::paths::prepare_dir(dir)?;
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir)?;
    let mut log_path = socket.as_os_str().to_owned();
    log_path.push(".log");
    let log = std::fs::File::options()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        // Don't keep the app's directory busy.
        .current_dir(dir);
    detach(&mut cmd)?;
    let mut child = cmd.spawn()?;
    // Reap it when it exits, so it doesn't linger as a zombie while the
    // app runs.
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// In a session of its own, so no terminal's hang-up signals reach it.
#[cfg(unix)]
fn detach(cmd: &mut std::process::Command) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    // SAFETY: setsid is async-signal-safe.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(())
}

/// With no console (closing the starter's doesn't end it) and out of the
/// starter's Ctrl+C group.
#[cfg(windows)]
fn detach(cmd: &mut std::process::Command) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    Ok(())
}
