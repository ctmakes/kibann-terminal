//! An update replaces the app, but the terminal service (the daemon) from
//! the earlier version keeps running so that its shells survive. Terminal
//! behaviour lives in the service, so the update's terminal changes only
//! take effect once the service restarts.
//!
//! When the app starts and finds an older service, it restarts it on its
//! own if every shell sits at its prompt: the tabs and splits come back
//! with fresh shells in the same directories. If a program is running in a
//! pane, the app asks first instead.
//!
//! The check must work with services from earlier versions, so it reads
//! the process table rather than asking the service.

use std::path::Path;

use daemon::client::Client;
use daemon::saved::Saved;
use proto::wire::{ClientMsg, Reply};

/// One row of `ps -A -o pid=,ppid=,pgid=,tpgid=,args=`.
#[derive(Debug, PartialEq)]
struct Proc<'a> {
    pid: i32,
    ppid: i32,
    pgid: i32,
    /// The foreground process group of its terminal (0 or less: none).
    tpgid: i32,
    args: &'a str,
}

fn parse_row(line: &str) -> Option<Proc<'_>> {
    let mut rest = line.trim_start();
    let mut num = || -> Option<i32> {
        let end = rest.find(char::is_whitespace)?;
        let n = rest[..end].parse().ok()?;
        rest = rest[end..].trim_start();
        Some(n)
    };
    let (pid, ppid, pgid, tpgid) = (num()?, num()?, num()?, num()?);
    Some(Proc {
        pid,
        ppid,
        pgid,
        tpgid,
        args: rest.trim_end(),
    })
}

/// A program's name from its command line: `/usr/bin/vim x` → `vim`.
fn name(args: &str) -> String {
    let first = args.split_whitespace().next().unwrap_or("");
    first.rsplit('/').next().unwrap_or(first).to_owned()
}

/// The programs running in the shells of the service on `socket`, from
/// `ps` output: for each shell not at its prompt, the program in front.
/// Empty when every shell is at its prompt; `None` when the service isn't
/// in the table, so it can't tell.
fn running_in(ps: &str, socket: &Path) -> Option<Vec<String>> {
    let procs: Vec<Proc> = ps.lines().filter_map(parse_row).collect();
    let tail = format!("--daemon --socket {}", socket.display());
    let service = procs.iter().find(|p| p.args.ends_with(&tail))?;
    let mut running = Vec::new();
    for shell in procs.iter().filter(|p| p.ppid == service.pid) {
        if shell.tpgid <= 0 || shell.tpgid == shell.pgid {
            continue;
        }
        let front = procs
            .iter()
            .find(|p| p.pid == shell.tpgid)
            .or_else(|| procs.iter().find(|p| p.pgid == shell.tpgid));
        running.push(front.map_or_else(|| "a program".to_owned(), |p| name(p.args)));
    }
    Some(running)
}

/// What runs in the service's shells now; see [`running_in`].
pub fn running_programs(socket: &Path) -> Option<Vec<String>> {
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,pgid=,tpgid=,args="])
        .output()
        .ok()?;
    running_in(&String::from_utf8_lossy(&out.stdout), socket)
}

/// Save the tabs, splits and each pane's directory, as the service would
/// itself, so they come back after the restart even when the old service
/// never saved them. Nothing is written for no panes.
pub fn save_layout(client: &Client) -> std::io::Result<()> {
    let Some(file) = daemon::layout_file(std::env::var_os("KIBANN_SOCKET").is_some()) else {
        return Ok(());
    };
    let layout = client.layout()?;
    let panes = match client.request(|req| ClientMsg::Panes { req })? {
        Reply::Panes { panes } => panes,
        _ => Vec::new(),
    };
    let saved = Saved {
        layout,
        dirs: panes
            .into_iter()
            .filter_map(|p| Some((p.id, p.cwd?)))
            .collect(),
    };
    if saved.panes() == 0 {
        return Ok(());
    }
    daemon::saved::save(&file, &saved)
}

/// "claude, vim and 2 more", for the question.
pub fn list(programs: &[String]) -> String {
    let mut names: Vec<&str> = Vec::new();
    for p in programs {
        if !names.contains(&p.as_str()) {
            names.push(p);
        }
    }
    match names.as_slice() {
        [] => "a program".to_owned(),
        [one] => (*one).to_owned(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOCKET: &str = "/var/folders/T/kibann/daemon.sock";

    fn ps(rows: &[&str]) -> String {
        rows.join("\n")
    }

    #[test]
    fn shells_at_their_prompt_run_nothing() {
        let table = ps(&[
            "  100     1   100     0 /Applications/Kibann Terminal.app/Contents/MacOS/kibann --daemon --socket /var/folders/T/kibann/daemon.sock",
            "  101   100   101   101 -zsh",
            "  102   100   102   102 -zsh",
            "  103   101   103   101 gitstatusd -s -1",
        ]);
        assert_eq!(running_in(&table, Path::new(SOCKET)), Some(vec![]));
    }

    #[test]
    fn a_program_in_front_is_running() {
        let table = ps(&[
            "  100     1   100     0 /Applications/Kibann Terminal.app/Contents/MacOS/kibann --daemon --socket /var/folders/T/kibann/daemon.sock",
            "  101   100   101   200 -zsh",
            "  200   101   200   200 /opt/homebrew/bin/claude --resume",
            "  102   100   102   102 -zsh",
            "  300     1   300   300 /usr/bin/vim not-ours",
        ]);
        assert_eq!(
            running_in(&table, Path::new(SOCKET)),
            Some(vec!["claude".to_owned()])
        );
    }

    #[test]
    fn another_service_is_not_ours() {
        let table = ps(&[
            "  100     1   100     0 /x/kibann --daemon --socket /var/folders/T/kibann/daemon-dev.sock",
            "  101   100   101   200 -zsh",
        ]);
        assert_eq!(running_in(&table, Path::new(SOCKET)), None);
    }

    #[test]
    fn programs_are_listed_once_each() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(list(&v(&["claude"])), "claude");
        assert_eq!(list(&v(&["claude", "claude", "vim"])), "claude and vim");
        assert_eq!(
            list(&v(&["claude", "vim", "htop", "npm"])),
            "claude, vim and 2 more"
        );
    }
}
