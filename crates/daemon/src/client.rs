//! Talking to the daemon: requests with replies, and a callback for frames and
//! exits. Frames are handed over as JSON text, not decoded, so a client that
//! passes them on (the app, to its webview) does no per-frame work.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use proto::layout::{Action, Layout, PaneId, TabId};
use proto::wire::{
    ClientMsg, PROTOCOL_VERSION, RawServerMsg, Reply, ReqId, ServerMsg, SessionId, read_msg,
    write_msg,
};

use crate::pipe::pipe;
use crate::server::Daemon;

/// How long a request may take. Spawning looks up a directory, which can
/// take up to half a second on a busy session.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Something the daemon sent that isn't a reply.
#[derive(Debug)]
pub enum ClientEvent<'a> {
    /// A frame for `session`, as JSON (a [`proto::Frame`]).
    Frame { session: SessionId, json: &'a str },
    /// The layout changed (after [`Client::watch`]).
    Layout(Layout),
    /// The connection closed: the daemon exited or crashed.
    Disconnected,
}

type Pending = Arc<Mutex<HashMap<ReqId, Sender<Reply>>>>;

pub struct Client {
    writer: Mutex<Box<dyn Write + Send>>,
    pending: Pending,
    next_req: AtomicU32,
    /// The daemon itself, when it runs in this process.
    _local: Option<Arc<Daemon>>,
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
                    while let Ok(Some(msg)) = read_msg::<_, RawServerMsg>(&mut reader) {
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
            _local: None,
            hangup: None,
        })
    }

    /// Run the daemon inside this process (where there is no socket support
    /// yet). Its sessions end with the process, as before the daemon existed.
    pub fn in_process(on_event: impl FnMut(ClientEvent<'_>) + Send + 'static) -> io::Result<Self> {
        let daemon = Daemon::new("in-process");
        let (to_daemon, daemon_reads) = pipe();
        let (daemon_writes, from_daemon) = pipe();
        daemon.serve(daemon_reads, daemon_writes)?;
        let mut c = Self::new(from_daemon, to_daemon, on_event)?;
        c._local = Some(daemon);
        Ok(c)
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

    /// The daemon's protocol version and build id.
    pub fn hello(&self) -> io::Result<(u32, String)> {
        match self.request(|req| ClientMsg::Hello {
            req,
            version: PROTOCOL_VERSION,
        })? {
            Reply::Hello { version, build } => Ok((version, build)),
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

    pub fn layout(&self) -> io::Result<Layout> {
        match self.request(|req| ClientMsg::GetLayout { req })? {
            Reply::Layout { layout } => Ok(layout),
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
        Reply::Error { message } => io::Error::other(message),
        other => io::Error::other(format!("unexpected reply: {other:?}")),
    }
}

#[cfg(unix)]
mod unix {
    use std::ffi::OsString;
    use std::io;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{Client, ClientEvent};

    /// A client over `stream` that shuts it down when dropped.
    fn over(
        stream: UnixStream,
        on_event: impl FnMut(ClientEvent<'_>) + Send + 'static,
    ) -> io::Result<Client> {
        let handle = stream.try_clone()?;
        let mut c = Client::new(stream.try_clone()?, stream, on_event)?;
        c.hangup = Some(Box::new(move || {
            let _ = handle.shutdown(std::net::Shutdown::Both);
        }));
        Ok(c)
    }

    /// How long to wait for a daemon we started to start listening.
    const START_TIMEOUT: Duration = Duration::from_secs(3);

    impl Client {
        /// Connect to a running daemon.
        pub fn connect(
            socket: &Path,
            on_event: impl FnMut(ClientEvent<'_>) + Send + 'static,
        ) -> io::Result<Self> {
            over(UnixStream::connect(socket)?, on_event)
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
            if let Ok(s) = UnixStream::connect(socket) {
                return over(s, on_event);
            }
            start_daemon(socket, command)?;
            let deadline = Instant::now() + START_TIMEOUT;
            loop {
                match UnixStream::connect(socket) {
                    Ok(s) => return over(s, on_event),
                    Err(e) if Instant::now() >= deadline => return Err(e),
                    Err(_) => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        }
    }

    /// Start `command` as the daemon for `socket`: in its own session, so
    /// closing the app (or its terminal) doesn't signal it, with output going
    /// to a log file beside the socket.
    pub fn start_daemon(socket: &Path, command: &[OsString]) -> io::Result<()> {
        let (exe, args) = command
            .split_first()
            .ok_or_else(|| io::Error::other("no daemon command"))?;
        let dir = socket
            .parent()
            .ok_or_else(|| io::Error::other("socket path has no directory"))?;
        crate::paths::prepare_dir(dir)?;
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
            .current_dir("/");
        // SAFETY: setsid is async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn()?;
        // Reap it when it exits, so it doesn't linger as a zombie while the
        // app runs.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
}

#[cfg(unix)]
pub use unix::start_daemon;
