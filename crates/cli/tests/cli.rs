//! The `kibann` CLI (as `kibann-cli pane ...`) against a daemon with real shells.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use client::Client;
use daemon::server::{Daemon, PaneEnv, SocketConfig, serve_socket};
use proto::layout::Action;
use serde_json::Value;

fn start(tag: &str) -> (PathBuf, Arc<Daemon>, JoinHandle<std::io::Result<()>>) {
    let socket = std::env::temp_dir()
        .join(format!("kc{}{tag}", std::process::id()))
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
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kibann-cli"));
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
    let (socket, d, h) = start("a");
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

#[test]
fn agents_can_be_waited_on_and_report_themselves() {
    let (socket, d, h) = start("b");
    // A fake Claude Code (see the daemon test of the same idea).
    let dir = socket.parent().unwrap().join("bin");
    std::fs::create_dir_all(&dir).unwrap();
    std::os::unix::fs::symlink("/bin/sh", dir.join("claude")).unwrap();
    std::fs::write(
        dir.join("agent.sh"),
        "printf '\\033]0;\\342\\240\\213 Working\\007'; read x\n\
         printf '\\033]0;\\342\\234\\263 Claude Code\\007'; read y\n",
    )
    .unwrap();
    let c = Client::connect(&socket, |_| {}).unwrap();
    let (p, _) = c
        .act(Action::NewTab {
            near: None,
            cwd: None,
            argv: Some(vec!["/bin/sh".into()]),
            focus: true,
        })
        .unwrap();
    let p = format!("p{}", p.unwrap());
    let p = p.as_str();
    let run = format!("{0}/claude {0}/agent.sh", dir.display());
    ok(kibann(&socket, None, &["pane", "run", p, &run]));

    let r = ok(kibann(
        &socket,
        None,
        &["pane", "wait", p, "--until", "working"],
    ));
    assert_eq!(r["agent"], "claude");
    assert_eq!(r["state"], "working");
    let list = ok(kibann(&socket, None, &["pane", "list"]));
    assert_eq!(list["panes"][0]["agent"], "claude");
    assert_eq!(list["panes"][0]["state"], "working");

    // No app is watching, so nobody saw it finish.
    ok(kibann(&socket, None, &["pane", "keys", p, "enter"]));
    let r = ok(kibann(
        &socket,
        None,
        &["pane", "wait", p, "--until", "done,blocked"],
    ));
    assert_eq!(r["state"], "done");

    // Reports.
    let report = |state: &str, seq: &str| {
        kibann(
            &socket,
            None,
            &[
                "pane", "report", p, "--agent", "bot", "--state", state, "--seq", seq,
            ],
        )
    };
    ok(report("blocked", "5"));
    let r = ok(kibann(
        &socket,
        None,
        &["pane", "wait", p, "--until", "blocked"],
    ));
    assert_eq!(r["agent"], "bot");
    assert_eq!(fails(&report("working", "4"), 1)["error"]["code"], "stale");
    fails(&report("done", "6"), 2);
    ok(kibann(&socket, None, &["pane", "release", p]));
    let r = ok(kibann(
        &socket,
        None,
        &["pane", "wait", p, "--until", "idle", "--timeout", "2000"],
    ));
    assert_eq!(r["agent"], "claude");

    // Usage errors.
    fails(
        &kibann(&socket, None, &["pane", "wait", p, "--until", "busy"]),
        2,
    );
    // The agent exits; its status goes once the shell is back.
    ok(kibann(&socket, None, &["pane", "keys", p, "enter"]));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let list = ok(kibann(&socket, None, &["pane", "list"]));
        if list["panes"][0]["agent"].is_null() {
            break;
        }
        assert!(Instant::now() < deadline, "status never cleared: {list}");
        thread::sleep(Duration::from_millis(50));
    }
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn pane_list_shows_the_checkout() {
    let (socket, d, h) = start("c");
    let repo = socket.parent().unwrap().join("proj");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(&repo)
            .args(["-c", "init.defaultBranch=trunk"])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("x"), "").unwrap();
    let c = Client::connect(&socket, |_| {}).unwrap();
    c.act(Action::NewTab {
        near: None,
        cwd: Some(repo.to_string_lossy().into_owned()),
        argv: Some(vec!["/bin/sh".into()]),
        focus: true,
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let pane = loop {
        let list = ok(kibann(&socket, None, &["pane", "list"]));
        let p = list["panes"][0].clone();
        if !p["git"].is_null() {
            break p;
        }
        assert!(Instant::now() < deadline, "no git: {list}");
        thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(pane["git"]["branch"], "trunk");
    assert_eq!(pane["git"]["repo"], "proj");
    assert_eq!(pane["git"]["changed"], 1);

    // Its diff, as a patch and as files.
    let id = pane["id"].as_str().unwrap().to_owned();
    let out = kibann(&socket, None, &["pane", "diff", &id]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("+++ b/x"));
    let stat = ok(kibann(&socket, None, &["pane", "diff", &id, "--stat"]));
    assert_eq!(stat["files"][0]["path"], "x");
    assert_eq!(stat["files"][0]["status"], "?");
    let out = kibann(&socket, None, &["pane", "diff", "p999"]);
    assert_eq!(fails(&out, 1)["error"]["code"], "not_found");
    d.shutdown();
    h.join().unwrap().unwrap();
}

/// `kibann args...` run in `dir`, with its own config and state folders.
fn kibann_at(socket: &Path, dir: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kibann-cli"))
        .args(args)
        .current_dir(dir)
        .env("KIBANN_SOCKET", socket)
        .env("KIBANN_CONFIG_DIR", home.join("config"))
        .env("KIBANN_STATE_DIR", home.join("state"))
        .env_remove("KIBANN_PANE")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn workspaces_are_checked_opened_and_saved() {
    let (socket, d, h) = start("l");
    let c = Client::connect(&socket, |_| {}).unwrap();
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("kl{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("web")).unwrap();
    std::fs::create_dir_all(repo.join(".kibann")).unwrap();
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();

    // A bad file: every problem, with its line.
    std::fs::write(
        repo.join(".kibann/workspace.toml"),
        "[[tab]]\nsplit = \"right\"\npanes = [{ cwd = \"nope\" }, { comand = \"x\" }]\n",
    )
    .unwrap();
    let out = kibann_at(&socket, &repo, &home, &["workspace", "check"]);
    let e = fails(&out, 1);
    assert_eq!(e["error"]["code"], "invalid");
    let msg = e["error"]["message"].as_str().unwrap();
    assert!(msg.contains("workspace.toml:3:"), "{msg}");

    // A good one: what it runs, and that it asks first.
    let text = "[[tab]]\ntitle = \"dev\"\nsplit = \"right\"\npanes = [\n  { command = \"echo from-workspace-$((6*7))\", size = 60 },\n  { cwd = \"web\", focus = true },\n]\n";
    std::fs::write(repo.join(".kibann/workspace.toml"), text).unwrap();
    let r = ok(kibann_at(
        &socket,
        &repo.join("web"),
        &home,
        &["workspace", "check"],
    ));
    assert_eq!(r["name"], "repo");
    assert_eq!(r["origin"], "project");
    assert_eq!(r["panes"], 2);
    assert_eq!(r["asks"], true);
    assert_eq!(r["runs"][0]["run"], "echo from-workspace-$((6*7))");

    // Without a terminal to ask on, it won't run them...
    let out = kibann_at(&socket, &repo, &home, &["workspace", "open"]);
    assert_eq!(fails(&out, 1)["error"]["code"], "untrusted");
    assert!(c.layout().unwrap().tabs.is_empty());
    // ...until told to; then that version is trusted.
    let r = ok(kibann_at(
        &socket,
        &repo,
        &home,
        &["workspace", "open", "--yes"],
    ));
    assert_eq!(r["opened"], "repo");
    let r = ok(kibann_at(&socket, &repo, &home, &["workspace", "check"]));
    assert_eq!(r["asks"], false);
    let layout = c.layout().unwrap();
    assert_eq!(layout.tabs.len(), 1);
    let tab = &layout.tabs[0];
    assert_eq!(tab.title.as_deref(), Some("dev"));
    let panes = tab.tree.panes();
    assert_eq!(tab.focused, panes[1]);
    let first = format!("p{}", panes[0]);
    ok(kibann_at(
        &socket,
        &repo,
        &home,
        &[
            "pane",
            "wait",
            &first,
            "--regex",
            "^from-workspace-42$",
            "--timeout",
            "5000",
        ],
    ));

    // Saved back into the project: relative directories, the title kept.
    let out = kibann_at(
        &socket,
        &repo,
        &home,
        &["workspace", "save", "--to", ".kibann/workspace.toml"],
    );
    assert_eq!(fails(&out, 1)["error"]["code"], "exists");
    let r = ok(kibann_at(
        &socket,
        &repo,
        &home,
        &[
            "workspace",
            "save",
            "--to",
            ".kibann/workspace.toml",
            "--force",
        ],
    ));
    assert_eq!(r["name"], "repo");
    let saved = std::fs::read_to_string(repo.join(".kibann/workspace.toml")).unwrap();
    assert!(saved.contains("title = \"dev\""), "{saved}");
    assert!(
        saved.contains("{ cwd = \"web\", size = 40, focus = true }"),
        "{saved}"
    );
    assert!(saved.contains("{ size = 60 }"), "{saved}");
    // What the user saved opens without asking.
    let r = ok(kibann_at(&socket, &repo, &home, &["workspace", "check"]));
    assert_eq!(r["asks"], false);

    // Named workspaces, and copying a built-in one to edit.
    let r = ok(kibann_at(
        &socket,
        &repo,
        &home,
        &["workspace", "save", "mine", "--from", "system"],
    ));
    let mine = std::path::PathBuf::from(r["saved"].as_str().unwrap());
    assert_eq!(mine, home.join("config/workspaces/mine.toml"));
    let list = ok(kibann_at(&socket, &repo, &home, &["workspace", "list"]));
    let names: Vec<(&str, &str)> = list["workspaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| (l["name"].as_str().unwrap(), l["origin"].as_str().unwrap()))
        .collect();
    assert_eq!(names[0], ("repo", "project"));
    assert_eq!(names[1], ("mine", "user"));
    assert!(names.contains(&("system", "builtin")));
    let out = kibann_at(&socket, &repo, &home, &["workspace", "open", "nope"]);
    assert_eq!(fails(&out, 1)["error"]["code"], "not_found");

    // watch runs the command again and again.
    let mut child = Command::new(env!("CARGO_BIN_EXE_kibann-cli"))
        .args(["watch", "0.1", "--", "echo", "tick"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(450));
    child.kill().unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.matches("tick").count() >= 2, "{text:?}");
    assert!(text.contains("Every 0.1s: echo tick"), "{text:?}");

    std::fs::remove_dir_all(&root).unwrap();
    d.shutdown();
    h.join().unwrap().unwrap();
}

#[test]
fn screens_are_listed_copied_checked_and_used() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("ks{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // No daemon is needed: screens are files.
    let socket = root.join("none.sock");
    let run = |args: &[&str]| kibann_at(&socket, &root, &home, args);

    let list = ok(run(&["screen", "list"]));
    let names: Vec<&str> = list["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    for n in ["default", "board", "focus", "departures"] {
        assert!(names.contains(&n), "{names:?}");
    }

    // A copy to edit, checked, then shown.
    let r = ok(run(&["screen", "new", "mine", "--from", "board"]));
    let file = PathBuf::from(r["path"].as_str().unwrap());
    assert_eq!(file, home.join("config/screens/mine/screen.json"));
    let out = run(&["screen", "new", "mine"]);
    assert!(
        fails(&out, 1)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exists")
    );
    let r = ok(run(&["screen", "check", "mine"]));
    assert_eq!(r["builtin"], false);
    assert_eq!(r["parts"][0]["name"], "board");
    assert_eq!(r["parts"][0]["permissions"]["read"], true);
    // Built in: nothing to ask.
    assert_eq!(r["parts"][0]["asks"], serde_json::json!([]));
    ok(run(&["screen", "use", "mine"]));
    let config = std::fs::read_to_string(home.join("config/config")).unwrap();
    assert_eq!(config, "screen = mine\n");
    // Back to the default takes the line out.
    ok(run(&["screen", "use", "default"]));
    assert_eq!(
        std::fs::read_to_string(home.join("config/config")).unwrap(),
        ""
    );

    // A part of the user's, with permissions it must be allowed.
    let r = ok(run(&["screen", "part", "prs"]));
    let dir = PathBuf::from(r["path"].as_str().unwrap());
    assert!(dir.join("index.html").is_file());
    std::fs::write(
        dir.join("part.json"),
        r#"{"about": "PRs", "permissions": {"exec": ["gh"], "net": ["api.github.com"]}}"#,
    )
    .unwrap();
    let text = std::fs::read_to_string(&file)
        .unwrap()
        .replace("\"part\": \"board\"", "\"part\": \"prs\"");
    std::fs::write(&file, text).unwrap();
    let r = ok(run(&["screen", "check", "mine"]));
    assert_eq!(r["parts"][0]["name"], "prs");
    assert_eq!(
        r["parts"][0]["asks"],
        serde_json::json!(["run gh", "connect to api.github.com"])
    );

    // Mistakes say where.
    std::fs::write(
        &file,
        "{\n  \"root\": {\"type\": \"block\", \"colour\": 1}\n}\n",
    )
    .unwrap();
    let out = run(&["screen", "check", "mine"]);
    let msg = fails(&out, 1)["error"]["message"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(msg.contains("screen.json:2:"), "{msg}");
    assert!(msg.contains("unknown field `colour`"), "{msg}");
    let out = run(&["screen", "use", "nope"]);
    assert!(
        fails(&out, 1)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no screen named nope")
    );

    // The guide and the types.
    let out = run(&["skill", "show", "screens"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("name: kibann-screens"));
    let out = run(&["screen", "api"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("interface Kibann"));

    std::fs::remove_dir_all(&root).unwrap();
}
