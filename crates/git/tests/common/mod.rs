//! Temp repositories for the tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

/// A fresh empty directory.
pub fn tmp() -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "kg{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Resolve /tmp → /private/tmp, as git reports it.
    d.canonicalize().unwrap()
}

/// Run git in `dir` with no user or system config (hooks, signing).
pub fn run(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
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
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

pub fn write(p: &Path, text: &str) {
    std::fs::write(p, text).unwrap();
}

/// A repo with one commit of `a.txt`.
pub fn repo() -> PathBuf {
    let d = tmp().join("proj");
    std::fs::create_dir(&d).unwrap();
    run(&d, &["init", "-q"]);
    write(&d.join("a.txt"), "one\n");
    run(&d, &["add", "."]);
    run(&d, &["commit", "-qm", "first"]);
    d
}
