//! `git status --porcelain=v2 --branch -z`, parsed.

/// What one status run says.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Status {
    /// HEAD's commit (`(initial)` before the first commit).
    pub oid: String,
    /// The branch, unless detached.
    pub head: Option<String>,
    pub detached: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changed: u32,
    pub conflicts: u32,
}

/// Records are NUL-terminated. Headers start with `# `; entries with `1`
/// (changed), `2` (renamed or copied: the original path follows as its own
/// record), `u` (unmerged), `?` (untracked) or `!` (ignored, not counted).
pub fn parse_status(out: &[u8]) -> Status {
    let mut s = Status::default();
    let mut records = out.split(|&b| b == 0).filter(|r| !r.is_empty());
    while let Some(r) = records.next() {
        let line = String::from_utf8_lossy(r);
        if let Some(h) = line.strip_prefix("# ") {
            let (key, value) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" => s.oid = value.to_owned(),
                "branch.head" if value == "(detached)" => s.detached = true,
                "branch.head" => s.head = Some(value.to_owned()),
                "branch.upstream" => s.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    for part in value.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            s.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            s.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        match r[0] {
            b'1' | b'?' => s.changed += 1,
            b'2' => {
                s.changed += 1;
                records.next(); // the original path
            }
            b'u' => {
                s.changed += 1;
                s.conflicts += 1;
            }
            _ => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(records: &[&str]) -> Vec<u8> {
        let mut v = Vec::new();
        for r in records {
            v.extend_from_slice(r.as_bytes());
            v.push(0);
        }
        v
    }

    #[test]
    fn clean_on_a_branch_with_upstream() {
        let s = parse_status(&z(&[
            "# branch.oid 9f12549abcdef0123456789abcdef0123456789",
            "# branch.head feat/term",
            "# branch.upstream origin/feat/term",
            "# branch.ab +2 -1",
        ]));
        assert_eq!(s.head.as_deref(), Some("feat/term"));
        assert_eq!(s.upstream.as_deref(), Some("origin/feat/term"));
        assert_eq!((s.ahead, s.behind, s.changed), (2, 1, 0));
        assert!(!s.detached);
    }

    #[test]
    fn every_kind_of_change_counts_once() {
        let s = parse_status(&z(&[
            "# branch.oid (initial)",
            "# branch.head main",
            "1 M. N... 100644 100644 100644 aaaa bbbb staged.rs",
            "1 .M N... 100644 100644 100644 aaaa aaaa unstaged.rs",
            "1 MM N... 100644 100644 100644 aaaa bbbb both.rs",
            "2 R. N... 100644 100644 100644 aaaa aaaa R100 new name.rs",
            "old name.rs",
            "u UU N... 100644 100644 100644 100644 a b c conflict.rs",
            "? untracked\nwith newline.txt",
            "! ignored.log",
        ]));
        assert_eq!(s.oid, "(initial)");
        assert_eq!(s.upstream, None);
        assert_eq!((s.changed, s.conflicts), (6, 1));
    }

    #[test]
    fn detached() {
        let s = parse_status(&z(&["# branch.oid 1234567890", "# branch.head (detached)"]));
        assert!(s.detached);
        assert_eq!(s.head, None);
    }
}
