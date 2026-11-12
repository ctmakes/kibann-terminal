//! `kibann`: the command line agents use to open panes, type into them, read
//! them and wait on them. A thin client of the daemon's socket.
//!
//! Output is JSON on stdout (`pane read` prints the text itself). Errors are
//! `{"error": {"code", "message"}}` on stderr with exit code 1; a malformed
//! command line prints usage and exits 2.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use proto::layout::{Action, Dir, Layout, PaneId, TabId};
use proto::status::{AgentState, PaneStatus};
#[cfg(unix)]
use proto::wire::PROTOCOL_VERSION;
use proto::wire::{ClientMsg, ReadSource, Reply};
use proto::{InputEvent, KeyAction, mods};
use serde_json::{Value, json};

use crate::client::{Client, unexpected};

/// The agent guide, installed by `kibann skill install`.
pub const SKILL: &str = include_str!("../../../skills/kibann/SKILL.md");

const USAGE: &str = "\
usage: kibann <command>

  pane list                         every pane: id, tab, title, directory, focused,
                                    agent and its state, git checkout
  pane current                      the pane this shell runs in
  pane split [P] [--direction right|down] [--cwd DIR] [--focus]
                                    a new shell next to P (default: this pane)
  tab new [--cwd DIR] [--focus]     a new tab
  pane run P TEXT [--wait] [--timeout MS]
                                    type TEXT and Enter; --wait waits for the
                                    command to finish and returns its exit code
  pane send P TEXT                  type TEXT without Enter
  pane keys P KEY...                press keys: enter, esc, tab, up, ctrl+c, ...
  pane read P [--source visible|recent] [--lines N]
                                    print the pane's text
  pane wait P (--match TEXT | --regex RE | --exit | --until STATE[,STATE])
            [--timeout MS]          wait for text to appear, the shell to exit,
                                    or P's agent to be idle|working|blocked|done
  pane report P --agent NAME --state idle|working|blocked [--seq N]
                                    say what the agent in P is doing (beats
                                    what its screen shows)
  pane release P                    forget P's report
  pane focus P                      focus P and show its tab
  pane close P                      end P's shell
  skill show | skill install        the guide for agents (to ~/.claude/skills/kibann)

P is a pane id like p3, or --current (this pane, from $KIBANN_PANE).
Without P where one is optional, the calling pane is used.";

/// Is this a CLI invocation (as opposed to the app or the daemon)?
pub fn is_command(first: Option<&OsString>) -> bool {
    matches!(
        first.and_then(|a| a.to_str()),
        Some("pane" | "tab" | "skill" | "help" | "--help" | "-h")
    )
}

/// A failed command: usage (exit 2) or an error (exit 1).
#[derive(Debug)]
enum Fail {
    Usage(String),
    Error { code: &'static str, message: String },
}

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Self {
        Fail::Error {
            code: "daemon",
            message: e.to_string(),
        }
    }
}

fn usage(m: impl Into<String>) -> Fail {
    Fail::Usage(m.into())
}

fn error(code: &'static str, m: impl Into<String>) -> Fail {
    Fail::Error {
        code,
        message: m.into(),
    }
}

type Out = Result<Option<Value>, Fail>;

/// Run the command line `args` (after the program name). Returns the exit code.
pub fn run(args: Vec<OsString>) -> i32 {
    let args: Vec<String> = args
        .into_iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    match dispatch(&args) {
        Ok(Some(v)) => {
            println!("{v}");
            0
        }
        Ok(None) => 0,
        Err(Fail::Usage(m)) => {
            if !m.is_empty() {
                eprintln!("kibann: {m}\n");
            }
            eprintln!("{USAGE}");
            2
        }
        Err(Fail::Error { code, message }) => {
            eprintln!("{}", json!({"error": {"code": code, "message": message}}));
            1
        }
    }
}

/// Flags and positional arguments.
struct Args {
    pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

/// Flags that take a value.
const VALUED: &[&str] = &[
    "--direction",
    "--cwd",
    "--timeout",
    "--source",
    "--lines",
    "--match",
    "--regex",
    "--until",
    "--agent",
    "--state",
    "--seq",
];

impl Args {
    fn parse(args: &[String]) -> Result<Self, Fail> {
        let mut pos = Vec::new();
        let mut flags = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--" {
                pos.extend(it.by_ref().cloned());
            } else if let Some((k, v)) = a.split_once('=').filter(|_| a.starts_with("--")) {
                flags.push((k.to_owned(), Some(v.to_owned())));
            } else if VALUED.contains(&a.as_str()) {
                let v = it
                    .next()
                    .ok_or_else(|| usage(format!("{a} needs a value")))?;
                flags.push((a.clone(), Some(v.clone())));
            } else if a.starts_with("--") {
                flags.push((a.clone(), None));
            } else {
                pos.push(a.clone());
            }
        }
        Ok(Self { pos, flags })
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(k, _)| k == name)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn only(&self, allowed: &[&str]) -> Result<(), Fail> {
        for (k, _) in &self.flags {
            if k != "--current" && !allowed.contains(&k.as_str()) {
                return Err(usage(format!("unknown option {k}")));
            }
        }
        Ok(())
    }

    fn millis(&self, default: u64) -> Result<Duration, Fail> {
        match self.get("--timeout") {
            Some(v) => v
                .parse()
                .map(Duration::from_millis)
                .map_err(|_| usage("--timeout takes milliseconds")),
            None => Ok(Duration::from_millis(default)),
        }
    }
}

/// `p3` or `3`.
fn parse_pane(s: &str) -> Result<PaneId, Fail> {
    s.strip_prefix('p')
        .unwrap_or(s)
        .parse()
        .map_err(|_| usage(format!("not a pane id: {s}")))
}

fn current_pane() -> Result<PaneId, Fail> {
    let p = std::env::var("KIBANN_PANE")
        .map_err(|_| error("no_pane", "not in a kibann pane ($KIBANN_PANE is not set)"))?;
    parse_pane(&p)
}

/// The pane named first among the positional arguments (removed), or the
/// calling pane when `optional`.
fn take_pane(a: &mut Args, optional: bool) -> Result<PaneId, Fail> {
    if a.has("--current") {
        return current_pane();
    }
    if !a.pos.is_empty() && (a.pos[0].starts_with('p') || a.pos[0].parse::<u32>().is_ok()) {
        return parse_pane(&a.pos.remove(0));
    }
    if optional {
        current_pane()
    } else {
        Err(usage("which pane? give an id like p3, or --current"))
    }
}

fn pane_id(p: PaneId) -> String {
    format!("p{p}")
}

fn tab_id(t: TabId) -> String {
    format!("t{t}")
}

/// The daemon runs inside the app there (no socket yet), so nothing can
/// reach it from outside.
#[cfg(not(unix))]
fn connect() -> Result<Client, Fail> {
    Err(error(
        "unsupported",
        "the kibann command needs the daemon's socket, which this platform doesn't have yet",
    ))
}

#[cfg(unix)]
fn connect() -> Result<Client, Fail> {
    let socket = crate::paths::socket_path(cfg!(debug_assertions));
    let client = Client::connect(&socket, |_| {}).map_err(|e| {
        error(
            "not_running",
            format!(
                "kibann isn't running (no daemon at {}: {e})",
                socket.display()
            ),
        )
    })?;
    let (version, _) = client.hello()?;
    if version != PROTOCOL_VERSION {
        return Err(error(
            "version",
            format!(
                "the running kibann speaks protocol {version}, this command {PROTOCOL_VERSION}"
            ),
        ));
    }
    Ok(client)
}

/// Fails unless `pane` is in the layout.
fn check_pane(c: &Client, pane: PaneId) -> Result<Layout, Fail> {
    let layout = c.layout()?;
    if layout.tab_of(pane).is_none() {
        return Err(error("not_found", format!("no pane {}", pane_id(pane))));
    }
    Ok(layout)
}

fn dispatch(args: &[String]) -> Out {
    let (noun, verb) = (
        args.first().map(String::as_str),
        args.get(1).map(String::as_str),
    );
    let mut a = Args::parse(args.get(2..).unwrap_or_default())?;
    match (noun, verb) {
        (Some("help" | "--help" | "-h"), _) | (None, _) => Err(usage("")),
        (Some("skill"), Some("show")) => {
            print!("{SKILL}");
            Ok(None)
        }
        (Some("skill"), Some("install")) => skill_install(),
        (Some("tab"), Some("new")) => tab_new(&a),
        (Some("pane"), Some("list")) => pane_list(&connect()?, None),
        (Some("pane"), Some("current")) => {
            let p = current_pane()?;
            pane_list(&connect()?, Some(p))
        }
        (Some("pane"), Some("split")) => {
            a.only(&["--direction", "--cwd", "--focus"])?;
            let dir = match a.get("--direction").unwrap_or("right") {
                "right" => Dir::Row,
                "down" => Dir::Column,
                d => return Err(usage(format!("--direction is right or down, not {d}"))),
            };
            let pane = take_pane(&mut a, true)?;
            let c = connect()?;
            let (p, t) = c.act(Action::Split {
                pane,
                dir,
                cwd: cwd(&a)?,
                argv: None,
                focus: a.has("--focus"),
            })?;
            Ok(Some(json!({"pane": made(p, t)})))
        }
        (Some("pane"), Some("run" | "send")) => {
            a.only(&["--wait", "--timeout"])?;
            let pane = take_pane(&mut a, false)?;
            let [text] = &a.pos[..] else {
                return Err(usage("give the text as one argument (quote it)"));
            };
            let enter = verb == Some("run");
            let c = connect()?;
            check_pane(&c, pane)?;
            if enter && a.has("--wait") {
                return run_and_wait(&c, pane, text, a.millis(120_000)?);
            }
            type_text(&c, pane, text, enter)?;
            Ok(Some(json!({"ok": true})))
        }
        (Some("pane"), Some("keys")) => {
            a.only(&[])?;
            let pane = take_pane(&mut a, false)?;
            if a.pos.is_empty() {
                return Err(usage("which keys?"));
            }
            let events = a
                .pos
                .iter()
                .map(|k| key_events(k))
                .collect::<Result<Vec<_>, _>>()?;
            let c = connect()?;
            check_pane(&c, pane)?;
            for event in events.into_iter().flatten() {
                c.send(&ClientMsg::Input {
                    session: pane,
                    event,
                })?;
            }
            Ok(Some(json!({"ok": true})))
        }
        (Some("pane"), Some("read")) => {
            a.only(&["--source", "--lines"])?;
            let pane = take_pane(&mut a, false)?;
            let source = source(&a)?;
            let lines = lines(&a)?;
            let text = read(&connect()?, pane, source, lines)?;
            println!("{text}");
            Ok(None)
        }
        (Some("pane"), Some("wait")) => {
            a.only(&["--match", "--regex", "--exit", "--until", "--timeout"])?;
            let pane = take_pane(&mut a, false)?;
            wait(&connect()?, pane, &a)
        }
        (Some("pane"), Some("report")) => {
            a.only(&["--agent", "--state", "--seq"])?;
            let pane = take_pane(&mut a, true)?;
            let agent = a
                .get("--agent")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| usage("--agent NAME is required"))?
                .to_owned();
            let state = match a.get("--state").map(|s| (s, AgentState::parse(s))) {
                Some((_, Some(s))) if s != AgentState::Done => s,
                Some((s, _)) => {
                    return Err(usage(format!(
                        "--state is idle, working or blocked, not {s}"
                    )));
                }
                None => return Err(usage("--state is required")),
            };
            let seq = a
                .get("--seq")
                .map(|n| n.parse().map_err(|_| usage("--seq takes a number")))
                .transpose()?;
            match connect()?.request(|req| ClientMsg::Report {
                req,
                pane,
                agent,
                state,
                seq,
            })? {
                Reply::Ok => Ok(Some(json!({"ok": true}))),
                Reply::Error { message } if message == "stale seq" => {
                    Err(error("stale", "a report with a higher --seq came first"))
                }
                Reply::Error { message } => Err(error("not_found", message)),
                other => Err(unexpected(other).into()),
            }
        }
        (Some("pane"), Some("release")) => {
            a.only(&[])?;
            let pane = take_pane(&mut a, true)?;
            match connect()?.request(|req| ClientMsg::Release { req, pane })? {
                Reply::Ok => Ok(Some(json!({"ok": true}))),
                Reply::Error { message } => Err(error("not_found", message)),
                other => Err(unexpected(other).into()),
            }
        }
        (Some("pane"), Some("focus")) => {
            let pane = take_pane(&mut a, false)?;
            let (p, t) = connect()?.act(Action::Focus { pane })?;
            Ok(Some(json!({"pane": made(p, t)})))
        }
        (Some("pane"), Some("close")) => {
            let pane = take_pane(&mut a, false)?;
            connect()?.act(Action::Close { pane })?;
            Ok(Some(json!({"closed": pane_id(pane)})))
        }
        (Some(n), v) => Err(usage(format!(
            "unknown command: {n}{}",
            v.map(|v| format!(" {v}")).unwrap_or_default()
        ))),
    }
}

fn made(p: Option<PaneId>, t: Option<TabId>) -> Value {
    json!({"id": p.map(pane_id), "tab": t.map(tab_id)})
}

/// `--cwd`, made absolute against our own directory.
fn cwd(a: &Args) -> Result<Option<String>, Fail> {
    let Some(d) = a.get("--cwd") else {
        return Ok(None);
    };
    let p = std::env::current_dir()?.join(d);
    if !p.is_dir() {
        return Err(error("not_found", format!("no directory {}", p.display())));
    }
    Ok(Some(p.to_string_lossy().into_owned()))
}

fn source(a: &Args) -> Result<ReadSource, Fail> {
    match a.get("--source").unwrap_or("visible") {
        "visible" => Ok(ReadSource::Visible),
        "recent" => Ok(ReadSource::Recent),
        s => Err(usage(format!("--source is visible or recent, not {s}"))),
    }
}

fn lines(a: &Args) -> Result<Option<u32>, Fail> {
    a.get("--lines")
        .map(|n| n.parse().map_err(|_| usage("--lines takes a number")))
        .transpose()
}

fn tab_new(a: &Args) -> Out {
    a.only(&["--cwd", "--focus"])?;
    let near = std::env::var("KIBANN_PANE")
        .ok()
        .and_then(|p| parse_pane(&p).ok());
    let (p, t) = connect()?.act(Action::NewTab {
        near,
        cwd: cwd(a)?,
        argv: None,
        focus: a.has("--focus"),
    })?;
    Ok(Some(json!({"tab": t.map(tab_id), "pane": made(p, t)})))
}

fn pane_list(c: &Client, only: Option<PaneId>) -> Out {
    let layout = c.layout()?;
    let panes = match c.request(|req| ClientMsg::Panes { req })? {
        Reply::Panes { panes } => panes,
        other => return Err(unexpected(other).into()),
    };
    let entry = |p: &proto::wire::PaneInfo| {
        let focused =
            layout.active == Some(p.tab) && layout.tab(p.tab).is_some_and(|t| t.focused == p.id);
        json!({
            "id": pane_id(p.id),
            "tab": tab_id(p.tab),
            "title": p.title,
            "cwd": p.cwd,
            "focused": focused,
            "agent": p.agent,
            "state": p.state.map(AgentState::name),
            "git": p.git,
        })
    };
    match only {
        Some(id) => {
            let p = panes
                .iter()
                .find(|p| p.id == id)
                .ok_or_else(|| error("not_found", format!("no pane {}", pane_id(id))))?;
            Ok(Some(json!({"pane": entry(p)})))
        }
        None => Ok(Some(
            json!({"panes": panes.iter().map(entry).collect::<Vec<_>>()}),
        )),
    }
}

fn read(c: &Client, pane: PaneId, source: ReadSource, lines: Option<u32>) -> Result<String, Fail> {
    match c.request(|req| ClientMsg::Read {
        req,
        pane,
        source,
        lines,
    })? {
        Reply::Text { text } => Ok(text),
        Reply::Error { message } => Err(error("not_found", message)),
        other => Err(unexpected(other).into()),
    }
}

/// Type `text` (pasted if it has several lines, so the shell takes it as
/// one), then Enter if `enter`.
fn type_text(c: &Client, pane: PaneId, text: &str, enter: bool) -> Result<(), Fail> {
    // A pane just opened may not have a shell reading yet: text typed now
    // shows twice (echoed, then redrawn by the prompt). Wait for its first
    // output, usually the prompt.
    let deadline = Instant::now() + Duration::from_secs(3);
    while read(c, pane, ReadSource::Visible, None)?.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let event = if text.contains('\n') {
        InputEvent::Paste { data: text.into() }
    } else {
        InputEvent::Text { data: text.into() }
    };
    c.send(&ClientMsg::Input {
        session: pane,
        event,
    })?;
    if enter {
        c.send(&ClientMsg::Input {
            session: pane,
            event: InputEvent::Text { data: "\r".into() },
        })?;
    }
    Ok(())
}

struct Status {
    ended: bool,
    code: Option<u32>,
    /// The code printed with the mark asked for.
    mark: Option<i32>,
    agent: Option<PaneStatus>,
}

fn status(c: &Client, pane: PaneId, mark: Option<&str>) -> Result<Status, Fail> {
    match c.request(|req| ClientMsg::Status {
        req,
        pane,
        mark: mark.map(String::from),
    })? {
        Reply::Status {
            ended,
            code,
            mark,
            agent,
        } => Ok(Status {
            ended,
            code,
            mark,
            agent,
        }),
        Reply::Error { message } => Err(error("not_found", message)),
        other => Err(unexpected(other).into()),
    }
}

const POLL: Duration = Duration::from_millis(100);

fn timed_out(what: &str, timeout: Duration) -> Fail {
    error(
        "timeout",
        format!("{what} within {} ms", timeout.as_millis()),
    )
}

/// Type the command with a suffix that prints a mark and its exit code
/// when it finishes, and wait for the mark.
fn run_and_wait(c: &Client, pane: PaneId, text: &str, timeout: Duration) -> Out {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let mark = format!("k{}x{nanos:x}", std::process::id());
    let osc = proto::wire::MARK_OSC;
    let line = format!("{text}; printf '\\033]{osc};{mark};%d\\007' $?");
    type_text(c, pane, &line, true)?;
    let deadline = Instant::now() + timeout;
    loop {
        let Status {
            ended,
            code,
            mark: got,
            ..
        } = status(c, pane, Some(&mark))?;
        if let Some(code) = got {
            return Ok(Some(json!({"exit_code": code})));
        }
        if ended {
            return Err(error(
                "ended",
                format!("the shell exited (code {code:?}) before the command finished"),
            ));
        }
        if Instant::now() >= deadline {
            return Err(timed_out("the command didn't finish", timeout));
        }
        std::thread::sleep(POLL);
    }
}

fn wait(c: &Client, pane: PaneId, a: &Args) -> Out {
    let timeout = a.millis(120_000)?;
    let deadline = Instant::now() + timeout;
    let regex = a
        .get("--regex")
        .map(regex::Regex::new)
        .transpose()
        .map_err(|e| usage(format!("bad --regex: {e}")))?;
    let text = a.get("--match");
    if let Some(until) = a.get("--until") {
        let want = until
            .split(',')
            .map(|s| {
                AgentState::parse(s.trim()).ok_or_else(|| {
                    usage(format!(
                        "--until takes idle, working, blocked or done, not {s}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        loop {
            let s = status(c, pane, None)?;
            if s.ended {
                return Err(error(
                    "ended",
                    format!("pane {} ended (code {:?})", pane_id(pane), s.code),
                ));
            }
            if let Some(agent) = s.agent.filter(|a| want.contains(&a.state)) {
                return Ok(Some(
                    json!({"agent": agent.agent, "state": agent.state.name()}),
                ));
            }
            if Instant::now() >= deadline {
                return Err(timed_out(&format!("not {until}"), timeout));
            }
            std::thread::sleep(POLL);
        }
    }
    if a.has("--exit") {
        loop {
            let Status { ended, code, .. } = status(c, pane, None)?;
            if ended {
                return Ok(Some(json!({"exit_code": code})));
            }
            if Instant::now() >= deadline {
                return Err(timed_out("the shell didn't exit", timeout));
            }
            std::thread::sleep(POLL);
        }
    }
    if text.is_none() && regex.is_none() {
        return Err(usage(
            "wait for what? --match TEXT, --regex RE, --exit or --until STATE",
        ));
    }
    loop {
        let screen = match read(c, pane, ReadSource::Recent, None) {
            Ok(s) => s,
            // The pane went away (its shell exited).
            Err(_) => {
                return Err(error(
                    "ended",
                    format!("pane {} ended before a match", pane_id(pane)),
                ));
            }
        };
        let found = screen.lines().rev().find(|l| match (&regex, text) {
            (Some(r), _) => r.is_match(l),
            (None, Some(t)) => l.contains(t),
            (None, None) => false,
        });
        if let Some(line) = found {
            return Ok(Some(json!({"matched": line})));
        }
        if Instant::now() >= deadline {
            return Err(timed_out("no match", timeout));
        }
        std::thread::sleep(POLL);
    }
}

/// Press and release of a key like `enter`, `ctrl+c` or `shift+tab`.
fn key_events(spec: &str) -> Result<Vec<InputEvent>, Fail> {
    let lower = spec.to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('+').collect();
    let name = parts.pop().unwrap_or_default();
    let mut m = 0u16;
    for p in parts {
        m |= match p {
            "ctrl" | "control" => mods::CTRL,
            "alt" | "opt" | "option" => mods::ALT,
            "shift" => mods::SHIFT,
            "cmd" | "super" | "meta" => mods::SUPER,
            _ => return Err(usage(format!("unknown modifier in {spec}"))),
        };
    }
    fn named(
        code: &str,
        key: &str,
        text: Option<&'static str>,
    ) -> (String, String, Option<&'static str>) {
        (code.to_owned(), key.to_owned(), text)
    }
    let (code, key, text) = match name {
        "enter" | "return" => named("Enter", "Enter", Some("\r")),
        "esc" | "escape" => named("Escape", "Escape", None),
        "tab" => named("Tab", "Tab", Some("\t")),
        "backspace" => named("Backspace", "Backspace", None),
        "delete" | "del" => named("Delete", "Delete", None),
        "space" => named("Space", " ", Some(" ")),
        "up" => named("ArrowUp", "ArrowUp", None),
        "down" => named("ArrowDown", "ArrowDown", None),
        "left" => named("ArrowLeft", "ArrowLeft", None),
        "right" => named("ArrowRight", "ArrowRight", None),
        "home" => named("Home", "Home", None),
        "end" => named("End", "End", None),
        "pageup" => named("PageUp", "PageUp", None),
        "pagedown" => named("PageDown", "PageDown", None),
        n if n.len() > 1
            && n.starts_with('f')
            && n[1..].parse::<u8>().is_ok_and(|i| (1..=12).contains(&i)) =>
        {
            let f = format!("F{}", &n[1..]);
            (f.clone(), f, None)
        }
        n if n.len() == 1 && n.as_bytes()[0].is_ascii_alphanumeric() => {
            let c = n.as_bytes()[0] as char;
            let code = if c.is_ascii_digit() {
                format!("Digit{c}")
            } else {
                format!("Key{}", c.to_ascii_uppercase())
            };
            let key = if m & mods::SHIFT != 0 {
                c.to_ascii_uppercase().to_string()
            } else {
                c.to_string()
            };
            let plain = m & (mods::CTRL | mods::ALT | mods::SUPER) == 0;
            let text = plain.then(|| key.clone());
            return Ok(press_release(code, key.clone(), text, m));
        }
        _ => return Err(usage(format!("unknown key {spec}"))),
    };
    let plain = m & (mods::CTRL | mods::ALT | mods::SUPER) == 0;
    Ok(press_release(
        code,
        key,
        text.filter(|_| plain).map(String::from),
        m,
    ))
}

fn press_release(code: String, key: String, text: Option<String>, m: u16) -> Vec<InputEvent> {
    let ev = |action, text| InputEvent::Key {
        action,
        code: code.clone(),
        key: key.clone(),
        text,
        mods: m,
        composing: false,
    };
    vec![ev(KeyAction::Press, text), ev(KeyAction::Release, None)]
}

fn skill_install() -> Out {
    let home = std::env::var_os("HOME").ok_or_else(|| error("no_home", "$HOME is not set"))?;
    let dir = PathBuf::from(home).join(".claude/skills/kibann");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, SKILL)?;
    Ok(Some(json!({"installed": path})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        let ev = key_events("ctrl+c").unwrap();
        assert!(matches!(
            &ev[0],
            InputEvent::Key { code, text: None, mods: m, action: KeyAction::Press, .. }
                if code == "KeyC" && *m == mods::CTRL
        ));
        let ev = key_events("Enter").unwrap();
        assert!(matches!(&ev[0], InputEvent::Key { text: Some(t), .. } if t == "\r"));
        assert!(key_events("f5").is_ok());
        assert!(key_events("hyper+x").is_err());
        assert!(key_events("nope").is_err());
    }

    #[test]
    fn arguments() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let mut a = Args::parse(&s(&["p3", "--direction", "down", "--focus"])).unwrap();
        assert_eq!(take_pane(&mut a, true).unwrap(), 3);
        assert_eq!(a.get("--direction"), Some("down"));
        assert!(a.has("--focus"));
        let mut a = Args::parse(&s(&["12", "echo hi", "--timeout=50"])).unwrap();
        assert_eq!(take_pane(&mut a, false).unwrap(), 12);
        assert_eq!(a.pos, vec!["echo hi"]);
        assert_eq!(a.millis(1).unwrap(), Duration::from_millis(50));
        assert!(Args::parse(&s(&["--lines"])).is_err());
    }
}
