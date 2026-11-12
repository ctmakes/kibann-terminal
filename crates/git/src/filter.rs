//! Which filesystem events can change what `git status` says.

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::Checkout;

/// Files in a git dir whose change matters: the branch, the index, refs,
/// and the markers of an operation in progress. Not objects, logs or locks.
fn git_file_matters(rel: &Path) -> bool {
    let s = rel.to_string_lossy();
    if s.ends_with(".lock") {
        return false;
    }
    let first = rel
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy());
    matches!(
        first.as_deref(),
        Some(
            "HEAD"
                | "index"
                | "packed-refs"
                | "refs"
                | "MERGE_HEAD"
                | "CHERRY_PICK_HEAD"
                | "REVERT_HEAD"
                | "BISECT_LOG"
                | "rebase-merge"
                | "rebase-apply"
                | "info"
        )
    ) || (rel.components().count() == 1 && s == "config")
}

/// Decides whether a changed path matters to a checkout.
pub struct Filter {
    checkout: Checkout,
    ignore: Gitignore,
}

impl Filter {
    pub fn new(checkout: &Checkout, global_excludes: Option<&Path>) -> Filter {
        let mut b = GitignoreBuilder::new(&checkout.root);
        // Errors (a missing file, a bad line) leave the rest in place.
        let _ = b.add(checkout.root.join(".gitignore"));
        let _ = b.add(checkout.common_dir.join("info/exclude"));
        if let Some(g) = global_excludes {
            let _ = b.add(g);
        }
        Filter {
            checkout: checkout.clone(),
            ignore: b.build().unwrap_or_else(|_| Gitignore::empty()),
        }
    }

    /// Whether a change to `path` could change the checkout's status.
    /// `None`: the path isn't this checkout's at all.
    pub fn matters(&self, path: &Path) -> Option<bool> {
        let c = &self.checkout;
        // Most specific first: a linked worktree's git dir is inside the
        // common dir, and a main worktree's git dir inside its root.
        if let Ok(rel) = path.strip_prefix(&c.git_dir) {
            return Some(git_file_matters(rel));
        }
        if let Ok(rel) = path.strip_prefix(&c.common_dir) {
            // Shared refs; another worktree's own files don't matter.
            return Some(!rel.starts_with("worktrees") && git_file_matters(rel));
        }
        let rel = path.strip_prefix(&c.root).ok()?;
        if rel.components().any(|p| p.as_os_str() == ".git") {
            // Another checkout's git dir inside ours (a submodule), or our
            // own seen through a different path.
            return Some(false);
        }
        if rel.as_os_str().is_empty() {
            return Some(true);
        }
        let is_dir = path.is_dir();
        Some(
            !self
                .ignore
                .matched_path_or_any_parents(path, is_dir)
                .is_ignore(),
        )
    }

    /// Whether `path` is an ignore file the filter was built from.
    pub fn is_ignore_file(&self, path: &Path) -> bool {
        path == self.checkout.root.join(".gitignore")
            || path == self.checkout.common_dir.join("info/exclude")
    }
}

/// The user's global excludes file (`core.excludesFile`, else
/// `$XDG_CONFIG_HOME/git/ignore` or `~/.config/git/ignore`).
pub fn global_excludes(dir: &Path) -> Option<PathBuf> {
    let mut cmd = crate::git(dir);
    cmd.args(["config", "--path", "core.excludesFile"]);
    if let Ok(Some(out)) = crate::output(cmd) {
        let p = String::from_utf8_lossy(&out).trim().to_owned();
        if !p.is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("git/ignore"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkout(linked: bool) -> Checkout {
        if linked {
            Checkout {
                root: "/w/fix".into(),
                git_dir: "/r/proj/.git/worktrees/fix".into(),
                common_dir: "/r/proj/.git".into(),
            }
        } else {
            Checkout {
                root: "/r/proj".into(),
                git_dir: "/r/proj/.git".into(),
                common_dir: "/r/proj/.git".into(),
            }
        }
    }

    #[test]
    fn git_dir_files() {
        let f = Filter::new(&checkout(false), None);
        let m = |p: &str| f.matters(Path::new(p));
        assert_eq!(m("/r/proj/.git/HEAD"), Some(true));
        assert_eq!(m("/r/proj/.git/index"), Some(true));
        assert_eq!(m("/r/proj/.git/refs/heads/main"), Some(true));
        assert_eq!(m("/r/proj/.git/rebase-merge/done"), Some(true));
        assert_eq!(m("/r/proj/.git/index.lock"), Some(false));
        assert_eq!(m("/r/proj/.git/objects/ab/cdef"), Some(false));
        assert_eq!(m("/r/proj/.git/logs/HEAD"), Some(false));
        assert_eq!(m("/r/proj/.git/FETCH_HEAD"), Some(false));
        assert_eq!(m("/r/proj/src/main.rs"), Some(true));
        assert_eq!(m("/r/proj/vendor/sub/.git/HEAD"), Some(false));
        assert_eq!(m("/elsewhere/x"), None);
    }

    #[test]
    fn a_linked_worktree_sees_its_own_head_and_shared_refs() {
        let f = Filter::new(&checkout(true), None);
        let m = |p: &str| f.matters(Path::new(p));
        assert_eq!(m("/r/proj/.git/worktrees/fix/HEAD"), Some(true));
        assert_eq!(m("/r/proj/.git/worktrees/other/HEAD"), Some(false));
        assert_eq!(m("/r/proj/.git/refs/remotes/origin/main"), Some(true));
        assert_eq!(m("/r/proj/.git/HEAD"), Some(true));
        assert_eq!(m("/w/fix/a.rs"), Some(true));
        assert_eq!(m("/r/proj/a.rs"), None);
    }
}
