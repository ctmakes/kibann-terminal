//! The daemon: owns the sessions and the layout, and serves clients over
//! byte streams.
//!
//! Each connection gets a reader thread (decodes requests and acts on them)
//! and a writer thread (drains the connection's [`Outbox`]). Sessions send
//! frames from their own threads into their viewer's outbox, so a slow client
//! never stalls a session or other clients.
//!
//! A session always has a sink. With no viewer it only reports the exit;
//! frames stop after two unacked ones (the session's own pacing), so a
//! detached session costs nothing but its emulator keeps up with the output.
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

use proto::git::PaneDir;
use proto::layout::{Action, Dir, PaneId, TabId};
use proto::status::{AgentState, PaneStatus};
use proto::wire::{
    ClientMsg, PROTOCOL_VERSION, PaneInfo, Reply, ReqId, ServerMsg, SessionId, read_msg, write_msg,
};
use proto::{Frame, Resize};
use term::{Event, Foreground, Session, Spawn};

use crate::layout::Model;
use crate::outbox::Outbox;
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
}

/// `dir` with the home directory shown as `~`.
fn shown(dir: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = dir.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        };
    }
    dir.display().to_string()
}

/// How many ended panes to remember, for `kibann pane wait --exit`.
const MAX_ENDED: usize = 256;

struct Entry {
    session: Arc<Session>,
    /// The connection the frames go to.
    viewer: Option<ConnId>,
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
}

#[derive(Default)]
struct State {
    sessions: BTreeMap<SessionId, Entry>,
    next_session: SessionId,
    conns: HashMap<ConnId, Arc<Outbox>>,
    next_conn: ConnId,
    /// Connections that get every layout change.
    watchers: HashSet<ConnId>,
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
    /// The app's theme; `None` until it sends one (libghostty's defaults,
    /// the same colors as the built-in dark theme).
    theme: Option<proto::theme::Theme>,
    shutdown: bool,
}

impl State {
    /// Send the layout to every watcher but `skip`. Done under the lock, so
    /// changes reach each connection in order.
    fn broadcast(&mut self, skip: Option<ConnId>) {
        let msg = encode(&ServerMsg::Layout {
            layout: self.model.layout().clone(),
        });
        self.send_watchers(&msg, skip);
        self.status_changed();
    }

    fn send_watchers(&self, msg: &[u8], skip: Option<ConnId>) {
        for c in &self.watchers {
            if Some(*c) != skip
                && let Some(out) = self.conns.get(c)
            {
                out.push(msg.to_vec());
            }
        }
    }

    /// The pane the user is looking at: the focused pane of the shown tab,
    /// while an app watches.
    fn seen_pane(&self) -> Option<PaneId> {
        if self.watchers.is_empty() {
            return None;
        }
        let layout = self.model.layout();
        let tab = layout.tabs.iter().find(|t| Some(t.id) == layout.active)?;
        Some(tab.zoomed.unwrap_or(tab.focused))
    }

    fn is_seen(&self, pane: PaneId) -> bool {
        self.seen_pane() == Some(pane)
    }

    /// After anything that may change agent status (or what is seen): clear
    /// done on the seen pane, and send the map if it changed.
    fn status_changed(&mut self) {
        if let Some(p) = self.seen_pane() {
            self.status.seen(p);
        }
        if let Some(panes) = self.status.changed() {
            self.send_watchers(&encode(&ServerMsg::Status { panes }), None);
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
            self.send_watchers(&encode(&ServerMsg::Git { panes: now.clone() }), None);
            self.git_sent = now;
        }
    }

    /// The pane is gone: stop watching its checkout for it.
    fn forget_place(&mut self, pane: PaneId, git: &git::Watcher) {
        if let Some(root) = self.places.remove(&pane).and_then(|p| p.root) {
            git.remove(&root);
        }
        self.git_changed(git);
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
}

/// One message, framed, ready for an outbox.
fn encode(msg: &ServerMsg) -> Vec<u8> {
    let mut buf = Vec::new();
    // Writing to a Vec only fails for a message over the size cap.
    let _ = write_msg(&mut buf, msg);
    buf
}

/// Where a session's events go: frames to its viewer's outbox, if any; the
/// exit to the daemon, which closes the pane.
fn sink(
    id: SessionId,
    out: Option<Arc<Outbox>>,
    exited: Arc<AtomicBool>,
    daemon: Weak<Daemon>,
) -> impl FnMut(Event) + Send + 'static {
    move |event| match event {
        Event::Frame(frame) => {
            if let Some(o) = &out {
                o.push_frame(encode(&ServerMsg::Frame { session: id, frame }));
            }
        }
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

fn err(message: impl Into<String>) -> Reply {
    Reply::Error {
        message: message.into(),
    }
}

impl Daemon {
    /// `build` identifies this daemon's binary; see [`crate::build_id`].
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
                },
            ),
            ClientMsg::Watch { req } => {
                let mut st = self.lock();
                st.watchers.insert(conn);
                let layout = st.model.layout().clone();
                // Also as the first event, so a client can take every
                // layout from one ordered stream.
                out.push(encode(&ServerMsg::Layout {
                    layout: layout.clone(),
                }));
                st.status_changed();
                out.push(encode(&ServerMsg::Status {
                    panes: st.status.map(),
                }));
                out.push(encode(&ServerMsg::Git {
                    panes: st.git_map(&self.git),
                }));
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
                    Some(s) => Reply::Text {
                        text: s.read(source, lines.map(|n| n as usize)),
                    },
                    None => err(format!("no pane p{pane}")),
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
                        None => err(format!("no pane p{pane}")),
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
                    err(format!("no pane p{pane}"))
                } else if state == AgentState::Done {
                    err("report idle; done is worked out from it")
                } else if agent.is_empty() {
                    err("no agent name")
                } else {
                    let seen = st.is_seen(pane);
                    if st.status.report(pane, agent, state, seq, seen) {
                        st.status_changed();
                        Reply::Ok
                    } else {
                        err("stale seq")
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
                        Err(err(format!("no pane p{pane}")))
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
                            Ok(None) => err("not in a git checkout"),
                            Ok(Some(root)) => match git::Checkout::discover(&root)
                                .ok_or_else(|| err("not in a git checkout"))
                                .and_then(|c| {
                                    c.diff(git::Limits::default())
                                        .map_err(|e| err(format!("git diff: {e}")))
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
                    err(format!("no pane p{pane}"))
                };
                drop(st);
                Self::reply(out, req, reply);
            }
            ClientMsg::Attach { session } => {
                let mut st = self.lock();
                if let Some(e) = st.sessions.get_mut(&session) {
                    e.viewer = Some(conn);
                    e.session.attach(sink(
                        session,
                        Some(out.clone()),
                        e.exited.clone(),
                        self.me.clone(),
                    ));
                }
            }
            ClientMsg::Detach { session } => {
                let mut st = self.lock();
                if let Some(e) = st.sessions.get_mut(&session)
                    && e.viewer == Some(conn)
                {
                    e.viewer = None;
                    e.session
                        .attach(sink(session, None, e.exited.clone(), self.me.clone()));
                }
            }
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
            ClientMsg::Ack { session, seq } => with(session, &|s| s.ack(seq)),
            ClientMsg::RequestFull { session } => with(session, &|s| s.request_full()),
            ClientMsg::Shutdown => self.shutdown(),
        }
    }

    /// Apply a layout action. Returns the pane and tab it made or acted on.
    fn act(&self, conn: ConnId, action: Action) -> Result<(Option<PaneId>, Option<TabId>), Reply> {
        let no_pane = |p: PaneId| err(format!("no pane p{p}"));
        let no_tab = |t: TabId| err(format!("no tab t{t}"));
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
                let entry = {
                    let mut st = self.lock();
                    if !st.model.remove(pane) {
                        return Err(no_pane(pane));
                    }
                    st.note_ended(pane, None);
                    st.status.remove(pane);
                    st.forget_place(pane, &self.git);
                    st.broadcast(None);
                    st.sessions.remove(&pane)
                };
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
                    return Err(err("no such split"));
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
            Action::Zoom { tab, pane } => {
                let mut st = self.lock();
                if !st.model.zoom(tab, pane) {
                    return Err(err("no such tab or pane"));
                }
                st.broadcast(None);
                Ok((pane, Some(tab)))
            }
        }
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
        session.attach(sink(id, None, exited.clone(), self.me.clone()));
        let mut st = self.lock();
        // Under the lock, so a theme set meanwhile isn't missed.
        if let Some(t) = &st.theme {
            session.set_theme(t.clone());
        }
        st.sessions.insert(
            id,
            Entry {
                session: Arc::new(session),
                viewer: None,
                exited,
                size,
            },
        );
        Ok(id)
    }

    /// A shell exited: its pane goes.
    fn pane_ended(&self, id: SessionId, code: Option<u32>) {
        let entry = {
            let mut st = self.lock();
            let Some(entry) = st.sessions.remove(&id) else {
                return; // closed already
            };
            st.note_ended(id, code);
            st.status.remove(id);
            st.forget_place(id, &self.git);
            if st.model.remove(id) {
                st.broadcast(None);
            } else {
                st.status_changed();
            }
            entry
        };
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
        let back_at_shell = fg.at_shell && !place.at_shell;
        place.at_shell = fg.at_shell;
        if let Some(dir) = fg.cwd
            && dir != place.dir
        {
            place.dir = dir.clone();
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
        // Events can be missed; a prompt coming back or an agent finishing
        // is a good moment to look again.
        if (back_at_shell || finished)
            && let Some(root) = &place.root
        {
            self.git.refresh(root);
        }
        st.git_changed(&self.git);
    }

    /// `dir`, where pane `id` is, belongs to `checkout`.
    fn pane_checkout(&self, id: PaneId, dir: &std::path::Path, checkout: Option<git::Checkout>) {
        let mut st = self.lock();
        let Some(place) = st.places.get_mut(&id) else {
            return; // closed meanwhile
        };
        if place.dir != dir {
            return; // moved on meanwhile
        }
        let root = checkout.as_ref().map(|c| c.root.clone());
        if place.root != root {
            if let Some(old) = place.root.take() {
                self.git.remove(&old);
            }
            if let Some(c) = &checkout {
                self.git.add(c);
            }
            place.root = root;
        }
        st.git_changed(&self.git);
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
        // Each asks the session thread: outside the lock.
        let panes = list
            .into_iter()
            .map(|(id, tab, s, status, git)| PaneInfo {
                git,
                id,
                tab,
                agent: status.as_ref().map(|a| a.agent.clone()),
                state: status.map(|a| a.state),
                title: s.as_ref().map(|s| s.title()).unwrap_or_default(),
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
        let me = self.me.clone();
        for (id, e) in st.sessions.iter_mut() {
            if e.viewer == Some(conn) {
                e.viewer = None;
                e.session
                    .attach(sink(*id, None, e.exited.clone(), me.clone()));
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

fn spawn_error(e: io::Error) -> Reply {
    err(format!("could not start the shell: {e}"))
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
