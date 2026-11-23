//! The daemon on Windows: over its named pipe, with cmd.exe in its panes.
//! The Unix tests (`daemon.rs`) need a POSIX shell.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use client::Client;
use daemon::server::{Daemon, SocketConfig, serve_socket};
use proto::InputEvent;
use proto::ReadSource;
use proto::Resize;
use proto::layout::Action;
use proto::layout::PaneId;
use proto::wire::{ClientMsg, Reply};

fn socket() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("kd{}-{n}", std::process::id()))
        .join("s.sock")
}

/// Poll until `f` holds.
fn eventually(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f() {
        assert!(Instant::now() < deadline, "never: {what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn connect(socket: &Path) -> Client {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match Client::connect(socket, |_| {}) {
            Ok(c) => return c,
            Err(e) if Instant::now() >= deadline => panic!("no daemon: {e}"),
            Err(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn cmd(c: &Client) -> PaneId {
    let (pane, _) = c
        .act(Action::NewTab {
            near: None,
            cwd: None,
            argv: Some(vec!["cmd.exe".into(), "/d".into(), "/q".into()]),
            focus: true,
        })
        .expect("new tab");
    let pane = pane.unwrap();
    c.send(&ClientMsg::Resize {
        session: pane,
        size: Resize {
            cols: 80,
            rows: 20,
            cell_w: 8,
            cell_h: 16,
        },
    })
    .unwrap();
    pane
}

fn screen(c: &Client, pane: PaneId) -> String {
    match c
        .request(|req| ClientMsg::Read {
            req,
            pane,
            source: ReadSource::Recent,
            lines: None,
        })
        .unwrap()
    {
        Reply::Text { text } => text,
        other => panic!("{other:?}"),
    }
}

fn type_text(c: &Client, session: PaneId, text: &str) {
    c.send(&ClientMsg::Input {
        session,
        event: InputEvent::Text { data: text.into() },
    })
    .unwrap();
}

#[test]
fn a_shell_over_the_pipe() {
    let socket = socket();
    let d = Daemon::new("test-build");
    let cfg = SocketConfig {
        socket: socket.clone(),
        idle_exit: Duration::from_secs(60),
    };
    let dd: Arc<Daemon> = d.clone();
    thread::spawn(move || serve_socket(&dd, &cfg));
    let a = connect(&socket);
    let pane = cmd(&a);
    // Only the output has the variable's value; the typed line has its name.
    type_text(&a, pane, "echo hi-%OS%\r");
    eventually("the echo", || screen(&a, pane).contains("hi-Windows_NT"));
    // A second client sees the same session.
    let b = connect(&socket);
    type_text(&b, pane, "echo two-%OS%\r");
    eventually("the second echo", || {
        screen(&a, pane).contains("two-Windows_NT")
    });
    assert!(client::lock_held(&socket));
    // A second daemon on the same pipe gives up.
    let other = Daemon::new("test-build");
    let e = serve_socket(
        &other,
        &SocketConfig {
            socket: socket.clone(),
            idle_exit: Duration::from_secs(60),
        },
    )
    .unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AddrInUse);
}

#[test]
fn clients_start_the_daemon() {
    let socket = socket();
    let command = vec![env!("CARGO_BIN_EXE_kibannd").into()];
    let c = Client::connect_or_start(&socket, &command, |_| {}).unwrap();
    let pane = cmd(&c);
    type_text(&c, pane, "echo started-%OS%\r");
    eventually("the echo", || {
        screen(&c, pane).contains("started-Windows_NT")
    });
    c.stop(&socket).unwrap();
    assert!(!client::lock_held(&socket));
}
