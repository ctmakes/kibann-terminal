//! Everything uncommitted in a checkout, as files with their patches: what
//! the diff view shows.
//!
//! Git runs with `--no-ext-diff --no-textconv --no-color` so the user's
//! config can't run external programs or add escapes, and diffs against the
//! empty tree before the first commit.

use std::io;
use std::path::Path;

use proto::git::{Diff, FileDiff};

use crate::{Checkout, git, output};

/// Cuts, so a generated file or a huge change can't flood the viewer.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub per_file: usize,
    pub total: usize,
    /// Untracked files bigger than this are listed without content.
    pub untracked: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            per_file: 512 << 10,
            total: 4 << 20,
            untracked: 256 << 10,
        }
    }
}

const DIFF_ARGS: &[&str] = &[
    "diff",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-relative",
    "-M",
];

fn run(root: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut cmd = git(root);
    cmd.args(args);
    output(cmd)?.ok_or_else(|| io::Error::other(format!("git {} failed", args[0])))
}

/// HEAD, or the empty tree before the first commit.
fn base(root: &Path) -> io::Result<String> {
    if run(root, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok() {
        return Ok("HEAD".into());
    }
    let mut cmd = git(root);
    cmd.args(["hash-object", "-t", "tree", "--stdin"])
        .stdin(std::process::Stdio::null());
    let out = output(cmd)?.ok_or_else(|| io::Error::other("no empty tree"))?;
    Ok(String::from_utf8_lossy(&out).trim().to_owned())
}

/// Lines added and removed (none for binary files), path, old path.
type Numstat = (Option<u32>, Option<u32>, String, Option<String>);

/// `--numstat -z` records: `A\tR\tpath` or, for renames, `A\tR\t` then the
/// old and new paths as their own records.
fn numstat(out: &[u8]) -> Vec<Numstat> {
    let mut v = Vec::new();
    let mut recs = out.split(|&b| b == 0);
    while let Some(r) = recs.next() {
        if r.is_empty() {
            continue;
        }
        let line = String::from_utf8_lossy(r);
        let mut parts = line.splitn(3, '\t');
        let (a, d, p) = (parts.next(), parts.next(), parts.next().unwrap_or(""));
        let n = |s: Option<&str>| s.and_then(|s| s.parse().ok());
        if p.is_empty() {
            let old = recs.next().map(|r| String::from_utf8_lossy(r).into_owned());
            let new = recs.next().map(|r| String::from_utf8_lossy(r).into_owned());
            v.push((n(a), n(d), new.unwrap_or_default(), old));
        } else {
            v.push((n(a), n(d), p.to_owned(), None));
        }
    }
    v
}

/// `--name-status -z` records: the letter (with a score for renames), then
/// one path, or two for renames and copies.
fn name_status(out: &[u8]) -> Vec<String> {
    let mut v = Vec::new();
    let mut recs = out.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(r) = recs.next() {
        let letter = String::from_utf8_lossy(&r[..1]).into_owned();
        recs.next();
        if letter == "R" || letter == "C" {
            recs.next();
        }
        v.push(letter);
    }
    v
}

/// The patch split at each `diff --git` line, in git's file order.
fn split_patch(text: &str) -> Vec<&str> {
    let mut starts: Vec<usize> = text
        .match_indices("diff --git ")
        .filter(|(i, _)| *i == 0 || text.as_bytes()[i - 1] == b'\n')
        .map(|(i, _)| i)
        .collect();
    starts.push(text.len());
    starts.windows(2).map(|w| &text[w[0]..w[1]]).collect()
}

/// Cut `s` to at most `max` bytes, at a line end where possible.
fn cut(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_owned(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let end = s[..end].rfind('\n').map_or(end, |i| i + 1);
    (s[..end].to_owned(), true)
}

/// An untracked file as an all-added patch, if it's small text.
fn untracked_patch(root: &Path, path: &str, limit: u64) -> (Option<u32>, bool, Option<String>) {
    let full = root.join(path);
    let Ok(meta) = std::fs::metadata(&full) else {
        return (None, false, None);
    };
    if meta.len() > limit {
        return (None, false, None);
    }
    let Ok(bytes) = std::fs::read(&full) else {
        return (None, false, None);
    };
    if bytes.contains(&0) {
        return (None, true, None);
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let mut patch = format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for l in &lines {
        patch.push('+');
        patch.push_str(l);
        patch.push('\n');
    }
    (Some(lines.len() as u32), false, Some(patch))
}

impl Checkout {
    /// Everything uncommitted: tracked changes against HEAD, then untracked
    /// files.
    pub fn diff(&self, limits: Limits) -> io::Result<Diff> {
        let root = &self.root;
        let base = base(root)?;
        let with = |extra: &[&str]| {
            let mut args: Vec<&str> = DIFF_ARGS.to_vec();
            args.extend_from_slice(extra);
            args.push(&base);
            run(root, &args)
        };
        let stats = numstat(&with(&["--numstat", "-z"])?);
        let letters = name_status(&with(&["--name-status", "-z"])?);
        let patch = String::from_utf8_lossy(&with(&[])?).into_owned();
        let patches = split_patch(&patch);

        let mut diff = Diff::default();
        let mut used = 0usize;
        for (i, (added, removed, path, old_path)) in stats.into_iter().enumerate() {
            let binary = added.is_none() && removed.is_none();
            let (patch, truncated) = match patches.get(i) {
                Some(p) if !binary && used < limits.total => {
                    let (p, cut_) = cut(p, limits.per_file.min(limits.total - used));
                    used += p.len();
                    (Some(p), cut_)
                }
                Some(_) if !binary => {
                    diff.truncated = true;
                    (None, false)
                }
                _ => (None, false),
            };
            diff.files.push(FileDiff {
                status: letters.get(i).cloned().unwrap_or_else(|| "M".into()),
                path,
                old_path,
                added,
                removed,
                binary,
                patch,
                truncated,
            });
        }

        let others = run(root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        for p in others.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let path = String::from_utf8_lossy(p).into_owned();
            let (added, binary, patch) = if used < limits.total {
                untracked_patch(root, &path, limits.untracked)
            } else {
                diff.truncated = true;
                (None, false, None)
            };
            let (patch, truncated) = match patch {
                Some(p) => {
                    let (p, c) = cut(&p, limits.per_file.min(limits.total - used));
                    used += p.len();
                    (Some(p), c)
                }
                None => (None, false),
            };
            diff.files.push(FileDiff {
                path,
                old_path: None,
                status: "?".into(),
                added,
                removed: added.map(|_| 0),
                binary,
                patch,
                truncated,
            });
        }
        Ok(diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_records() {
        let out = b"3\t1\tsrc/a.rs\0-\t-\timg.png\x000\t0\t\0old name.rs\0new name.rs\0";
        let v = numstat(out);
        assert_eq!(v[0], (Some(3), Some(1), "src/a.rs".into(), None));
        assert_eq!(v[1], (None, None, "img.png".into(), None));
        assert_eq!(
            v[2],
            (
                Some(0),
                Some(0),
                "new name.rs".into(),
                Some("old name.rs".into())
            )
        );
    }

    #[test]
    fn name_status_records() {
        let out = b"M\0a.rs\0R100\0old\0new\0D\0gone\0";
        assert_eq!(name_status(out), vec!["M", "R", "D"]);
    }

    #[test]
    fn patches_split_per_file() {
        let p = "diff --git a/x b/x\n+one\ndiff --git a/y b/y\n+ diff --git inside\n";
        let v = split_patch(p);
        assert_eq!(v.len(), 2);
        assert!(v[1].contains("inside"));
    }

    #[test]
    fn cutting_at_a_line() {
        assert_eq!(cut("ab\ncd\nef\n", 7), ("ab\ncd\n".into(), true));
        assert_eq!(cut("short", 10), ("short".into(), false));
        assert_eq!(cut("éé", 3).0, "é");
    }
}
