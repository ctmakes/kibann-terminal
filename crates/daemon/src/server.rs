//! The daemon: owns the sessions and serves clients over byte streams.
//!
//! Each connection gets a reader thread (decodes requests and acts on them)
//! and a writer thread (drains the connection's [`Outbox`]). Sessions send
//! frames from their own threads into their viewer's outbox, so a slow client
//! never stalls a session or other clients.
//!
//! A session always has a sink. With no viewer it only notes the exit; frames
//! stop after two unacked ones (the session's own pacing), so a detached
//! session costs nothing but its emulator keeps up with the output.

use std::collections::{BTreeMap, HashMap};
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use proto::Frame;
use proto::wire::{
    ClientMsg, PROTOCOL_VERSION, Reply, ReqId, ServerMsg, SessionId, read_msg, write_msg,
};
use term::{Event, Session, Spawn};

use crate::outbox::Outbox;

type ConnId = u64;

struct Entry {
    session: Arc<Session>,
    /// The connection the frames go to.
    viewer: Option<ConnId>,
    exited: Arc<AtomicBool>,
}

#[derive(Default)]
struct State {
    sessions: BTreeMap<SessionId, Entry>,
    next_session: SessionId,
    conns: HashMap<ConnId, Arc<Outbox>>,
    next_conn: ConnId,
    layout: Option<String>,
    shutdown: bool,
}

pub struct Daemon {
    state: Mutex<State>,
    build: String,
}

/// One message, framed, ready for an outbox.
fn encode(msg: &ServerMsg) -> Vec<u8> {
    let mut buf = Vec::new();
    // Writing to a Vec only fails for a message over the size cap.
    let _ = write_msg(&mut buf, msg);
    buf
}

/// Where a session's events go: its viewer's outbox, if any.
fn sink(
    id: SessionId,
    out: Option<Arc<Outbox>>,
    exited: Arc<AtomicBool>,
) -> impl FnMut(Event) + Send + 'static {
    move |event| match event {
        Event::Frame(frame) => {
            if let Some(o) = &out {
                o.push_frame(encode(&ServerMsg::Frame { session: id, frame }));
            }
        }
        Event::Exited(code) => {
            exited.store(true, Ordering::SeqCst);
            if let Some(o) = &out {
                o.push(encode(&ServerMsg::<Frame>::Exited { session: id, code }));
            }
        }
    }
}

impl Daemon {
    /// `build` identifies this daemon's binary; see [`crate::file_fingerprint`].
    pub fn new(build: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            build: build.into(),
        })
    }

    /// A panic on one connection's thread mustn't lock everyone out.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Serve one client: requests are read from `reader`, replies and frames
    /// written to `writer`, each on its own thread. Returns at once.
    pub fn serve(
        self: &Arc<Self>,
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
    ) -> io::Result<()> {
        let out = Arc::new(Outbox::new());
        let conn = {
            let mut st = self.lock();
            st.next_conn += 1;
            let id = st.next_conn;
            st.conns.insert(id, out.clone());
            id
        };
        {
            let out = out.clone();
            let mut writer = writer;
            thread::Builder::new()
                .name("daemon-writer".into())
                .spawn(move || {
                    while let Some(bytes) = out.pop() {
                        if writer
                            .write_all(&bytes)
                            .and_then(|()| writer.flush())
                            .is_err()
                        {
                            out.close();
                            break;
                        }
                    }
                })?;
        }
        let me = self.clone();
        let mut reader = reader;
        thread::Builder::new()
            .name("daemon-reader".into())
            .spawn(move || {
                while let Ok(Some(msg)) = read_msg::<_, ClientMsg>(&mut reader) {
                    me.handle(conn, &out, msg);
                }
                me.disconnect(conn);
            })?;
        Ok(())
    }

    fn reply(out: &Outbox, req: ReqId, reply: Reply) {
        out.push(encode(&ServerMsg::<Frame>::Reply { req, reply }));
    }

    fn handle(&self, conn: ConnId, out: &Arc<Outbox>, msg: ClientMsg) {
        // Calls on one session, made outside the lock.
        let with = |id: SessionId, f: &dyn Fn(&Session)| {
            let s = self.lock().sessions.get(&id).map(|e| e.session.clone());
            if let Some(s) = s {
                f(&s);
            }
        };
        match msg {
            ClientMsg::Hello { req, .. } => Self::reply(
                out,
                req,
                Reply::Hello {
                    version: PROTOCOL_VERSION,
                    build: self.build.clone(),
                },
            ),
            ClientMsg::Spawn {
                req,
                size,
                cwd_from,
                argv,
            } => {
                let reply = match self.spawn(size, cwd_from, argv) {
                    Ok(session) => Reply::Spawned { session },
                    Err(e) => Reply::Error {
                        message: format!("could not start the shell: {e}"),
                    },
                };
                Self::reply(out, req, reply);
            }
            ClientMsg::List { req } => {
                let ids = self.lock().sessions.keys().copied().collect();
                Self::reply(out, req, Reply::Sessions { ids });
            }
            ClientMsg::Attach { session } => {
                let mut st = self.lock();
                if let Some(e) = st.sessions.get_mut(&session) {
                    e.viewer = Some(conn);
                    e.session
                        .attach(sink(session, Some(out.clone()), e.exited.clone()));
                }
            }
            ClientMsg::Detach { session } => {
                let mut st = self.lock();
                if let Some(e) = st.sessions.get_mut(&session)
                    && e.viewer == Some(conn)
                {
                    e.viewer = None;
                    e.session.attach(sink(session, None, e.exited.clone()));
                }
            }
            ClientMsg::Close { session } => {
                let entry = self.lock().sessions.remove(&session);
                // Dropping ends the shell and joins its thread: not under the lock.
                drop(entry);
            }
            ClientMsg::Input { session, event } => with(session, &|s| s.input(event.clone())),
            ClientMsg::Resize { session, size } => with(session, &|s| s.resize(size)),
            ClientMsg::Ack { session, seq } => with(session, &|s| s.ack(seq)),
            ClientMsg::RequestFull { session } => with(session, &|s| s.request_full()),
            ClientMsg::LayoutGet { req } => {
                let layout = self.lock().layout.clone();
                Self::reply(out, req, Reply::Layout { layout });
            }
            ClientMsg::LayoutSet { layout } => self.lock().layout = Some(layout),
            ClientMsg::Shutdown => self.shutdown(),
        }
    }

    fn spawn(
        &self,
        size: proto::Resize,
        cwd_from: Option<SessionId>,
        argv: Option<Vec<String>>,
    ) -> io::Result<SessionId> {
        let near = cwd_from.and_then(|id| Some(self.lock().sessions.get(&id)?.session.clone()));
        let cwd = near.and_then(|s| s.cwd());
        let session = Session::spawn(
            Spawn {
                argv,
                cwd,
                ..Spawn::default()
            },
            size,
        )?;
        let exited = Arc::new(AtomicBool::new(false));
        let mut st = self.lock();
        st.next_session += 1;
        let id = st.next_session;
        session.attach(sink(id, None, exited.clone()));
        st.sessions.insert(
            id,
            Entry {
                session: Arc::new(session),
                viewer: None,
                exited,
            },
        );
        Ok(id)
    }

    /// The client went away: its sessions keep running, unattached.
    fn disconnect(&self, conn: ConnId) {
        let mut st = self.lock();
        if let Some(out) = st.conns.remove(&conn) {
            out.close();
        }
        for (id, e) in st.sessions.iter_mut() {
            if e.viewer == Some(conn) {
                e.viewer = None;
                e.session.attach(sink(*id, None, e.exited.clone()));
            }
        }
    }

    /// End every session; [`serve_socket`] then exits.
    pub fn shutdown(&self) {
        let sessions = {
            let mut st = self.lock();
            st.shutdown = true;
            for out in st.conns.values() {
                out.close();
            }
            std::mem::take(&mut st.sessions)
        };
        drop(sessions);
    }

    pub fn is_shut_down(&self) -> bool {
        self.lock().shutdown
    }

    pub fn clients(&self) -> usize {
        self.lock().conns.len()
    }

    /// Sessions whose program is still running.
    pub fn live_sessions(&self) -> usize {
        self.lock()
            .sessions
            .values()
            .filter(|e| !e.exited.load(Ordering::SeqCst))
            .count()
    }
}

#[cfg(unix)]
pub use unix::{SocketConfig, serve_socket};

#[cfg(unix)]
mod unix {
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::Daemon;

    pub struct SocketConfig {
        pub socket: PathBuf,
        /// Exit after this long with no clients and no running sessions.
        pub idle_exit: Duration,
    }

    /// The user id of the process at the other end of `s`.
    fn peer_uid(s: &UnixStream) -> Option<u32> {
        let fd = s.as_raw_fd();
        #[cfg(target_os = "linux")]
        {
            let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
            let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
            // SAFETY: `cred` and `len` are valid for SO_PEERCRED.
            let r = unsafe {
                libc::getsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_PEERCRED,
                    (&raw mut cred).cast(),
                    &mut len,
                )
            };
            (r == 0).then_some(cred.uid)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let (mut uid, mut gid) = (0, 0);
            // SAFETY: plain out-parameters.
            let r = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
            (r == 0).then_some(uid)
        }
    }

    /// Only the user who runs the daemon may connect: anyone who can
    /// connect can type into every shell.
    pub fn peer_allowed(s: &UnixStream) -> bool {
        // SAFETY: geteuid has no preconditions.
        peer_uid(s) == Some(unsafe { libc::geteuid() })
    }

    /// Hold an exclusive lock on `path` for as long as the file stays open.
    /// Fails if another daemon holds it.
    fn lock_file(path: &std::path::Path) -> io::Result<File> {
        let f = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        // SAFETY: a valid open file descriptor.
        let r = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if r != 0 {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "another daemon is running",
            ));
        }
        Ok(f)
    }

    /// Listen on the socket and serve until shut down or idle. Fails with
    /// `AddrInUse` if another daemon already serves this socket.
    pub fn serve_socket(daemon: &Arc<Daemon>, cfg: &SocketConfig) -> io::Result<()> {
        let dir = cfg
            .socket
            .parent()
            .ok_or_else(|| io::Error::other("socket path has no directory"))?;
        crate::paths::prepare_dir(dir)?;
        let mut lock_path = cfg.socket.clone().into_os_string();
        lock_path.push(".lock");
        let _lock = lock_file(std::path::Path::new(&lock_path))?;
        // Holding the lock means any socket file left here is stale.
        let _ = std::fs::remove_file(&cfg.socket);
        let listener = UnixListener::bind(&cfg.socket)?;
        std::fs::set_permissions(&cfg.socket, std::fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;

        let mut idle_since: Option<Instant> = None;
        let result = loop {
            if daemon.is_shut_down() {
                break Ok(());
            }
            match listener.accept() {
                Ok((stream, _)) => {
                    if stream.set_nonblocking(false).is_err() || !peer_allowed(&stream) {
                        continue;
                    }
                    match stream.try_clone() {
                        Ok(reader) => daemon.serve(reader, stream)?,
                        Err(_) => continue,
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => break Err(e),
            }
            if daemon.clients() == 0 && daemon.live_sessions() == 0 {
                let since = *idle_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= cfg.idle_exit {
                    break Ok(());
                }
            } else {
                idle_since = None;
            }
        };
        let _ = std::fs::remove_file(&cfg.socket);
        daemon.shutdown();
        result
    }
}
