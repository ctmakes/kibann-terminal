//! Discovery and status against real repositories in temp directories.

mod common;

use common::{repo, run, tmp, write};
use git::Checkout;
use proto::git::GitOp;

#[test]
fn outside_a_repo_or_inside_dot_git_is_none() {
    let d = tmp();
    assert_eq!(Checkout::discover(&d), None);
    let r = repo();
    assert_eq!(Checkout::discover(&r.join(".git")), None);
    assert_eq!(Checkout::discover(&r.join("missing")), None);
}

#[test]
fn branch_and_changed_files() {
    let r = repo();
    std::fs::create_dir(r.join("sub")).unwrap();
    let c = Checkout::discover(&r.join("sub")).unwrap();
    assert_eq!(c.root, r);
    assert!(!c.is_linked());
    let i = c.info().unwrap();
    assert_eq!(i.repo, "proj");
    assert_eq!(i.branch.as_deref(), Some("main"));
    assert_eq!((i.changed, i.worktree.clone()), (0, None));

    write(&r.join("a.txt"), "two\n"); // unstaged
    write(&r.join("b.txt"), "new\n"); // untracked
    write(&r.join("ignored.log"), "x\n");
    write(&r.join(".gitignore"), "*.log\n"); // untracked too
    assert_eq!(c.info().unwrap().changed, 3);
    run(&r, &["add", "a.txt"]);
    write(&r.join("a.txt"), "three\n"); // staged and unstaged: still one
    assert_eq!(c.info().unwrap().changed, 3);

    run(&r, &["add", "."]);
    run(&r, &["commit", "-qm", "second"]);
    assert_eq!(c.info().unwrap().changed, 0);
    run(&r, &["checkout", "-qb", "feature/x"]);
    assert_eq!(c.info().unwrap().branch.as_deref(), Some("feature/x"));
    run(&r, &["checkout", "-q", "--detach", "HEAD~1"]);
    let i = c.info().unwrap();
    assert_eq!(i.branch, None);
    assert_eq!(i.detached.map(|d| d.len()), Some(7));
}

#[test]
fn ahead_and_behind_an_upstream() {
    let origin = repo();
    let clone = origin.parent().unwrap().join("clone");
    run(origin.parent().unwrap(), &["clone", "-q", "proj", "clone"]);
    write(&origin.join("a.txt"), "upstream\n");
    run(&origin, &["commit", "-qam", "upstream"]);
    write(&clone.join("c.txt"), "mine\n");
    run(&clone, &["add", "."]);
    run(&clone, &["commit", "-qm", "mine"]);
    run(&clone, &["fetch", "-q"]);
    let i = Checkout::discover(&clone).unwrap().info().unwrap();
    assert_eq!(i.upstream.as_deref(), Some("origin/main"));
    assert_eq!((i.ahead, i.behind), (1, 1));
    assert_eq!(i.repo, "clone");
}

#[test]
fn a_linked_worktree() {
    let r = repo();
    let wt = r.parent().unwrap().join("fix-links");
    run(
        &r,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "fix/links",
            wt.to_str().unwrap(),
        ],
    );
    let c = Checkout::discover(&wt).unwrap();
    assert!(c.is_linked());
    let i = c.info().unwrap();
    assert_eq!(i.repo, "proj");
    assert_eq!(i.worktree.as_deref(), Some("fix-links"));
    assert_eq!(i.branch.as_deref(), Some("fix/links"));
}

#[test]
fn a_rebase_stopped_on_a_conflict() {
    let r = repo();
    run(&r, &["checkout", "-qb", "other"]);
    write(&r.join("a.txt"), "theirs\n");
    run(&r, &["commit", "-qam", "theirs"]);
    run(&r, &["checkout", "-q", "main"]);
    write(&r.join("a.txt"), "ours\n");
    run(&r, &["commit", "-qam", "ours"]);
    run(&r, &["rebase", "other"]);
    let i = Checkout::discover(&r).unwrap().info().unwrap();
    assert_eq!(i.operation, Some(GitOp::Rebase));
    assert_eq!((i.conflicts, i.changed), (1, 1));
    run(&r, &["rebase", "--abort"]);
    run(&r, &["merge", "other"]);
    let i = Checkout::discover(&r).unwrap().info().unwrap();
    assert_eq!(i.operation, Some(GitOp::Merge));
}

#[test]
fn status_never_takes_the_index_lock() {
    let r = repo();
    // Make the index stale so a plain `git status` would want to refresh it.
    write(&r.join("a.txt"), "one\n");
    let before = std::fs::metadata(r.join(".git/index"))
        .unwrap()
        .modified()
        .unwrap();
    // Another git holding the lock (an agent mid-commit) doesn't stop us.
    write(&r.join(".git/index.lock"), "");
    let c = Checkout::discover(&r).unwrap();
    assert_eq!(c.info().unwrap().changed, 0);
    std::fs::remove_file(r.join(".git/index.lock")).unwrap();
    let after = std::fs::metadata(r.join(".git/index"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after, "status rewrote the index");
}
