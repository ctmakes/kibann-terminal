//! The daemon with real shells, over a real socket.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use daemon::client::{Client, ClientEvent};
use daemon::server::{Daemon, SocketConfig, serve_socket};
use proto::git::PaneDir;
use proto::layout::{Action, Dir, Layout};
use proto::status::{AgentState, PaneStatus};
use proto::wire::{ClientMsg, PROTOCOL_VERSION, ReadSource, Reply, SessionId};
use proto::{Frame, InputEvent, Resize};

const SIZE: Resize = Resize {
    cols: 60,
    rows: 10,
    cell_w: 8,
    cell_h: 16,
};

/// A short socket path (macOS caps them at 104 bytes) in a fresh directory.
fn socket() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("kd{}-{n}", std::process::id()))
        .join("s.sock")
}

fn start(socket: &Path, idle: Duration) -> (Arc<Daemon>, JoinHandle<std::io::Result<()>>) {
    let d = Daemon::new("test-build");
    let cfg = SocketConfig {
        socket: socket.to_path_buf(),
        idle_exit: idle,
    };
    let dd = d.clone();
    let h = thread::spawn(move || serve_socket(&dd, &cfg));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon didn't start");
        thread::sleep(Duration::from_millis(10));
    }
    (d, h)
}

#[derive(Debug)]
enum Ev {
    Frame(SessionId, Box<Frame>),
    Layout(Layout),
    Status(BTreeMap<SessionId, PaneStatus>),
    Git(BTreeMap<SessionId, PaneDir>),
    Disconnected,
}

fn events() -> (impl FnMut(ClientEvent<'_>) + Send + 'static, Receiver<Ev>) {
    let (tx, rx) = channel();
    let f = move |e: ClientEvent<'_>| {
        let _ = tx.send(match e {
            ClientEvent::Frame { session, json } => Ev::Frame(
                session,
                Box::new(serde_json::from_str(json).expect("frame json")),
            ),
            ClientEvent::Layout(l) => Ev::Layout(l),
            ClientEvent::Status(s) => Ev::Status(s),
            ClientEvent::Git(g) => Ev::Git(g),
            ClientEvent::Disconnected => Ev::Disconnected,
        });
    };
    (f, rx)
}

fn sh(c: &Client, script: Option<&str>) -> SessionId {
    let argv = match script {
        Some(s) => vec!["/bin/sh".into(), "-c".into(), s.into()],
        None => vec!["/bin/sh".into()],
    };
    let (pane, _) = c
        .act(Action::NewTab {
            near: None,
            cwd: None,
            argv: Some(argv),
            focus: true,
        })
        .expect("new tab");
    let pane = pane.unwrap();
    c.send(&ClientMsg::Resize {
        session: pane,
        size: SIZE,
    })
    .unwrap();
    pane
}

/// The next layout event, skipping frames.
fn next_layout(rx: &Receiver<Ev>) -> Layout {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Ev::Layout(l)) => return l,
            Ok(_) => {}
            Err(_) => panic!("no layout event"),
        }
    }
}

fn read(c: &Client, pane: SessionId, source: ReadSource) -> String {
    match c
        .request(|req| ClientMsg::Read {
            req,
            pane,
            source,
            lines: None,
        })
        .unwrap()
    {
        Reply::Text { text } => text,
        other => panic!("{other:?}"),
    }
}

fn status(c: &Client, pane: SessionId, mark: Option<&str>) -> Reply {
    c.request(|req| ClientMsg::Status {
        req,
        pane,
        mark: mark.map(String::from),
    })
    .unwrap()
}

/// Poll until `f` holds.
fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f() {
        assert!(Instant::now() < deadline, "never: {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn type_text(c: &Client, session: SessionId, text: &str) {
    c.send(&ClientMsg::Input {
        session,
        event: InputEvent::Text { data: text.into() },
    })
    .unwrap();
}

/// The session's screen as the frames describe it, one string per row.
#[derive(Default)]
struct Screen(BTreeMap<u16, String>);

impl Screen {
    fn apply(&mut self, f: &Frame) {
        if f.full {
            self.0.clear();
        }
        for row in &f.rows {
            let mut s = String::new();
            for run in &row.runs {
                match &run.text {
                    Some(t) => t
                        .iter()
                        .for_each(|c| s.push_str(if c.is_empty() { " " } else { c })),
                    None => s.push_str(&" ".repeat(run.len as usize)),
                }
            }
            self.0.insert(row.y, s);
        }
    }

    fn contains(&self, needle: &str) -> bool {
        self.0.values().any(|r| r.contains(needle))
    }
}

/// Apply frames for `session` (acking them, as the app does) until the
/// screen shows `needle`.
fn wait_for(c: &Client, rx: &Receiver<Ev>, session: SessionId, needle: &str) -> Screen {
    let mut screen = Screen::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ev::Frame(s, f)) if s == session => {
                screen.apply(&f);
                c.send(&ClientMsg::Ack {
                    session,
                    seq: f.seq,
                })
                .unwrap();
                if screen.contains(needle) {
                    return screen;
                }
            }
            _ => {}
        }
    }
    panic!("never saw {needle:?}; screen: {:#?}", screen.0);
}

#[test]
fn spawn_type_and_see_the_output() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    assert_eq!(c.hello().unwrap(), (PROTOCOL_VERSION, "test-build".into()));
    let s = sh(&c, None);
    c.send(&ClientMsg::Attach { session: s }).unwrap();
    type_text(&c, s, "echo hi-$((40+2))\r");
    wait_for(&c, &rx, s, "hi-42");
    d.shutdown();
    h.join().unwrap().unwrap();
    assert!(!path.exists(), "socket removed on exit");
}

#[test]
fn sessions_keep_running_while_detached() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let s = {
        let (f, rx) = events();
        let c = Client::connect(&path, f).unwrap();
        let s = sh(&c, None);
        c.send(&ClientMsg::Attach { session: s }).unwrap();
        type_text(&c, s, "sleep 0.3; echo later-$((1+1))\r");
        wait_for(&c, &rx, s, "sleep 0.3");
        s
        // The client goes away here, before the output.
    };
    thread::sleep(Duration::from_millis(700));
    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    assert_eq!(c.layout().unwrap().panes(), vec![s]);
    c.send(&ClientMsg::Attach { session: s }).unwrap();
    wait_for(&c, &rx, s, "later-2");
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn a_second_client_takes_the_session_over() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f1, rx1) = events();
    let c1 = Client::connect(&path, f1).unwrap();
    let s = sh(&c1, None);
    c1.send(&ClientMsg::Attach { session: s }).unwrap();
    type_text(&c1, s, "echo one\r");
    wait_for(&c1, &rx1, s, "one");

    let (f2, rx2) = events();
    let c2 = Client::connect(&path, f2).unwrap();
    c2.send(&ClientMsg::Attach { session: s }).unwrap();
    wait_for(&c2, &rx2, s, "one"); // the full frame on attach
    while rx1.recv_timeout(Duration::from_millis(200)).is_ok() {}
    type_text(&c2, s, "echo two-$((1+1))\r");
    wait_for(&c2, &rx2, s, "two-2");
    assert!(
        !rx1.try_iter().any(|e| matches!(e, Ev::Frame(..))),
        "the first client no longer gets frames"
    );
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn a_shell_that_exits_takes_its_pane_with_it() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    let keep = sh(&c, None);
    assert_eq!(c.watch().unwrap().panes(), vec![keep]);
    assert_eq!(next_layout(&rx).panes(), vec![keep]); // the same, as an event
    let s = sh(&c, Some("sleep 0.2; exit 3"));
    assert_eq!(next_layout(&rx).panes(), vec![keep, s]);
    // Nobody watches its frames; the exit still closes the pane.
    assert_eq!(next_layout(&rx).panes(), vec![keep]);
    assert_eq!(d.live_sessions(), 1);
    assert_eq!(
        status(&c, s, None),
        Reply::Status {
            ended: true,
            code: Some(3),
            mark: None,
            agent: None,
        }
    );
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn layout_changes_reach_the_watcher_but_its_own_focus_changes_dont() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, app_rx) = events();
    let app = Client::connect(&path, f).unwrap();
    let first = sh(&app, None);
    let layout = app.watch().unwrap();
    assert_eq!(layout.tabs.len(), 1);
    assert_eq!(next_layout(&app_rx), layout);
    let tab = layout.tabs[0].id;

    // An agent splits next to the first pane without taking focus.
    let (f, _rx) = events();
    let agent = Client::connect(&path, f).unwrap();
    let (p, t) = agent
        .act(Action::Split {
            pane: first,
            dir: Dir::Row,
            cwd: None,
            argv: Some(vec!["/bin/sh".into()]),
            focus: false,
        })
        .unwrap();
    assert_eq!(t, Some(tab));
    let p = p.unwrap();
    let l = next_layout(&app_rx);
    assert_eq!(l.panes(), vec![first, p]);
    assert_eq!(l.tabs[0].focused, first);

    // The app's own click on the new pane isn't echoed back...
    app.act(Action::Focus { pane: p }).unwrap();
    // ...but the agent's focus change is.
    agent.act(Action::Focus { pane: first }).unwrap();
    let l = next_layout(&app_rx);
    assert_eq!(l.tabs[0].focused, first);

    // Errors for panes that aren't there.
    assert!(agent.act(Action::Close { pane: 99 }).is_err());
    assert!(
        agent
            .act(Action::Split {
                pane: 99,
                dir: Dir::Row,
                cwd: None,
                argv: None,
                focus: false
            })
            .is_err()
    );
    agent.act(Action::Close { pane: p }).unwrap();
    assert_eq!(next_layout(&app_rx).panes(), vec![first]);
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn panes_can_be_read_and_marks_seen() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, _rx) = events();
    let c = Client::connect(&path, f).unwrap();
    let s = sh(&c, None);
    type_text(
        &c,
        s,
        "printf 'a-%s\\n' 1 2; printf '\\033]7770;m1;%d\\007' 5\r",
    );
    eventually("the mark", || {
        status(&c, s, Some("m1"))
            == Reply::Status {
                ended: false,
                code: None,
                mark: Some(5),
                agent: None,
            }
    });
    let text = read(&c, s, ReadSource::Visible);
    assert!(text.contains("a-1\na-2"), "{text}");
    assert_eq!(
        status(&c, s, Some("m2")),
        Reply::Status {
            ended: false,
            code: None,
            mark: None,
            agent: None,
        }
    );
    assert!(matches!(status(&c, 99, None), Reply::Error { .. }));
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn the_daemon_exits_when_nothing_is_left() {
    let path = socket();
    let (_d, h) = start(&path, Duration::from_millis(300));
    {
        let (f, _rx) = events();
        let c = Client::connect(&path, f).unwrap();
        let s = sh(&c, None);
        c.act(Action::Close { pane: s }).unwrap();
    }
    let start = Instant::now();
    h.join().unwrap().unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(!path.exists());
}

#[test]
fn a_client_that_stops_reading_stalls_nobody() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    // This client's callback blocks on its first frame, so it stops reading.
    let gate = Arc::new(Mutex::new(()));
    let held = gate.lock().unwrap();
    let g = gate.clone();
    let stuck = Client::connect(&path, move |_e: ClientEvent<'_>| {
        drop(g.lock());
    })
    .unwrap();
    let flood = sh(
        &stuck,
        Some("while :; do echo flood flood flood flood; done"),
    );
    stuck.send(&ClientMsg::Attach { session: flood }).unwrap();
    // Acks keep the flood's frames coming, to fill the socket and the outbox.
    for seq in 1..200 {
        stuck
            .send(&ClientMsg::Ack {
                session: flood,
                seq,
            })
            .unwrap();
    }

    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    let s = sh(&c, None);
    c.send(&ClientMsg::Attach { session: s }).unwrap();
    type_text(&c, s, "echo fine-$((2+3))\r");
    wait_for(&c, &rx, s, "fine-5");
    drop(held);
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn stale_sockets_are_replaced_and_live_ones_kept() {
    let path = socket();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    // Left behind by a daemon that died.
    drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
    let (d, h) = start(&path, Duration::from_secs(60));
    // The old file was already there, so wait until the new daemon answers.
    let deadline = Instant::now() + Duration::from_secs(2);
    let c = loop {
        let (f, _rx) = events();
        match Client::connect(&path, f) {
            Ok(c) => break c,
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(e) => panic!("{e}"),
        }
    };
    c.hello().unwrap();

    // A second daemon on the same socket refuses to start.
    let other = Daemon::new("other");
    let cfg = SocketConfig {
        socket: path.clone(),
        idle_exit: Duration::from_secs(1),
    };
    let e = serve_socket(&other, &cfg).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);
    assert!(path.exists());

    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn clients_racing_to_start_share_one_daemon() {
    let path = socket();
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_kibannd"));
    let command = vec![exe.clone().into_os_string()];
    let clients: Vec<_> = (0..3)
        .map(|_| {
            let (path, command) = (path.clone(), command.clone());
            thread::spawn(move || {
                let (f, rx) = events();
                (Client::connect_or_start(&path, &command, f).unwrap(), rx)
            })
        })
        .collect();
    let clients: Vec<_> = clients.into_iter().map(|t| t.join().unwrap()).collect();
    let builds: Vec<_> = clients.iter().map(|(c, _)| c.hello().unwrap().1).collect();
    assert_eq!(builds[0], daemon::build_id(&exe).unwrap());
    assert!(builds.iter().all(|b| *b == builds[0]));

    let s = sh(&clients[0].0, None);
    for (c, _) in &clients {
        assert_eq!(c.layout().unwrap().panes(), vec![s]);
    }
    clients[0].0.send(&ClientMsg::Shutdown).unwrap();
    // Every client sees the daemon go.
    for (_, rx) in &clients {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ev::Disconnected) => break,
                _ => assert!(Instant::now() < deadline, "still connected"),
            }
        }
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while path.exists() {
        assert!(Instant::now() < deadline, "socket left behind");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_daemon_can_run_in_process() {
    let (f, rx) = events();
    let c = Client::in_process(f).unwrap();
    let s = sh(&c, None);
    c.send(&ClientMsg::Attach { session: s }).unwrap();
    type_text(&c, s, "echo inproc-$((3*3))\r");
    wait_for(&c, &rx, s, "inproc-9");
}

/// Median time from a typed character to the frame that shows it.
fn echo_latency(c: &Client, rx: &Receiver<Ev>, s: SessionId) -> Duration {
    c.send(&ClientMsg::Attach { session: s }).unwrap();
    type_text(c, s, "stty -icanon; echo ready\r");
    let mut screen = wait_for(c, rx, s, "ready");
    let mut times = Vec::new();
    for i in 0..30 {
        thread::sleep(Duration::from_millis(30)); // past the frame throttle
        // Frames still coming (the prompt after the last line, on a slow
        // machine) are acked and applied too: dropping them unacked stops
        // the session sending at two outstanding, and nothing would echo.
        while let Ok(ev) = rx.try_recv() {
            if let Ev::Frame(id, f) = ev {
                if id == s {
                    c.send(&ClientMsg::Ack {
                        session: s,
                        seq: f.seq,
                    })
                    .unwrap();
                    screen.apply(&f);
                }
            }
        }
        let ch = char::from(b'a' + (i % 26) as u8).to_string();
        let marker = format!("{ch}{ch}");
        let t = Instant::now();
        type_text(c, s, &ch);
        type_text(c, s, &ch);
        loop {
            match rx.recv_timeout(Duration::from_secs(2)).expect("echo") {
                Ev::Frame(id, f) if id == s => {
                    c.send(&ClientMsg::Ack {
                        session: s,
                        seq: f.seq,
                    })
                    .unwrap();
                    screen.apply(&f);
                    if screen.0.values().any(|r| r.trim_end().ends_with(&marker)) {
                        break;
                    }
                }
                _ => {}
            }
        }
        times.push(t.elapsed());
        type_text(c, s, "\r");
    }
    times.sort();
    times[times.len() / 2]
}

#[test]
fn the_socket_adds_little_latency() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    let s = sh(&c, None);
    let socket = echo_latency(&c, &rx, s);

    let (f, rx) = events();
    let local = Client::in_process(f).unwrap();
    let s = sh(&local, None);
    let in_process = echo_latency(&local, &rx, s);

    eprintln!("echo latency: socket {socket:?}, in process {in_process:?}");
    // A margin for a busy machine, still well under one frame (16 ms).
    assert!(
        socket < in_process + Duration::from_millis(5),
        "socket {socket:?} vs in process {in_process:?}"
    );
    d.shutdown();
    h.join().unwrap().unwrap();
}

/// The agent state the daemon reports for `pane`.
fn agent_state(c: &Client, pane: SessionId) -> Option<AgentState> {
    match status(c, pane, None) {
        Reply::Status { agent, .. } => agent.map(|a| a.state),
        other => panic!("{other:?}"),
    }
}

/// Wait for a status event giving `pane` this state.
fn next_status(rx: &Receiver<Ev>, pane: SessionId, state: Option<AgentState>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Ev::Status(m)) if m.get(&pane).map(|s| s.state) == state => return,
            Ok(_) => {}
            Err(_) => panic!("no status event with p{pane} {state:?}"),
        }
    }
}

#[test]
fn agents_are_detected_and_done_while_unseen() {
    let path = socket();
    // A fake Claude Code: a shell named `claude` that sets the title the
    // way Claude does (spinner while working, ✳ when idle).
    let dir = path.parent().unwrap().join("bin");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink("/bin/sh", dir.join("claude")).unwrap();
    std::fs::write(
        dir.join("agent.sh"),
        "printf '\\033]0;\\342\\240\\213 Working\\007'; echo busy; read x\n\
         printf '\\033]0;\\342\\234\\263 Claude Code\\007'; echo finished; read y\n",
    )
    .unwrap();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, rx) = events();
    let app = Client::connect(&path, f).unwrap();
    let first = sh(&app, None);
    app.watch().unwrap();

    let script = format!(
        "set -m; {0}/claude {0}/agent.sh; exec /bin/sh",
        dir.display()
    );
    let (p, _) = app
        .act(Action::Split {
            pane: first,
            dir: Dir::Row,
            cwd: None,
            argv: Some(vec!["/bin/sh".into(), "-c".into(), script]),
            focus: false,
        })
        .unwrap();
    let p = p.unwrap();
    next_status(&rx, p, Some(AgentState::Working));
    assert_eq!(agent_state(&app, first), None, "a plain shell has none");
    let panes = match app.request(|req| ClientMsg::Panes { req }).unwrap() {
        Reply::Panes { panes } => panes,
        other => panic!("{other:?}"),
    };
    let info = panes.iter().find(|i| i.id == p).unwrap();
    assert_eq!(info.agent.as_deref(), Some("claude"));
    assert_eq!(info.state, Some(AgentState::Working));

    // It finishes while the user looks at the other pane: done, until they
    // focus it.
    type_text(&app, p, "\r");
    next_status(&rx, p, Some(AgentState::Done));
    app.act(Action::Focus { pane: p }).unwrap();
    next_status(&rx, p, Some(AgentState::Idle));

    // A report beats the screen; an older one is dropped; release undoes it.
    let report = |state, seq| {
        app.request(|req| ClientMsg::Report {
            req,
            pane: p,
            agent: "bot".into(),
            state,
            seq: Some(seq),
        })
        .unwrap()
    };
    assert_eq!(report(AgentState::Blocked, 2), Reply::Ok);
    next_status(&rx, p, Some(AgentState::Blocked));
    assert!(matches!(
        report(AgentState::Working, 1),
        Reply::Error { .. }
    ));
    assert_eq!(agent_state(&app, p), Some(AgentState::Blocked));
    assert!(matches!(report(AgentState::Done, 3), Reply::Error { .. }));
    assert_eq!(
        app.request(|req| ClientMsg::Release { req, pane: p })
            .unwrap(),
        Reply::Ok
    );
    next_status(&rx, p, Some(AgentState::Idle));

    // The agent exits to the shell: no status.
    type_text(&app, p, "\r");
    next_status(&rx, p, None);
    d.shutdown();
    h.join().unwrap().unwrap();
}

/// Wait for a git event in which `pane`'s place satisfies `f`.
fn next_git(rx: &Receiver<Ev>, pane: SessionId, what: &str, f: impl Fn(&PaneDir) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Ev::Git(m)) if m.get(&pane).is_some_and(&f) => return,
            Ok(_) => {}
            Err(_) => panic!("never: {what}"),
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn panes_know_their_checkout() {
    let path = socket();
    let repo = path.parent().unwrap().join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    let repo = repo.canonicalize().unwrap();
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "first"]);

    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, rx) = events();
    let app = Client::connect(&path, f).unwrap();
    app.watch().unwrap();
    let (p, _) = app
        .act(Action::NewTab {
            near: None,
            cwd: Some(repo.to_string_lossy().into_owned()),
            argv: Some(vec!["/bin/sh".into()]),
            focus: true,
        })
        .unwrap();
    let p = p.unwrap();
    next_git(&rx, p, "the branch", |d| {
        d.git
            .as_ref()
            .is_some_and(|g| g.branch.as_deref() == Some("main") && g.repo == "proj")
    });

    // Edits show up from file events alone.
    std::fs::write(repo.join("b.txt"), "new\n").unwrap();
    next_git(&rx, p, "one change", |d| {
        d.git.as_ref().is_some_and(|g| g.changed == 1)
    });

    // A second pane in the repo shares the checkout.
    let (q, _) = app
        .act(Action::Split {
            pane: p,
            dir: Dir::Row,
            cwd: None,
            argv: Some(vec!["/bin/sh".into()]),
            focus: false,
        })
        .unwrap();
    let q = q.unwrap();
    next_git(&rx, q, "the second pane", |d| d.git.is_some());
    assert_eq!(d.checkouts_watched(), 1);
    let panes = match app.request(|req| ClientMsg::Panes { req }).unwrap() {
        Reply::Panes { panes } => panes,
        other => panic!("{other:?}"),
    };
    assert!(
        panes
            .iter()
            .all(|i| i.git.as_ref().is_some_and(|g| g.changed == 1))
    );

    // Leaving the repo, and closing the other pane, stops watching it.
    type_text(&app, p, "cd /\r");
    next_git(&rx, p, "left the repo", |d| {
        d.git.is_none() && d.shown == "/"
    });
    app.act(Action::Close { pane: q }).unwrap();
    eventually("no checkouts watched", || d.checkouts_watched() == 0);
    d.shutdown();
    h.join().unwrap().unwrap();
}

/// The next frame for `session` whose default background is `bg`.
fn frame_with_bg(c: &Client, rx: &Receiver<Ev>, session: SessionId, bg: u32) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(Ev::Frame(s, f)) = rx.recv_timeout(Duration::from_millis(100))
            && s == session
        {
            c.send(&ClientMsg::Ack {
                session,
                seq: f.seq,
            })
            .unwrap();
            if f.default_bg == bg {
                assert!(f.full, "a theme change redraws everything");
                return;
            }
        }
    }
    panic!("no frame with background {bg:06x} for p{session}");
}

#[test]
fn a_theme_reaches_every_session_and_new_ones() {
    let path = socket();
    let (d, h) = start(&path, Duration::from_secs(60));
    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    let a = sh(&c, None);
    c.send(&ClientMsg::Attach { session: a }).unwrap();
    frame_with_bg(&c, &rx, a, 0x000000);

    let light = proto::theme::Theme::kibann_light();
    let r = c
        .request(|req| ClientMsg::SetTheme {
            req,
            theme: light.clone(),
        })
        .unwrap();
    assert!(matches!(r, Reply::Ok), "{r:?}");
    frame_with_bg(&c, &rx, a, 0xffffff);

    let b = sh(&c, None);
    c.send(&ClientMsg::Attach { session: b }).unwrap();
    frame_with_bg(&c, &rx, b, 0xffffff);
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn the_layout_comes_back_after_the_sessions_ended() {
    let path = socket();
    let dir = path.parent().unwrap().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let file = path.parent().unwrap().join("state").join("layout.json");

    let (d, h) = start(&path, Duration::from_secs(60));
    d.save_layout_to(file.clone(), Duration::from_millis(100));
    let (f, _rx) = events();
    let c = Client::connect(&path, f).unwrap();
    // Nothing saved yet: nothing to restore.
    assert_eq!(c.restore().unwrap(), (0, 0));
    let (a, _) = c
        .act(Action::NewTab {
            near: None,
            cwd: Some(dir.to_string_lossy().into_owned()),
            argv: Some(vec!["/bin/sh".into()]),
            focus: true,
        })
        .unwrap();
    let (b, _) = c
        .act(Action::Split {
            pane: a.unwrap(),
            dir: Dir::Row,
            cwd: None,
            argv: Some(vec!["/bin/sh".into()]),
            focus: true,
        })
        .unwrap();
    c.act(Action::NewTab {
        near: None,
        cwd: None,
        argv: Some(vec!["/bin/sh".into()]),
        focus: false,
    })
    .unwrap();
    // Restoring with panes open does nothing.
    assert_eq!(c.restore().unwrap(), (0, 0));
    let want = dir.to_string_lossy().into_owned();
    eventually("the layout saved with the directory", || {
        daemon::saved::load(&file)
            .is_some_and(|s| s.panes() == 3 && s.dirs.values().any(|d| *d == want))
    });
    // Ending everything on purpose keeps the file as it was.
    drop(c);
    d.shutdown();
    h.join().unwrap().unwrap();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(daemon::saved::load(&file).map(|s| s.panes()), Some(3));

    let (d, h) = start(&path, Duration::from_secs(60));
    d.save_layout_to(file.clone(), Duration::from_millis(100));
    let (f, rx) = events();
    let c = Client::connect(&path, f).unwrap();
    assert!(c.watch().unwrap().tabs.is_empty());
    assert!(next_layout(&rx).tabs.is_empty()); // the same, as an event
    assert_eq!(c.restore().unwrap(), (2, 3));
    let l = next_layout(&rx);
    assert_eq!(l.tabs.len(), 2);
    assert_eq!(l.tabs[0].tree.panes().len(), 2);
    assert_eq!(l.active, Some(l.tabs[0].id), "the shown tab is shown again");
    assert_eq!(
        l.tabs[0].focused,
        l.tabs[0].tree.panes()[1],
        "and its focused pane"
    );
    let _ = b;
    let first = l.tabs[0].tree.panes()[0];
    eventually("the first pane starts in its directory", || {
        let Ok(Reply::Panes { panes }) = c.request(|req| ClientMsg::Panes { req }) else {
            return false;
        };
        panes
            .iter()
            .any(|p| p.id == first && p.cwd.as_deref() == Some(want.as_str()))
    });
    d.shutdown();
    h.join().unwrap().unwrap();
}
