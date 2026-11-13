//! The diff view's data, from real repositories.

mod common;

use common::{repo, run, tmp, write};
use git::{Checkout, Limits};

fn file<'a>(d: &'a proto::git::Diff, path: &str) -> &'a proto::git::FileDiff {
    d.files
        .iter()
        .find(|f| f.path == path)
        .unwrap_or_else(|| panic!("no {path}: {d:#?}"))
}

#[test]
fn every_kind_of_change() {
    let r = repo(); // a.txt committed
    write(&r.join("gone.txt"), "bye\n");
    write(
        &r.join("old.txt"),
        "same content here\nline two\nline three\n",
    );
    run(&r, &["add", "."]);
    run(&r, &["commit", "-qm", "more"]);

    write(&r.join("a.txt"), "one\ntwo\n"); // modified, unstaged
    write(&r.join("new.txt"), "fresh\n");
    run(&r, &["add", "new.txt"]); // added, staged
    std::fs::remove_file(r.join("gone.txt")).unwrap();
    run(&r, &["mv", "old.txt", "renamed.txt"]);
    write(&r.join("untracked.md"), "hello\nworld\n");
    std::fs::write(r.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();

    let d = Checkout::discover(&r)
        .unwrap()
        .diff(Limits::default())
        .unwrap();
    let a = file(&d, "a.txt");
    assert_eq!(
        (a.status.as_str(), a.added, a.removed),
        ("M", Some(1), Some(0))
    );
    assert!(a.patch.as_deref().unwrap().contains("+two"));
    assert_eq!(file(&d, "new.txt").status, "A");
    assert_eq!(file(&d, "gone.txt").status, "D");
    let mv = file(&d, "renamed.txt");
    assert_eq!(
        (mv.status.as_str(), mv.old_path.as_deref()),
        ("R", Some("old.txt"))
    );
    let u = file(&d, "untracked.md");
    assert_eq!((u.status.as_str(), u.added), ("?", Some(2)));
    assert!(u.patch.as_deref().unwrap().ends_with("+hello\n+world\n"));
    let b = file(&d, "blob.bin");
    assert!(b.binary && b.patch.is_none());
    assert!(!d.truncated);
}

#[test]
fn before_the_first_commit() {
    let d = tmp().join("fresh");
    std::fs::create_dir(&d).unwrap();
    run(&d, &["init", "-q"]);
    write(&d.join("x.txt"), "x\n");
    run(&d, &["add", "."]);
    let diff = Checkout::discover(&d)
        .unwrap()
        .diff(Limits::default())
        .unwrap();
    assert_eq!(file(&diff, "x.txt").status, "A");
}

#[test]
fn big_changes_are_cut() {
    let r = repo();
    write(&r.join("b.txt"), "b\n");
    run(&r, &["add", "."]);
    run(&r, &["commit", "-qm", "b"]);
    let big: String = (0..20_000).map(|i| format!("line {i}\n")).collect();
    write(&r.join("a.txt"), &big);
    write(&r.join("b.txt"), &big);
    write(&r.join("c.txt"), &big); // untracked and over its limit: no body
    let limits = Limits {
        per_file: 10_000,
        total: 15_000,
        untracked: 1_000,
    };
    let d = Checkout::discover(&r).unwrap().diff(limits).unwrap();
    let a = file(&d, "a.txt");
    assert!(a.truncated && a.patch.as_ref().unwrap().len() <= 10_000);
    let b = file(&d, "b.txt");
    let left = 15_000 - a.patch.as_ref().unwrap().len();
    assert!(b.patch.as_ref().unwrap().len() <= left);
    assert!(b.truncated, "b gets what the total leaves");
    let c = file(&d, "c.txt");
    assert!(c.patch.is_none() && !c.binary);
}

#[test]
fn user_config_cannot_run_programs() {
    let r = repo();
    let marker = r.join("ran");
    run(
        &r,
        &[
            "config",
            "diff.external",
            &format!("touch {}", marker.display()),
        ],
    );
    write(&r.join("a.txt"), "changed\n");
    let d = Checkout::discover(&r)
        .unwrap()
        .diff(Limits::default())
        .unwrap();
    assert!(file(&d, "a.txt").patch.is_some());
    assert!(!marker.exists(), "diff.external ran");
}
