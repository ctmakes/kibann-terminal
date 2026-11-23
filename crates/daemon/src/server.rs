//! The daemon: owns the sessions and the layout, and serves clients over
//! byte streams.
//!
//! Each connection gets a reader thread (decodes requests and acts on them)
//! and a writer thread (drains the connection's [`Outbox`]). Sessions send
//! frames from their own threads into the outbox of each connection viewing
//! them (a view per connection, paced by its own acks), so a slow client
//! never stalls a session or other clients. A session with no views builds
//! no frames; its emulator keeps up with the output.
//!
//! Every session is a pane in the layout ([`crate::layout::Model`]). Actions
//! from the app or the CLI change it, and each change goes to the
//! connections watching it. A shell that exits takes its pane with it.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use proto::git::PaneDir;
use proto::layout::{Action, Dir, PaneId, TabId};
use proto::status::{AgentState, PaneStatus};
use proto::wire::{
    ClientMsg, ErrorCode, PROTOCOL_VERSION, PaneInfo, Reply, ReqId, ServerMsg, SessionId, write_msg,
};
use proto::{Frame, Resize};
use term::{Event, Foreground, Session, Spawn};

use crate::layout::Model;
use crate::outbox::{Outbox, Snapshot};
use crate::status::Statuses;

type ConnId = u64;

/// Size of a pane started with nothing to size it after. The app resizes
/// it as soon as it shows it.
const DEFAULT_SIZE: Resize = Resize {
    cols: 80,
    rows: 24,
    cell_w: 8,
    cell_h: 16,
};

/// Where a pane is: its directory and the checkout that holds it.
#[derive(Debug, Clone, Default)]
struct Place {
    dir: PathBuf,
    /// The checkout's root (watched in [`Daemon::git`] for this pane).
    root: Option<PathBuf>,
    at_shell: bool,
    /// [`Foreground::entered`] when last seen.
    entered: u64,
}

/// `dir` with the home directory shown as `~`.
fn shown(dir: &std::path::Path) -> String {
    if let Some(home) = proto::paths::home_dir()
        && let Ok(rest) = dir.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        };
    }
    dir.display().to_string()
}

/// How many ended panes to remember, for `kibann pane wait --exit`.
const MAX_ENDED: usize = 256;

struct Entry {
    session: Arc<Session>,
    exited: Arc<AtomicBool>,
    /// Last size the app gave it, to size panes split from it.
    size: Resize,
}

/// What every shell is told about where it runs, so `kibann` in it can
/// find the daemon and its own pane.
#[derive(Debug, Clone, Default)]
pub struct PaneEnv {
    /// The daemon's socket (`KIBANN_SOCKET`).
    pub socket: Option<PathBuf>,
    /// The `kibann` executable (`KIBANN_BIN`).
    pub bin: Option<PathBuf>,
    /// A directory holding a `kibann` link, put first on `PATH`.
    pub path_dir: Option<PathBuf>,
    /// What runs in a pane opened with no program, instead of the login
    /// shell (`KIBANN_COMMAND`, for trying Kibann with vim, htop and the
    /// like).
    pub command: Option<Vec<String>>,
}

#[derive(Default)]
struct State {
    sessions: BTreeMap<SessionId, Entry>,
    next_session: SessionId,
    conns: HashMap<ConnId, Arc<Outbox>>,
    next_conn: ConnId,
    /// Connections that get every layout change.
    watchers: HashSet<ConnId>,
    /// The pane each connection's user is looking at (see
    /// [`ClientMsg::Looking`]).
    looking: HashMap<ConnId, PaneId>,
    model: Model,
    /// Panes whose shell ended, with the exit code, newest last.
    ended: VecDeque<(PaneId, Option<u32>)>,
    env: PaneEnv,
    /// Agent status per pane.
    status: Statuses,
    /// Directory and checkout per pane.
    places: HashMap<PaneId, Place>,
    /// The git map watchers last got.
    git_sent: BTreeMap<PaneId, PaneDir>,
    /// The sessions' colors: the last a client set, else the saved ones;
    /// `None` until then (libghostty's defaults, the same colors as the
    /// built-in dark theme).
    theme: Option<proto::theme::Theme>,
    shutdown: bool,
    /// Keeps the layout on disk, once [`Daemon::save_layout_to`] set a file.
    saver: Option<crate::saved::Saver>,
}

impl State {
    /// Send the layout to every watcher but `skip`. Done under the lock, so
    /// changes reach each connection in order.
    fn broadcast(&mut self, skip: Option<ConnId>) {
        let msg = encode(&ServerMsg::Layout {
            layout: self.model.layout().clone(),
        });
        self.send_watchers(Snapshot::Layout, &msg, skip);
        self.status_changed();
        self.offer_save();
    }

    /// Keep the layout and each pane's directory on disk (soon). Not while
    /// ending everything on purpose: the panes closing then are to come back.
    fn offer_save(&mut self) {
        if self.shutdown || self.saver.is_none() {
            return;
        }
        let workspace = proto::layout::Workspace::of(self.model.layout(), |p| {
            Some(self.places.get(&p)?.dir.to_str()?.to_owned())
        });
        let saved = crate::saved::Saved {
            workspace,
            theme: self.theme.clone(),
        };
        if let Some(s) = &mut self.saver {
            s.offer(&saved);
        }
    }

    fn send_watchers(&self, kind: Snapshot, msg: &[u8], skip: Option<ConnId>) {
        for c in &self.watchers {
            if Some(*c) != skip
                && let Some(out) = self.conns.get(c)
            {
                out.push_latest(kind, msg.to_vec());
            }
        }
    }

    /// Someone is looking at `pane`, through any client.
    fn is_seen(&self, pane: PaneId) -> bool {
        self.looking.values().any(|&p| p == pane)
    }

    /// After anything that may change agent status (or what is seen): clear
    /// done on the panes looked at, and send the map if it changed.
    fn status_changed(&mut self) {
        for &p in self.looking.values() {
            self.status.seen(p);
        }
        if let Some(panes) = self.status.changed() {
            self.send_watchers(
                Snapshot::Status,
                &encode(&ServerMsg::Status { panes }),
                None,
            );
        }
    }

    fn git_map(&self, git: &git::Watcher) -> BTreeMap<PaneId, PaneDir> {
        self.places
            .iter()
            .map(|(&id, p)| {
                let dir = PaneDir {
                    dir: p.dir.to_string_lossy().into_owned(),
                    shown: shown(&p.dir),
                    git: p.root.as_deref().and_then(|r| git.info(r)),
                };
                (id, dir)
            })
            .collect()
    }

    /// Send the git map to watchers if it changed.
    fn git_changed(&mut self, git: &git::Watcher) {
        let now = self.git_map(git);
        if now != self.git_sent {
            self.send_watchers(
                Snapshot::Git,
                &encode(&ServerMsg::Git { panes: now.clone() }),
                None,
            );
            self.git_sent = now;
            // A pane's directory changed.
            self.offer_save();
        }
    }

    /// The pane is gone. Returns the checkout to stop watching for it,
    /// which is for the caller to do outside the lock (see
    /// [`Daemon::git_ops`]).
    #[must_use]
    fn forget_place(&mut self, pane: PaneId, git: &git::Watcher) -> Option<PathBuf> {
        let root = self.places.remove(&pane).and_then(|p| p.root);
        self.git_changed(git);
        root
    }

    /// Of panes just placed in the layout, those with no session go again:
    /// they couldn't start, or their shell ended before they were placed
    /// (so the exit found no pane to take).
    fn drop_ended(&mut self, panes: impl IntoIterator<Item = PaneId>) {
        for p in panes {
            if !self.sessions.contains_key(&p) {
                self.model.remove(p);
            }
        }
    }

    fn note_ended(&mut self, pane: PaneId, code: Option<u32>) {
        if self.ended.len() == MAX_ENDED {
            self.ended.pop_front();
        }
        self.ended.push_back((pane, code));
    }
}

pub struct Daemon {
    state: Mutex<State>,
    build: String,
    me: Weak<Daemon>,
    /// Keeps each checkout's git info current.
    git: git::Watcher,
    /// Held while a pane's checkout changes, from before taking `state`
    /// until the watch is added or removed after releasing it: watching a
    /// big tree takes a while, and mustn't stall everyone holding `state`.
    git_ops: Mutex<()>,
}

/// One message, framed, ready for an outbox.
fn encode(msg: &ServerMsg) -> Vec<u8> {
    let mut buf = Vec::new();
    // Writing to a Vec only fails for a message over the size cap.
    let _ = write_msg(&mut buf, msg);
    buf
}

/// Where a session's events go: to the daemon, which closes the pane when
/// it exits.
fn events(
    id: SessionId,
    exited: Arc<AtomicBool>,
    daemon: Weak<Daemon>,
) -> impl FnMut(Event) + Send + 'static {
    move |event| match event {
        Event::Foreground(fg) => {
            if let Some(d) = daemon.upgrade() {
                d.foreground(id, fg);
            }
        }
        Event::Exited(code) => {
            if exited.swap(true, Ordering::SeqCst) {
                return;
            }
            // Not on this thread: closing the session joins it.
            let daemon = daemon.clone();
            let _ = thread::Builder::new()
                .name("daemon-exit".into())
                .spawn(move || {
                    if let Some(d) = daemon.upgrade() {
                        d.pane_ended(id, code);
                    }
                });
        }
    }
}

/// Half of `size` along `dir`, for a pane split from one this size.
fn half(size: Resize, dir: Dir) -> Resize {
    match dir {
        Dir::Row => Resize {
            cols: (size.cols.saturating_sub(1) / 2).max(1),
            ..size
        },
        Dir::Column => Resize {
            rows: (size.rows.saturating_sub(1) / 2).max(1),
            ..size
        },
    }
}

fn err(code: ErrorCode, message: impl Into<String>) -> Reply {
    Reply::Error {
        code,
        message: message.into(),
    }
}

impl Daemon {
    /// `build` identifies this daemon's binary; see [`crate::BUILD`].
    pub fn new(build: impl Into<String>) -> Arc<Self> {
        let build = build.into();
        Arc::new_cyclic(|me: &Weak<Daemon>| {
            let weak = me.clone();
            Self {
                state: Mutex::new(State::default()),
                build,
                me: me.clone(),
                git: git::Watcher::new(move |_root, _info| {
                    if let Some(d) = weak.upgrade() {
                        let mut st = d.lock();
                        st.git_changed(&d.git);
                    }
                }),
                git_ops: Mutex::new(()),
            }
        })
    }

    /// What new shells are told about the daemon (see [`PaneEnv`]).
    pub fn set_pane_env(&self, env: PaneEnv) {
        self.lock().env = env;
    }

    /// A panic on one connection's thread mustn't lock everyone out.
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// See [`Daemon::git_ops`]; always taken before `state`.
    fn git_ops(&self) -> MutexGuard<'_, ()> {
        self.git_ops.lock().unwrap_or_else(|e| e.into_inner())
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
        // Requests (which get a reply, and can take a while: opening a
        // workspace starts its panes) run in order on their own thread, so
        // typing, resizes and acks on the reader thread never wait on them.
        let (requests, queue) = std::sync::mpsc::channel::<ClientMsg>();
        let worker = {
            let me = self.clone();
            let out = out.clone();
            thread::Builder::new()
                .name("daemon-requests".into())
                .spawn(move || {
                    for msg in queue {
                        me.handle(conn, &out, msg);
                    }
                })?
        };
        let me = self.clone();
        let mut reader = reader;
        thread::Builder::new()
            .name("daemon-reader".into())
            .spawn(move || {
                while let Ok(Some(body)) = proto::wire::read_raw(&mut reader) {
                    match serde_json::from_slice::<ClientMsg>(&body) {
                        Ok(msg) if msg.req().is_some() => {
                            let _ = requests.send(msg);
                        }
                        Ok(msg) => me.handle(conn, &out, msg),
                        // A request this daemon doesn't know (a newer app or
                        // CLI): answer it with an error and carry on, rather
                        // than dropping the client, which takes its panes.
                        Err(e) => {
                            let req = serde_json::from_slice::<serde_json::Value>(&body)
                                .ok()
                                .and_then(|v| v.get("req")?.as_u64());
                            eprintln!("kibannd: a request it can't read: {e}");
                            if let Some(req) = req.and_then(|r| ReqId::try_from(r).ok()) {
                                Self::reply(
                                    &out,
                                    req,
                                    err(
                                        ErrorCode::Unsupported,
                                        format!(
                                        "this terminal service (build {}) can't do that; restart it to use the newer version",
                                        me.build
                                    )),
                                );
                            }
                        }
                    }
                }
                // Its requests finish first, so none acts for it after it's
                // gone.
                drop(requests);
                let _ = worker.join();
                me.disconnect(conn);
            })?;
        Ok(())
    }

    fn reply(out: &Outbox, req: ReqId, reply: Reply) {
        out.push(encode(&ServerMsg::<Frame>::Reply { req, reply }));
    }

    fn session(&self, id: SessionId) -> Option<Arc<Session>> {
        self.lock().sessions.get(&id).map(|e| e.session.clone())
    }

    fn handle(&self, conn: ConnId, out: &Arc<Outbox>, msg: ClientMsg) {
        // Calls on one session, made outside the lock.
        let with = |id: SessionId, f: &dyn Fn(&Session)| {
            if let Some(s) = self.session(id) {
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
                    busy: Some(self.busy()),
                },
            ),
            ClientMsg::Watch { req } => {
                let mut st = self.lock();
                st.watchers.insert(conn);
                let layout = st.model.layout().clone();
                // Also as the first event, so a client can take every
                // layout from one ordered stream.
                out.push_latest(
                    Snapshot::Layout,
                    encode(&ServerMsg::Layout {
                        layout: layout.clone(),
                    }),
                );
                st.status_changed();
                out.push_latest(
                    Snapshot::Status,
                    encode(&ServerMsg::Status {
                        panes: st.status.map(),
                    }),
                );
                out.push_latest(
                    Snapshot::Git,
                    encode(&ServerMsg::Git {
                        panes: st.git_map(&self.git),
                    }),
                );
                Self::reply(out, req, Reply::Layout { layout });
            }
            ClientMsg::GetLayout { req } => {
                let layout = self.lock().model.layout().clone();
                Self::reply(out, req, Reply::Layout { layout });
            }
            ClientMsg::Act { req, action } => {
                let reply = match self.act(conn, action) {
                    Ok((pane, tab)) => Reply::Acted { pane, tab },
                    Err(e) => e,
                };
                Self::reply(out, req, reply);
            }
            ClientMsg::Panes { req } => Self::reply(out, req, self.panes()),
            ClientMsg::Read {
                req,
                pane,
                source,
                lines,
            } => {
                let reply = match self.session(pane) {
                    Some(s) => match s.read(source, lines.map(|n| n as usize)) {
                        Some(text) => Reply::Text { text },
                        None => err(ErrorCode::Failed, format!("pane p{pane} is busy")),
                    },
                    None => err(ErrorCode::NotFound, format!("no pane p{pane}")),
                };
                Self::reply(out, req, reply);
            }
            ClientMsg::Status { req, pane, mark } => {
                let reply = match self.session(pane) {
                    Some(s) => Reply::Status {
                        ended: false,
                        code: None,
                        mark: mark.and_then(|m| s.mark(&m)),
                        agent: self.lock().status.get(pane),
                    },
                    None => match self.lock().ended.iter().rev().find(|(p, _)| *p == pane) {
                        Some(&(_, code)) => Reply::Status {
                            ended: true,
                            code,
                            mark: None,
                            agent: None,
                        },
                        None => err(ErrorCode::NotFound, format!("no pane p{pane}")),
                    },
                };
                Self::reply(out, req, reply);
            }
            ClientMsg::Report {
                req,
                pane,
                agent,
                state,
                seq,
            } => {
                let mut st = self.lock();
                let reply = if !st.sessions.contains_key(&pane) {
                    err(ErrorCode::NotFound, format!("no pane p{pane}"))
                } else if state == AgentState::Done {
                    err(
                        ErrorCode::Invalid,
                        "report idle; done is worked out from it",
                    )
                } else if agent.is_empty() {
                    err(ErrorCode::Invalid, "no agent name")
                } else {
                    let seen = st.is_seen(pane);
                    if st.status.report(pane, agent, state, seq, seen) {
                        st.status_changed();
                        Reply::Ok
                    } else {
                        err(ErrorCode::Stale, "stale seq")
                    }
                };
                drop(st);
                Self::reply(out, req, reply);
            }
            ClientMsg::Diff { req, pane } => {
                let root = {
                    let st = self.lock();
                    if st.sessions.contains_key(&pane) {
                        Ok(st.places.get(&pane).and_then(|p| p.root.clone()))
                    } else {
                        Err(err(ErrorCode::NotFound, format!("no pane p{pane}")))
                    }
                };
                // Git can take a while; this thread also carries the
                // connection's keystrokes.
                let out = out.clone();
                let _ = thread::Builder::new()
                    .name("daemon-diff".into())
                    .spawn(move || {
                        let reply = match root {
                            Err(e) => e,
                            Ok(None) => err(ErrorCode::NoRepo, "not in a git checkout"),
                            Ok(Some(root)) => match git::Checkout::discover(&root)
                                .ok_or_else(|| err(ErrorCode::NoRepo, "not in a git checkout"))
                                .and_then(|c| {
                                    c.diff(git::Limits::default()).map_err(|e| {
                                        err(ErrorCode::Failed, format!("git diff: {e}"))
                                    })
                                }) {
                                Ok(diff) => Reply::Diff { diff },
                                Err(e) => e,
                            },
                        };
                        Self::reply(&out, req, reply);
                    });
            }
            ClientMsg::SetTheme { req, theme } => {
                let mut st = self.lock();
                for e in st.sessions.values() {
                    e.session.set_theme(theme.clone());
                }
                st.theme = Some(theme);
                st.offer_save();
                drop(st);
                Self::reply(out, req, Reply::Ok);
            }
            ClientMsg::Release { req, pane } => {
                let mut st = self.lock();
                let reply = if st.sessions.contains_key(&pane) {
                    let seen = st.is_seen(pane);
                    st.status.release(pane, seen);
                    st.status_changed();
                    Reply::Ok
                } else {
                    err(ErrorCode::NotFound, format!("no pane p{pane}"))
                };
                drop(st);
                Self::reply(out, req, reply);
            }
            ClientMsg::Looking { pane } => {
                let mut st = self.lock();
                match pane {
                    Some(p) => st.looking.insert(conn, p),
                    None => st.looking.remove(&conn),
                };
                st.status_changed();
            }
            ClientMsg::Attach { session } => with(session, &|s| {
                let out = out.clone();
                s.view(conn, move |frame| {
                    out.push_frame(encode(&ServerMsg::Frame { session, frame }));
                });
            }),
            ClientMsg::Detach { session } => with(session, &|s| s.unview(conn)),
            ClientMsg::Input { session, event } => with(session, &|s| s.input(event.clone())),
            ClientMsg::Resize { session, size } => {
                let s = {
                    let mut st = self.lock();
                    st.sessions.get_mut(&session).map(|e| {
                        e.size = size;
                        e.session.clone()
                    })
                };
                if let Some(s) = s {
                    s.resize(size);
                }
            }
            ClientMsg::Ack { session, seq } => with(session, &|s| s.ack(conn, seq)),
            ClientMsg::RequestFull { session } => with(session, &|s| s.request_full(conn)),
            ClientMsg::Shutdown => self.shutdown(),
            ClientMsg::Restore { req } => Self::reply(out, req, self.restore()),
        }
    }

    /// Apply a layout action. Returns the pane and tab it made or acted on.
    fn act(&self, conn: ConnId, action: Action) -> Result<(Option<PaneId>, Option<TabId>), Reply> {
        let no_pane = |p: PaneId| err(ErrorCode::NotFound, format!("no pane p{p}"));
        let no_tab = |t: TabId| err(ErrorCode::NotFound, format!("no tab t{t}"));
        match action {
            Action::NewTab {
                near,
                cwd,
                argv,
                focus,
            } => {
                let size = near
                    .and_then(|p| self.lock().sessions.get(&p).map(|e| e.size))
                    .unwrap_or(DEFAULT_SIZE);
                let cwd = cwd
                    .map(PathBuf::from)
                    .or_else(|| self.session(near?)?.cwd());
                let pane = self.spawn(size, cwd, argv).map_err(spawn_error)?;
                let mut st = self.lock();
                let tab = st.model.new_tab(pane, focus);
                st.drop_ended([pane]);
                st.broadcast(None);
                Ok((Some(pane), Some(tab)))
            }
            Action::Split {
                pane: target,
                dir,
                cwd,
                argv,
                focus,
            } => {
                let size = {
                    let st = self.lock();
                    if st.model.layout().tab_of(target).is_none() {
                        return Err(no_pane(target));
                    }
                    st.sessions
                        .get(&target)
                        .map_or(DEFAULT_SIZE, |e| half(e.size, dir))
                };
                let cwd = cwd
                    .map(PathBuf::from)
                    .or_else(|| self.session(target)?.cwd());
                let pane = self.spawn(size, cwd, argv).map_err(spawn_error)?;
                let mut st = self.lock();
                match st.model.split(target, dir, pane, focus) {
                    Some(tab) => {
                        st.drop_ended([pane]);
                        st.broadcast(None);
                        Ok((Some(pane), Some(tab)))
                    }
                    // The target closed meanwhile.
                    None => {
                        let entry = st.sessions.remove(&pane);
                        drop(st);
                        drop(entry);
                        Err(no_pane(target))
                    }
                }
            }
            Action::Close { pane } => {
                let ops = self.git_ops();
                let (entry, root) = {
                    let mut st = self.lock();
                    if !st.model.remove(pane) {
                        return Err(no_pane(pane));
                    }
                    st.note_ended(pane, None);
                    st.status.remove(pane);
                    let root = st.forget_place(pane, &self.git);
                    st.broadcast(None);
                    (st.sessions.remove(&pane), root)
                };
                if let Some(root) = root {
                    self.git.remove(&root);
                }
                drop(ops);
                // Dropping ends the shell and joins its thread: not under the lock.
                drop(entry);
                Ok((Some(pane), None))
            }
            // Focus changes the app makes itself aren't echoed back to it:
            // a late echo could undo a newer click.
            Action::Focus { pane } => {
                let mut st = self.lock();
                if !st.model.focus(pane) {
                    return Err(no_pane(pane));
                }
                st.broadcast(Some(conn));
                let tab = st.model.layout().active;
                Ok((Some(pane), tab))
            }
            Action::Activate { tab } => {
                let mut st = self.lock();
                if !st.model.activate(tab) {
                    return Err(no_tab(tab));
                }
                st.broadcast(Some(conn));
                Ok((None, Some(tab)))
            }
            Action::SetRatio { tab, path, ratio } => {
                let mut st = self.lock();
                if !st.model.set_ratio(tab, &path, ratio) {
                    return Err(err(ErrorCode::NotFound, "no such split"));
                }
                st.broadcast(None);
                Ok((None, Some(tab)))
            }
            Action::Equalize { tab } => {
                let mut st = self.lock();
                if !st.model.equalize(tab) {
                    return Err(no_tab(tab));
                }
                st.broadcast(None);
                Ok((None, Some(tab)))
            }
            Action::OpenWorkspace { workspace } => self.open_workspace(workspace),
            Action::Zoom { tab, pane } => {
                let mut st = self.lock();
                if !st.model.zoom(tab, pane) {
                    return Err(err(ErrorCode::NotFound, "no such tab or pane"));
                }
                st.broadcast(None);
                Ok((pane, Some(tab)))
            }
        }
    }

    /// Open a workspace file's tabs after the others: every pane started,
    /// then the tabs added in one go (one redraw, nothing half built). A
    /// pane that can't start is left out. Returns the shown tab and pane.
    fn open_workspace(
        &self,
        workspace: proto::layout::Workspace,
    ) -> Result<(Option<PaneId>, Option<TabId>), Reply> {
        use proto::layout::{Run, WorkspaceTree};
        if workspace.tabs.is_empty() {
            return Err(err(ErrorCode::Invalid, "the layout has no tabs"));
        }
        // Which programs are installed, for commands with alternatives (or
        // a note for when none is): asked once, before anything starts.
        let mut names = Vec::new();
        for t in &workspace.tabs {
            for leaf in t.tree.leaves() {
                if let WorkspaceTree::Leaf {
                    run: Some(Run::Type { commands, missing }),
                    ..
                } = leaf
                    && (commands.len() > 1 || missing.is_some())
                {
                    names.extend(
                        commands
                            .iter()
                            .filter_map(|c| crate::installed::program_of(c)),
                    );
                }
            }
        }
        let have = crate::installed::check(&names);

        let mut failed = Vec::new();
        // What to do in each pane once it's up: a command to type, a note.
        let mut todo: Vec<(PaneId, Option<String>, Option<String>)> = Vec::new();
        let mut start = |cwd: &str, run: &Option<Run>| -> PaneId {
            let dir = Some(PathBuf::from(cwd)).filter(|d| d.is_dir());
            let (argv, typed, note) = match run {
                None => (None, None, None),
                Some(Run::Exec { argv }) => (Some(argv.clone()), None, None),
                Some(Run::Type { commands, missing }) => {
                    if commands.len() == 1 && missing.is_none() {
                        (None, Some(commands[0].clone()), None)
                    } else {
                        let pick = commands.iter().find(|c| {
                            crate::installed::program_of(c).is_some_and(|p| have.contains(p))
                        });
                        let note = match (pick, missing) {
                            (Some(_), _) => None,
                            (None, m) if commands.is_empty() => m.clone(),
                            (None, m) => {
                                let names: Vec<&str> = commands
                                    .iter()
                                    .filter_map(|c| crate::installed::program_of(c))
                                    .collect();
                                let what = if names.len() == 1 {
                                    format!("{} isn't installed", names[0])
                                } else {
                                    format!("none of {} is installed", names.join(", "))
                                };
                                Some(match m {
                                    Some(m) => format!("{what}: {m}"),
                                    None => what,
                                })
                            }
                        };
                        (None, pick.cloned(), note)
                    }
                }
            };
            match self.spawn(DEFAULT_SIZE, dir, argv) {
                Ok(id) => {
                    if typed.is_some() || note.is_some() {
                        todo.push((id, typed, note));
                    }
                    id
                }
                Err(e) => {
                    eprintln!("kibannd: opening a layout's pane: {e}");
                    let id = PaneId::MAX - failed.len() as PaneId;
                    failed.push(id);
                    id
                }
            }
        };
        fn build(
            t: &WorkspaceTree,
            start: &mut dyn FnMut(&str, &Option<Run>) -> PaneId,
        ) -> proto::layout::Tree {
            match t {
                WorkspaceTree::Leaf { cwd, run } => proto::layout::Tree::leaf(start(cwd, run)),
                WorkspaceTree::Split { dir, ratio, a, b } => proto::layout::Tree::Split {
                    dir: *dir,
                    ratio: ratio.clamp(0.05, 0.95),
                    a: Box::new(build(a, start)),
                    b: Box::new(build(b, start)),
                },
            }
        }
        let trees: Vec<_> = workspace
            .tabs
            .iter()
            .map(|t| build(&t.tree, &mut start))
            .collect();
        let placed: Vec<PaneId> = trees.iter().flat_map(|t| t.panes()).collect();
        let mut st = self.lock();
        for (i, (tree, t)) in trees.into_iter().zip(&workspace.tabs).enumerate() {
            let order = tree.panes();
            let focused = order.get(t.focused as usize).copied().unwrap_or(order[0]);
            st.model.add_tab(
                tree,
                focused,
                t.title.clone(),
                i == workspace.active as usize,
            );
        }
        st.drop_ended(placed);
        st.broadcast(None);
        let shown = st.model.layout().active;
        let pane = shown
            .and_then(|t| st.model.layout().tab(t))
            .map(|t| t.focused);
        let sessions: Vec<_> = todo
            .into_iter()
            .filter_map(|(id, typed, note)| {
                st.sessions
                    .get(&id)
                    .map(|e| (e.session.clone(), typed, note))
            })
            .collect();
        drop(st);
        let opened = workspace
            .tabs
            .iter()
            .map(|t| t.tree.leaves().len())
            .sum::<usize>();
        if failed.len() == opened {
            return Err(err(
                ErrorCode::Failed,
                "none of the workspace's panes could start",
            ));
        }
        for (session, typed, note) in sessions {
            if let Some(n) = note {
                // Dim, above the prompt.
                session.show(&format!("\x1b[2mkibann: {n}\x1b[0m\r\n"));
            }
            if let Some(cmd) = typed {
                let _ = thread::Builder::new()
                    .name("layout-type".into())
                    .spawn(move || type_when_ready(&session, &cmd));
            }
        }
        Ok((pane, shown))
    }

    /// Start a shell for a new pane (not yet in the layout).
    fn spawn(
        &self,
        size: Resize,
        cwd: Option<PathBuf>,
        argv: Option<Vec<String>>,
    ) -> io::Result<SessionId> {
        let (id, env) = {
            let mut st = self.lock();
            st.next_session += 1;
            (st.next_session, st.env.clone())
        };
        let argv = argv.or(env.command);
        let mut vars = vec![("KIBANN_PANE".to_owned(), format!("p{id}"))];
        if let Some(s) = &env.socket {
            vars.push(("KIBANN_SOCKET".into(), s.to_string_lossy().into_owned()));
        }
        if let Some(b) = &env.bin {
            vars.push(("KIBANN_BIN".into(), b.to_string_lossy().into_owned()));
        }
        if let Some(dir) = &env.path_dir {
            let mut path = std::ffi::OsString::from(dir);
            if let Some(old) = std::env::var_os("PATH") {
                path.push(if cfg!(windows) { ";" } else { ":" });
                path.push(old);
            }
            vars.push(("PATH".into(), path.to_string_lossy().into_owned()));
        }
        let session = Session::spawn(
            Spawn {
                argv,
                cwd,
                env: vars,
            },
            size,
        )?;
        let exited = Arc::new(AtomicBool::new(false));
        session.on_event(events(id, exited.clone(), self.me.clone()));
        let mut st = self.lock();
        // Under the lock, so a theme set meanwhile isn't missed.
        if let Some(t) = &st.theme {
            session.set_theme(t.clone());
        }
        st.sessions.insert(
            id,
            Entry {
                session: Arc::new(session),
                exited,
                size,
            },
        );
        Ok(id)
    }

    /// A shell exited: its pane goes.
    fn pane_ended(&self, id: SessionId, code: Option<u32>) {
        let ops = self.git_ops();
        let (entry, root) = {
            let mut st = self.lock();
            let Some(entry) = st.sessions.remove(&id) else {
                return; // closed already
            };
            st.note_ended(id, code);
            st.status.remove(id);
            let root = st.forget_place(id, &self.git);
            if st.model.remove(id) {
                st.broadcast(None);
            } else {
                st.status_changed();
            }
            (entry, root)
        };
        if let Some(root) = root {
            self.git.remove(&root);
        }
        drop(ops);
        drop(entry);
    }

    /// A session saw its foreground program change (see [`Foreground`]).
    fn foreground(&self, id: SessionId, fg: Foreground) {
        let mut st = self.lock();
        if !st.sessions.contains_key(&id) {
            return;
        }
        let agent = fg.agent.map(|(a, s)| {
            let state = match s {
                term::agent::State::Idle => AgentState::Idle,
                term::agent::State::Working => AgentState::Working,
                term::agent::State::Blocked => AgentState::Blocked,
            };
            (a.name().to_owned(), state)
        });
        let seen = st.is_seen(id);
        let was_working = st
            .status
            .get(id)
            .is_some_and(|s| s.state == AgentState::Working);
        st.status.detected(id, agent, fg.at_shell, seen);
        st.status_changed();
        let finished = was_working
            && st
                .status
                .get(id)
                .is_none_or(|s| matches!(s.state, AgentState::Idle | AgentState::Done));

        // Where it is. A new directory is looked up off this (the
        // session's) thread: it runs git.
        let place = st.places.entry(id).or_default();
        // Back at the prompt, or a line entered there (a command too quick
        // to be seen running, like `git init`).
        let back_at_shell = fg.at_shell && (!place.at_shell || fg.entered != place.entered);
        place.at_shell = fg.at_shell;
        place.entered = fg.entered;
        let moved = fg.cwd.as_ref().is_some_and(|dir| *dir != place.dir);
        if let Some(dir) = fg.cwd
            && moved
        {
            place.dir = dir;
        }
        // Events can be missed; a prompt coming back or an agent finishing
        // is a good moment to look again. Look the checkout up again too:
        // `git init`, a clone into `.` or `rm -rf .git` change it without
        // a new directory.
        if (back_at_shell || finished)
            && let Some(root) = &place.root
        {
            self.git.refresh(root);
        }
        if moved || ((back_at_shell || finished) && !place.dir.as_os_str().is_empty()) {
            let dir = place.dir.clone();
            let me = self.me.clone();
            let _ = thread::Builder::new()
                .name("daemon-git-discover".into())
                .spawn(move || {
                    let checkout = git::Checkout::discover(&dir);
                    if let Some(d) = me.upgrade() {
                        d.pane_checkout(id, &dir, checkout);
                    }
                });
        }
        st.git_changed(&self.git);
    }

    /// `dir`, where pane `id` is, belongs to `checkout`.
    fn pane_checkout(&self, id: PaneId, dir: &std::path::Path, checkout: Option<git::Checkout>) {
        let _ops = self.git_ops();
        let old = {
            let mut st = self.lock();
            let Some(place) = st.places.get_mut(&id) else {
                return; // closed meanwhile
            };
            if place.dir != dir {
                return; // moved on meanwhile
            }
            let root = checkout.as_ref().map(|c| c.root.clone());
            if place.root == root {
                return;
            }
            std::mem::replace(&mut place.root, root)
        };
        if let Some(old) = old {
            self.git.remove(&old);
        }
        if let Some(c) = &checkout {
            self.git.add(c);
        }
        self.lock().git_changed(&self.git);
    }

    fn panes(&self) -> Reply {
        type Row = (
            PaneId,
            TabId,
            Option<Arc<Session>>,
            Option<PaneStatus>,
            Option<proto::git::GitInfo>,
        );
        let list: Vec<Row> = {
            let st = self.lock();
            st.model
                .layout()
                .tabs
                .iter()
                .flat_map(|t| t.tree.panes().into_iter().map(move |p| (p, t.id)))
                .map(|(p, t)| {
                    let s = st.sessions.get(&p).map(|e| e.session.clone());
                    let git = st
                        .places
                        .get(&p)
                        .and_then(|pl| pl.root.as_deref())
                        .and_then(|r| self.git.info(r));
                    (p, t, s, st.status.get(p), git)
                })
                .collect()
        };
        let panes = list
            .into_iter()
            .map(|(id, tab, s, status, git)| PaneInfo {
                git,
                id,
                tab,
                agent: status.as_ref().map(|a| a.agent.clone()),
                state: status.map(|a| a.state),
                title: s.as_ref().map(|s| s.title()).unwrap_or_default(),
                program: s.as_ref().and_then(|s| s.program()),
                shell: s.as_ref().and_then(|s| s.shell().map(str::to_owned)),
                cwd: s
                    .and_then(|s| s.cwd())
                    .map(|d| d.to_string_lossy().into_owned()),
            })
            .collect();
        Reply::Panes { panes }
    }

    /// The client went away: its sessions keep running, unattached.
    fn disconnect(&self, conn: ConnId) {
        let mut st = self.lock();
        if let Some(out) = st.conns.remove(&conn) {
            out.close();
        }
        st.watchers.remove(&conn);
        st.looking.remove(&conn);
        st.status_changed();
        for e in st.sessions.values() {
            e.session.unview(conn);
        }
    }

    /// Keep the layout in `path` from now on, for [`Self::restore`],
    /// written once it stayed the same for `delay`
    /// ([`crate::saved::SAVE_DELAY`]).
    /// The theme saved there is used until a client sets one.
    pub fn save_layout_to(&self, path: PathBuf, delay: Duration) {
        let theme = crate::saved::load_theme(&path);
        let mut st = self.lock();
        if st.theme.is_none() {
            st.theme = theme;
        }
        st.saver = Some(crate::saved::Saver::start(path, delay));
    }

    /// With no panes open, reopen the saved layout: a fresh shell per pane
    /// in its directory (home when it's gone), the same splits, sizes and
    /// focus. Nothing when panes are open or nothing was saved.
    fn restore(&self) -> Reply {
        let nothing = Reply::Restored { tabs: 0, panes: 0 };
        let saved = {
            let st = self.lock();
            if !st.model.layout().tabs.is_empty() {
                return nothing;
            }
            st.saver.as_ref().and_then(|s| crate::saved::load(s.path()))
        };
        let Some(saved) = saved else {
            return nothing;
        };
        let tabs = saved.workspace.tabs.len() as u32;
        if self.open_workspace(saved.workspace).is_err() {
            return nothing;
        }
        let st = self.lock();
        let panes = st
            .model
            .layout()
            .tabs
            .iter()
            .map(|t| t.tree.panes().len() as u32)
            .sum();
        Reply::Restored { tabs, panes }
    }

    /// What runs in the panes besides their shells: a program name for each
    /// pane not at its prompt.
    fn busy(&self) -> Vec<String> {
        let sessions: Vec<Arc<Session>> = {
            let st = self.lock();
            st.sessions
                .iter()
                .filter(|(id, _)| st.places.get(id).is_some_and(|p| !p.at_shell))
                .map(|(_, e)| e.session.clone())
                .collect()
        };
        sessions
            .iter()
            .map(|s| {
                s.program()
                    .and_then(|argv| argv.into_iter().next())
                    .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(&p).to_owned())
                    .unwrap_or_else(|| "a program".to_owned())
            })
            .collect()
    }

    /// Save the layout, then end every session; [`serve_socket`] then exits.
    pub fn shutdown(&self) {
        // Outside the lock: the saver writes on its own thread.
        let saver = self.lock().saver.take();
        if let Some(s) = &saver {
            s.flush(Duration::from_secs(2));
        }
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

    /// Git checkouts watched for panes now.
    pub fn checkouts_watched(&self) -> usize {
        self.git.watching()
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

/// Type `cmd` and Enter into a pane just opened, once its shell is
/// reading: text typed before the first output (the prompt) shows twice.
fn type_when_ready(session: &Session, cmd: &str) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while session
        .read(proto::wire::ReadSource::Visible, None)
        .is_none_or(|t| t.trim().is_empty())
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(50));
    }
    session.input(proto::InputEvent::Text { data: cmd.into() });
    session.input(proto::InputEvent::Text { data: "\r".into() });
}

fn spawn_error(e: io::Error) -> Reply {
    err(ErrorCode::Failed, format!("could not start the shell: {e}"))
}

pub struct SocketConfig {
    pub socket: PathBuf,
    /// Exit after this long with no clients and no running sessions.
    pub idle_exit: Duration,
}

/// How often the accept loop looks at whether it should stop.
const ACCEPT_WAIT: Duration = Duration::from_millis(50);

/// Listen on the socket and serve until shut down or idle. Fails with
/// `AddrInUse` if another daemon already serves this socket.
pub fn serve_socket(daemon: &Arc<Daemon>, cfg: &SocketConfig) -> io::Result<()> {
    let listener = endpoint::Listener::bind(&cfg.socket)?;
    serve_listener(daemon, listener, cfg.idle_exit)
}

/// Serve the clients `listener` takes until shut down, or idle for
/// `idle_exit`. Then new clients find nobody there, and the lock stays
/// held until every session has ended.
pub fn serve_listener(
    daemon: &Arc<Daemon>,
    listener: endpoint::Listener,
    idle_exit: Duration,
) -> io::Result<()> {
    let mut idle_since: Option<Instant> = None;
    let result = loop {
        if daemon.is_shut_down() {
            break Ok(());
        }
        match listener.accept(ACCEPT_WAIT) {
            Ok(Some(c)) => daemon.serve(c.reader, c.writer)?,
            Ok(None) => {}
            Err(e) => break Err(e),
        }
        if daemon.clients() == 0 && daemon.live_sessions() == 0 {
            let since = *idle_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= idle_exit {
                break Ok(());
            }
        } else {
            idle_since = None;
        }
    };
    let lock = listener.close();
    daemon.shutdown();
    drop(lock);
    result
}
