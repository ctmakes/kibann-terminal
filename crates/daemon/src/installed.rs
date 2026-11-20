//! Which programs are installed, for a layout's `command = ["btop", "htop",
//! "top"]`: the first one installed runs.
//!
//! "Installed" means on the PATH the pane's shell will have. That isn't the
//! daemon's: an app started from Finder gets a bare PATH, without Homebrew
//! or `~/.cargo/bin`. So what the daemon's PATH doesn't find, the user's
//! login shell is asked about, once a minute at most.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long an answer is kept: a program installed meanwhile is noticed.
const KEEP: Duration = Duration::from_secs(60);
/// How long the login shell may take to answer.
const SHELL_WAIT: Duration = Duration::from_secs(5);

/// The program a command line runs: its first word, after any `NAME=value`.
pub fn program_of(command: &str) -> Option<&str> {
    command
        .split_whitespace()
        .find(|w| !(w.contains('=') && !w.starts_with('=')))
}

/// Which of `names` are installed.
pub fn check(names: &[&str]) -> HashSet<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, (bool, Instant)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let now = Instant::now();
    let mut found = HashSet::new();
    let mut ask = Vec::new();
    {
        let c = cache.lock().unwrap_or_else(|e| e.into_inner());
        for n in names {
            match c.get(*n) {
                Some((yes, at)) if now.duration_since(*at) < KEEP => {
                    if *yes {
                        found.insert((*n).to_owned());
                    }
                }
                _ => ask.push(*n),
            }
        }
    }
    if ask.is_empty() {
        return found;
    }
    let mut answers: HashMap<String, bool> = HashMap::new();
    let mut rest = Vec::new();
    for n in ask {
        if n.contains('/') || n.contains('\\') {
            // A path: it's there or it isn't (relative ones can't be told
            // apart from here; let the shell say so).
            answers.insert(
                n.to_owned(),
                !Path::new(n).is_absolute() || Path::new(n).exists(),
            );
        } else if on_path(n, std::env::var_os("PATH").as_deref()) {
            answers.insert(n.to_owned(), true);
        } else {
            rest.push(n);
        }
    }
    if !rest.is_empty() {
        let shell_found = ask_shell(&rest);
        for n in rest {
            answers.insert(
                n.to_owned(),
                shell_found.as_ref().is_some_and(|s| s.contains(n)),
            );
        }
    }
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    for (n, yes) in answers {
        if yes {
            found.insert(n.clone());
        }
        c.insert(n, (yes, now));
    }
    found
}

/// Whether `name` is an executable in one of `path`'s directories.
fn on_path(name: &str, path: Option<&std::ffi::OsStr>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT;.COM".into())
            .split(';')
            .map(str::to_owned)
            .chain(std::iter::once(String::new()))
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(path).any(|dir| {
        exts.iter().any(|e| {
            let p = dir.join(format!("{name}{e}"));
            is_executable(&p)
        })
    })
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Names safe to put in a shell script as they are.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

/// Ask the user's login shell which of `names` it finds. `None` when it
/// couldn't be asked (or took too long).
#[cfg(unix)]
fn ask_shell(names: &[&str]) -> Option<HashSet<String>> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let names: Vec<&str> = names.iter().copied().filter(|n| plain(n)).collect();
    if names.is_empty() {
        return Some(HashSet::new());
    }
    const MARK: &str = "__kibann_installed__";
    let mut script = format!("printf '%s\\n' {MARK}");
    for n in &names {
        script.push_str(&format!(
            "; command -v {n} >/dev/null 2>&1 && printf '%s\\n' {n}"
        ));
    }
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    // Interactive and login, so both .zprofile and .zshrc (where PATH is
    // often set) are read.
    let mut child = Command::new(&shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = out.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    let text = rx.recv_timeout(SHELL_WAIT);
    let _ = child.kill();
    let _ = child.wait();
    let text = text.ok()?;
    let after = &text[text.find(MARK)? + MARK.len()..];
    Some(
        after
            .lines()
            .map(str::trim)
            .filter(|l| names.contains(l))
            .map(str::to_owned)
            .collect(),
    )
}

#[cfg(not(unix))]
fn ask_shell(_names: &[&str]) -> Option<HashSet<String>> {
    // Windows apps get the user's PATH from the system; nothing to add.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_of_command_lines() {
        assert_eq!(program_of("npm run dev"), Some("npm"));
        assert_eq!(program_of("  FOO=1 BAR=x cargo watch"), Some("cargo"));
        assert_eq!(program_of(""), None);
    }

    #[cfg(unix)]
    #[test]
    fn finds_what_is_installed() {
        let got = check(&["sh", "no-such-program-kibann-test"]);
        assert!(got.contains("sh"));
        assert!(!got.contains("no-such-program-kibann-test"));
    }
}
