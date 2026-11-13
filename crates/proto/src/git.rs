//! Where a pane is: its directory and, inside a git checkout, the repo,
//! worktree, branch, changed files and ahead/behind.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A merge, rebase or the like, stopped in the middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum GitOp {
    Merge,
    Rebase,
    /// `git am`.
    Apply,
    CherryPick,
    Revert,
    Bisect,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct GitInfo {
    /// The worktree's top directory.
    pub root: String,
    /// The repository's name: its main worktree's folder.
    pub repo: String,
    /// A linked worktree's name (its folder); `None` in the main worktree.
    pub worktree: Option<String>,
    /// The branch checked out; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// HEAD's short commit when detached.
    pub detached: Option<String>,
    /// The upstream branch (`origin/main`), if one is set.
    pub upstream: Option<String>,
    /// Commits ahead of and behind the upstream (as of the last fetch).
    pub ahead: u32,
    pub behind: u32,
    /// Files that differ from HEAD: staged, unstaged or untracked, each
    /// counted once.
    pub changed: u32,
    /// Files with merge conflicts (also counted in `changed`).
    pub conflicts: u32,
    pub operation: Option<GitOp>,
}

/// A pane's directory, and its checkout when it is in one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PaneDir {
    pub dir: String,
    /// `dir` as shown: under the home directory, starting with `~`.
    pub shown: String,
    pub git: Option<GitInfo>,
}

/// One file's uncommitted change, against HEAD.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct FileDiff {
    pub path: String,
    /// The path before a rename or copy.
    pub old_path: Option<String>,
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `C` copied,
    /// `T` type changed, `?` untracked.
    pub status: String,
    /// Lines added and removed; `None` for binary files.
    pub added: Option<u32>,
    pub removed: Option<u32>,
    pub binary: bool,
    /// The unified diff, without color; `None` for binary or skipped files.
    pub patch: Option<String>,
    /// The patch was cut at the size limit.
    pub truncated: bool,
}

/// Everything uncommitted in a checkout.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Diff {
    pub files: Vec<FileDiff>,
    /// Some files' patches were left out to stay under the total limit.
    pub truncated: bool,
}
