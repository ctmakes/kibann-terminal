//! The `kibann` CLI (as `kibannd pane ...`) against a daemon with real shells.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use daemon::client::Client;
use daemon::server::{Daemon, PaneEnv, SocketConfig, serve_socket};
use proto::layout::Action;
use serde_json::Value;

fn start() -> (PathBuf, Arc<Daemon>, JoinHandle<std::io::Result<()>>) {
    let socket = std::env::temp_dir()
        .join(format!("kc{}", std::process::id()))
        .join("s.sock");
    let d = Daemon::new("test-build");
    d.set_pane_env(PaneEnv {
        socket: Some(socket.clone()),
        ..PaneEnv::default()
    });
    let cfg = SocketConfig {
        socket: socket.clone(),
        idle_exit: Duration::from_secs(60),
    };
    let dd = d.clone();
    let h = thread::spawn(move || serve_socket(&dd, &cfg));
    let deadline = Instant::now() + Duration::from_secs(2);
    while !socket.exists() {
        assert!(Instant::now() < deadline, "daemon didn't start");
        thread::sleep(Duration::from_millis(10));
    }
    (socket, d, h)
}

/// Run `kibann args...` as if from a shell in `pane`.
fn kibann(socket: &Path, pane: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kibannd"));
    cmd.args(args).env("KIBANN_SOCKET", socket);
    match pane {
        Some(p) => cmd.env("KIBANN_PANE", p),
        None => cmd.env_remove("KIBANN_PANE"),
    };
    cmd.output().unwrap()
}

fn ok(out: Output) -> Value {
    assert!(
        out.status.success(),
        "exit {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn fails(out: &Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code));
    serde_json::from_slice(&out.stderr).unwrap_or(Value::Null)
}

#[test]
fn an_agent_splits_runs_reads_and_waits() {
    let (socket, d, h) = start();
    let c = Client::connect(&socket, |_| {}).unwrap();
    let (first, _) = c
        .act(Action::NewTab {
            near: None,
            cwd: Some("/".into()),
            argv: Some(vec!["/bin/sh".into()]),
            focus: true,
        })
        .unwrap();
    let me = format!("p{}", first.unwrap());
    let me = me.as_str();

    // The calling pane is the default, and focus stays where it was.
    let made = ok(kibann(
        &socket,
        Some(me),
        &["pane", "split", "--direction", "down"],
    ));
    let p = made["pane"]["id"].as_str().unwrap().to_owned();
    assert_ne!(p, me);
    let layout = c.layout().unwrap();
    assert_eq!(layout.panes().len(), 2);
    assert_eq!(format!("p{}", layout.tabs[0].focused), me);

    // The new shell knows where it is.
    let r = ok(kibann(
        &socket,
        Some(me),
        &[
            "pane",
            "run",
            &p,
            "echo pane=$KIBANN_PANE in=$(pwd)",
            "--wait",
        ],
    ));
    assert_eq!(r["exit_code"], 0);
    let out = kibann(&socket, Some(me), &["pane", "read", &p]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains(&format!("pane={p} in=/\n")), "{text}");

    // Exit codes come back; text can be waited for.
    let r = ok(kibann(
        &socket,
        None,
        &["pane", "run", &p, "sh -c 'exit 7'", "--wait"],
    ));
    assert_eq!(r["exit_code"], 7);
    ok(kibann(
        &socket,
        None,
        &["pane", "run", &p, "sleep 0.3; echo do''ne-1"],
    ));
    let r = ok(kibann(
        &socket,
        None,
        &["pane", "wait", &p, "--regex", "^done-\\d$"],
    ));
    assert_eq!(r["matched"], "done-1");
    let out = kibann(
        &socket,
        None,
        &[
            "pane",
            "wait",
            &p,
            "--match",
            "never-printed",
            "--timeout",
            "300",
        ],
    );
    assert_eq!(fails(&out, 1)["error"]["code"], "timeout");

    // List and current.
    let list = ok(kibann(&socket, Some(me), &["pane", "list"]));
    assert_eq!(list["panes"].as_array().unwrap().len(), 2);
    let cur = ok(kibann(&socket, Some(me), &["pane", "current"]));
    assert_eq!(cur["pane"]["id"], me);
    assert_eq!(cur["pane"]["focused"], true);

    // The shell exiting ends a wait for it.
    ok(kibann(&socket, None, &["pane", "run", &p, "exit 4"]));
    let r = ok(kibann(&socket, None, &["pane", "wait", &p, "--exit"]));
    assert_eq!(r["exit_code"], 4);
    assert_eq!(c.layout().unwrap().panes().len(), 1);

    // Errors.
    let out = kibann(&socket, None, &["pane", "read", "p99"]);
    assert_eq!(fails(&out, 1)["error"]["code"], "not_found");
    let out = kibann(&socket, None, &["pane", "split"]);
    assert_eq!(fails(&out, 1)["error"]["code"], "no_pane");
    let out = kibann(&socket, None, &["pane", "split", "--direction", "up"]);
    fails(&out, 2);
    let out = kibann(&socket, None, &["pane", "frobnicate"]);
    fails(&out, 2);

    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn no_daemon_is_an_error() {
    let out = kibann(
        Path::new("/tmp/kibann-none/s.sock"),
        None,
        &["pane", "list"],
    );
    assert_eq!(fails(&out, 1)["error"]["code"], "not_running");
}
