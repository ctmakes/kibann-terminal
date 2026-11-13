//! Git context for panes, found by running `git` itself (so every git
//! feature, config and hook behaves as the user's own git does):
//!
//! - [`Checkout::discover`]: which worktree a directory is in;
//! - [`Checkout::info`]: branch, changed files, ahead/behind, operation.
//!
//! Git never takes optional locks here (`--no-optional-locks`): agents run
//! git all the time, and a status that refreshed the index could make their
//! commits fail on `index.lock`. It also never writes, so our own runs
//! don't show up as changes.

mod diff;
mod filter;
mod status;
mod watch;

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use proto::git::{GitInfo, GitOp};

pub use diff::Limits;
pub use filter::{Filter, global_excludes};
pub use status::{Status, parse_status};
pub use watch::Watcher;

/// A `git` command that leaves the repository alone and can't prompt.
pub fn git(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("--no-optional-locks")
        .arg("-C")
        .arg(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        // The directory decides the repository, not the daemon's env.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

fn output(mut cmd: Command) -> io::Result<Option<Vec<u8>>> {
    let out = cmd.output()?;
    Ok(out.status.success().then_some(out.stdout))
}

/// A git worktree (the main one or a linked one).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Checkout {
    /// Its top directory.
    pub root: PathBuf,
    /// Its own git dir (`.git`, or `.git/worktrees/NAME` when linked).
    pub git_dir: PathBuf,
    /// The git dir shared by all its worktrees (refs, objects).
    pub common_dir: PathBuf,
}

impl Checkout {
    /// The worktree `dir` is in. `None` outside one, inside a `.git`
    /// directory or a bare repository, or when git isn't installed.
    pub fn discover(dir: &Path) -> Option<Checkout> {
        let mut cmd = git(dir);
        cmd.args([
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ]);
        let out = output(cmd).ok()??;
        let text = String::from_utf8(out).ok()?;
        let mut lines = text.lines().map(PathBuf::from);
        let (root, git_dir, common_dir) = (lines.next()?, lines.next()?, lines.next()?);
        Some(Checkout {
            root,
            git_dir,
            common_dir,
        })
    }

    pub fn is_linked(&self) -> bool {
        self.git_dir != self.common_dir
    }

    /// The repository's name: the main worktree's folder (the common git
    /// dir's parent, when it is a `.git` folder).
    pub fn repo_name(&self) -> String {
        let main = match self.common_dir.file_name() {
            Some(n) if n == ".git" => self.common_dir.parent(),
            _ => None,
        };
        name_of(main.unwrap_or(&self.root))
    }

    /// The operation stopped in the middle, from files in the git dir.
    pub fn operation(&self) -> Option<GitOp> {
        let has = |p: &str| self.git_dir.join(p).exists();
        if has("rebase-merge") {
            Some(GitOp::Rebase)
        } else if has("rebase-apply") {
            Some(if has("rebase-apply/applying") {
                GitOp::Apply
            } else {
                GitOp::Rebase
            })
        } else if has("MERGE_HEAD") {
            Some(GitOp::Merge)
        } else if has("CHERRY_PICK_HEAD") {
            Some(GitOp::CherryPick)
        } else if has("REVERT_HEAD") {
            Some(GitOp::Revert)
        } else if has("BISECT_LOG") {
            Some(GitOp::Bisect)
        } else {
            None
        }
    }

    /// Run `git status` and put everything together.
    pub fn info(&self) -> io::Result<GitInfo> {
        let out = output(status_command(&self.root))?
            .ok_or_else(|| io::Error::other("git status failed"))?;
        Ok(self.info_from(&parse_status(&out)))
    }

    pub fn info_from(&self, s: &Status) -> GitInfo {
        GitInfo {
            root: self.root.to_string_lossy().into_owned(),
            repo: self.repo_name(),
            worktree: self.is_linked().then(|| name_of(&self.root)),
            detached: s.detached.then(|| s.oid.chars().take(7).collect()),
            branch: if s.detached { None } else { s.head.clone() },
            upstream: s.upstream.clone(),
            ahead: s.ahead,
            behind: s.behind,
            changed: s.changed,
            conflicts: s.conflicts,
            operation: self.operation(),
        }
    }
}

/// `git status` in the form [`parse_status`] reads.
pub fn status_command(root: &Path) -> Command {
    let mut cmd = git(root);
    cmd.args([
        "status",
        "--porcelain=v2",
        "--branch",
        "-z",
        "--untracked-files=normal",
    ]);
    cmd
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}
