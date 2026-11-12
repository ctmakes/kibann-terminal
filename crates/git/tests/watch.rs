//! The watcher against real repositories: changes arrive, ignored files and
//! idle time cost nothing.

mod common;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::{Duration, Instant};

use common::{repo, run, write};
use git::{Checkout, Watcher};
use proto::git::GitInfo;

fn watcher() -> (Watcher, Receiver<(PathBuf, Option<GitInfo>)>) {
    let (tx, rx) = channel();
    let w = Watcher::new(move |root, info| {
        let _ = tx.send((root.to_path_buf(), info.cloned()));
    });
    (w, rx)
}

/// The next change for which `f` holds.
fn until(rx: &Receiver<(PathBuf, Option<GitInfo>)>, what: &str, f: impl Fn(&GitInfo) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok((_, Some(i))) if f(&i) => return,
            Ok(_) => {}
            Err(_) => panic!("never: {what}"),
        }
    }
}

#[test]
fn changes_arrive_from_events() {
    let r = repo();
    let (w, rx) = watcher();
    let c = Checkout::discover(&r).unwrap();
    assert_eq!(w.add(&c), None);
    until(&rx, "first run", |i| {
        i.changed == 0 && i.branch.as_deref() == Some("main")
    });

    write(&r.join("a.txt"), "changed\n");
    until(&rx, "edit seen", |i| i.changed == 1);
    run(&r, &["commit", "-qam", "edit"]);
    until(&rx, "commit seen", |i| i.changed == 0);
    run(&r, &["checkout", "-qb", "topic"]);
    until(&rx, "branch seen", |i| i.branch.as_deref() == Some("topic"));
    assert_eq!(w.info(&c.root).unwrap().branch.as_deref(), Some("topic"));
}

#[test]
fn panes_in_one_checkout_share_it() {
    let r = repo();
    std::fs::create_dir(r.join("sub")).unwrap();
    let (w, rx) = watcher();
    let a = Checkout::discover(&r).unwrap();
    let b = Checkout::discover(&r.join("sub")).unwrap();
    w.add(&a);
    until(&rx, "first run", |_| true);
    assert_eq!(w.add(&b).map(|i| i.changed), Some(0), "known already");
    assert_eq!(w.watching(), 1);
    assert_eq!(w.runs(), 1);
    w.remove(&a.root);
    assert_eq!(w.watching(), 1);
    w.remove(&b.root);
    assert_eq!(w.watching(), 0);
}

#[test]
fn ignored_files_and_idle_time_cost_nothing() {
    let r = repo();
    write(&r.join(".gitignore"), "target/\n");
    run(&r, &["add", "."]);
    run(&r, &["commit", "-qm", "ignore"]);
    std::fs::create_dir(r.join("target")).unwrap();
    let (w, rx) = watcher();
    w.add(&Checkout::discover(&r).unwrap());
    until(&rx, "first run", |_| true);
    // Let events from setting up the repo drain.
    thread::sleep(Duration::from_millis(1500));
    let before = w.runs();

    for i in 0..1000 {
        write(&r.join(format!("target/out{i}.o")), "x");
    }
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(w.runs(), before, "ignored writes started a run");

    // And a burst of real changes is one or two runs, not hundreds.
    for i in 0..200 {
        write(&r.join(format!("src{i}.rs")), "x");
    }
    until(&rx, "burst seen", |i| i.changed == 200);
    thread::sleep(Duration::from_millis(1500));
    assert!(w.runs() - before <= 3, "{} runs", w.runs() - before);
}

#[test]
fn a_refresh_runs_without_events() {
    let r = repo();
    let (w, rx) = watcher();
    let c = Checkout::discover(&r).unwrap();
    w.add(&c);
    until(&rx, "first run", |_| true);
    let before = w.runs();
    w.refresh(&c.root);
    let deadline = Instant::now() + Duration::from_secs(3);
    while w.runs() == before {
        assert!(Instant::now() < deadline, "refresh never ran");
        thread::sleep(Duration::from_millis(20));
    }
}
