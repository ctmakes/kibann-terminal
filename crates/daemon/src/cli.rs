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
use crate::layouts::{self, Found};

/// The agent guide, installed by `kibann skill install`.
pub const SKILL: &str = include_str!("../../../skills/kibann/SKILL.md");
/// The guide to layout files, installed beside it.
pub const SKILL_LAYOUTS: &str = include_str!("../../../skills/kibann-layouts/SKILL.md");

/// Every skill `kibann skill install` writes, by folder name.
const SKILLS: &[(&str, &str)] = &[("kibann", SKILL), ("kibann-layouts", SKILL_LAYOUTS)];

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
  pane diff P [--stat|--view]       P's uncommitted changes as a patch; with
                                    --stat the files as JSON; with --view a
                                    full-screen viewer that follows changes
  pane focus P                      focus P and show its tab
  pane close P                      end P's shell
  layout list                       the layouts: the project's, yours, built-in
  layout check [NAME|FILE]          read a layout without opening it: what it
                                    runs, or every problem in it
  layout open [NAME|FILE] [--yes]   open a layout's tabs (no NAME: the project's
                                    .kibann/layout.toml); --yes runs a file's
                                    commands without asking
  layout save NAME [--tab T] [--to FILE] [--relative] [--force]
                                    write the window (or tab T) as a layout,
                                    to your layouts folder or FILE
  layout save NAME --from LAYOUT    copy a layout (a built-in one) to edit
  watch SECS -- CMD...              run CMD every SECS seconds, like watch
  skill show [layouts]              the guide for agents (or the layouts one)
  skill install [--claude|--codex|--opencode]
                                    install both: ~/.claude/skills/ (Claude
                                    Code, OpenCode), and ~/.agents/skills/
                                    when Codex is installed (Codex)

P is a pane id like p3, or --current (this pane, from $KIBANN_PANE).
Without P where one is optional, the calling pane is used.";

/// Is this a CLI invocation (as opposed to the app or the daemon)?
pub fn is_command(first: Option<&OsString>) -> bool {
    matches!(
        first.and_then(|a| a.to_str()),
        Some("pane" | "tab" | "layout" | "watch" | "skill" | "help" | "--help" | "-h")
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
    "--tab",
    "--to",
    "--from",
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
    if noun == Some("watch") {
        return watch(&args[1..]);
    }
    let mut a = Args::parse(args.get(2..).unwrap_or_default())?;
    match (noun, verb) {
        (Some("help" | "--help" | "-h"), _) | (None, _) => Err(usage("")),
        (Some("skill"), Some("show")) => {
            match a.pos.first().map(String::as_str) {
                None => print!("{SKILL}"),
                Some("layouts") => print!("{SKILL_LAYOUTS}"),
                Some(o) => return Err(usage(format!("no skill {o}; there's layouts"))),
            }
            Ok(None)
        }
        (Some("layout"), Some("list")) => {
            a.only(&[])?;
            Ok(Some(json!({"layouts": layouts::list(&here()?)})))
        }
        (Some("layout"), Some("check")) => {
            a.only(&[])?;
            layout_check(&a)
        }
        (Some("layout"), Some("open")) => {
            a.only(&["--yes"])?;
            layout_open(&a)
        }
        (Some("layout"), Some("save")) => {
            a.only(&["--tab", "--to", "--relative", "--force", "--from"])?;
            layout_save(&a)
        }
        (Some("skill"), Some("install")) => {
            a.only(&["--claude", "--codex", "--opencode"])?;
            skill_install(&a)
        }
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
        (Some("pane"), Some("diff")) => {
            a.only(&["--stat", "--view"])?;
            let pane = take_pane(&mut a, true)?;
            if a.has("--view") {
                return view(pane);
            }
            let diff = diff(&connect()?, pane)?;
            if a.has("--stat") {
                let files: Vec<Value> = diff
                    .files
                    .iter()
                    .map(|f| {
                        json!({"path": f.path, "old_path": f.old_path, "status": f.status,
                               "added": f.added, "removed": f.removed, "binary": f.binary})
                    })
                    .collect();
                return Ok(Some(json!({"files": files, "truncated": diff.truncated})));
            }
            for f in &diff.files {
                match &f.patch {
                    Some(p) => print!("{p}"),
                    None if f.binary => println!("Binary file {} ({})", f.path, f.status),
                    None => println!("{} ({}): too large to show", f.path, f.status),
                }
                if f.truncated {
                    println!("[{}: cut at the size limit]", f.path);
                }
            }
            Ok(None)
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

fn diff(c: &Client, pane: PaneId) -> Result<proto::git::Diff, Fail> {
    match c.request(|req| ClientMsg::Diff { req, pane })? {
        Reply::Diff { diff } => Ok(diff),
        Reply::Error { message } if message.starts_with("no pane") => {
            Err(error("not_found", message))
        }
        Reply::Error { message } => Err(error("no_repo", message)),
        other => Err(unexpected(other).into()),
    }
}

#[cfg(unix)]
fn view(pane: PaneId) -> Out {
    let result = (|| {
        // Check the pane first, so a bad id fails before the screen changes.
        check_pane(&connect()?, pane)?;
        let socket = crate::paths::socket_path(cfg!(debug_assertions));
        crate::view::run(pane, |on_event| Client::connect(&socket, on_event))?;
        Ok(None)
    })();
    // The viewer usually runs alone in a split that closes when it exits:
    // say what went wrong and wait, or the split just flashes.
    if let Err(Fail::Error { message, .. }) = &result
        && std::io::IsTerminal::is_terminal(&std::io::stdin())
    {
        eprintln!("kibann: can't show the changes: {message}\r\n\r\nPress Enter to close.");
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
    }
    result
}

#[cfg(not(unix))]
fn view(_pane: PaneId) -> Out {
    connect()?;
    Ok(None)
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

/// Where each agent reads skills, under `$HOME`, and who reads it.
/// OpenCode reads Claude Code's folder; Codex reads `~/.agents/skills`.
const SKILL_DIRS: &[(&str, &str, &[&str])] = &[
    ("claude", ".claude/skills", &["Claude Code", "OpenCode"]),
    ("codex", ".agents/skills", &["Codex"]),
];

/// The skill folders to write: the agents named with `--claude`, `--codex`
/// or `--opencode`, else Claude Code's folder (read by OpenCode too) and
/// Codex's when Codex is installed.
fn skill_targets(a: &Args, has_codex: bool) -> Vec<&'static str> {
    let named: Vec<&str> = [
        ("--claude", "claude"),
        ("--opencode", "claude"),
        ("--codex", "codex"),
    ]
    .iter()
    .filter(|(flag, _)| a.has(flag))
    .map(|&(_, dir)| dir)
    .collect();
    let mut want = if named.is_empty() {
        let mut all = vec!["claude"];
        if has_codex {
            all.push("codex");
        }
        all
    } else {
        named
    };
    want.dedup();
    want
}

/// Is `program` on `$PATH`?
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(program).is_file()))
}

fn skill_install(a: &Args) -> Out {
    let home = std::env::var_os("HOME").ok_or_else(|| error("no_home", "$HOME is not set"))?;
    let home = PathBuf::from(home);
    let has_codex = on_path("codex") || home.join(".codex").is_dir();
    let mut installed = Vec::new();
    for key in skill_targets(a, has_codex) {
        let (_, rel, readers) = SKILL_DIRS.iter().find(|(k, ..)| *k == key).unwrap();
        for (name, text) in SKILLS {
            let dir = home.join(rel).join(name);
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("SKILL.md");
            std::fs::write(&path, text)?;
            installed.push(json!({"path": path, "read_by": readers}));
        }
    }
    Ok(Some(json!({"installed": installed})))
}

fn here() -> Result<PathBuf, Fail> {
    std::env::current_dir().map_err(|e| error("no_cwd", format!("no current directory: {e}")))
}

/// Find and read the layout named on the command line.
fn layout_found(
    a: &Args,
    here: &std::path::Path,
) -> Result<(Found, proto::layout::Template), Fail> {
    if a.pos.len() > 1 {
        return Err(usage("give one layout: a name or a file"));
    }
    let f = layouts::find(a.pos.first().map(String::as_str), here)
        .map_err(|m| error("not_found", m))?;
    let t = layouts::parse(&f, here).map_err(|p| {
        let file = f
            .path
            .as_ref()
            .map_or_else(|| f.name.clone(), |p| p.display().to_string());
        let lines: Vec<String> = p.0.iter().map(|p| format!("{file}:{p}")).collect();
        error("invalid", lines.join("\n"))
    })?;
    Ok((f, t))
}

fn layout_check(a: &Args) -> Out {
    let here = here()?;
    let (f, t) = layout_found(a, &here)?;
    let panes: usize = t.tabs.iter().map(|t| t.tree.leaves().len()).sum();
    Ok(Some(json!({
        "name": t.name,
        "origin": f.origin,
        "path": f.path,
        "tabs": t.tabs.len(),
        "panes": panes,
        "runs": layouts::commands(&t),
        "asks": layouts::needs_asking(&f, &t),
        "layout": t,
    })))
}

fn layout_open(a: &Args) -> Out {
    let here = here()?;
    let (f, t) = layout_found(a, &here)?;
    if layouts::needs_asking(&f, &t) {
        if !a.has("--yes") {
            ask_to_run(&f, &t)?;
        }
        if let Some(p) = &f.path {
            layouts::trust(p, &f.text)?;
        }
    }
    let name = t.name.clone();
    let (p, tab) = connect()?
        .act(Action::OpenLayout { template: t })
        .map_err(|e| error("open", e.to_string()))?;
    Ok(Some(
        json!({"opened": name, "tab": tab.map(tab_id), "pane": made(p, tab)}),
    ))
}

/// Ask on the terminal before running a file's commands; without one (an
/// agent, a script), refuse and say how to agree.
fn ask_to_run(f: &Found, t: &proto::layout::Template) -> Result<(), Fail> {
    use std::io::{BufRead, IsTerminal, Write};
    let file = f
        .path
        .as_ref()
        .map_or_else(|| f.name.clone(), |p| p.display().to_string());
    let mut list = String::new();
    for r in layouts::commands(t) {
        list.push_str(&format!("  {}  (in {})\n", r.run, r.cwd));
    }
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(error(
            "untrusted",
            format!(
                "{file} runs commands you haven't agreed to:\n{list}Run kibann layout open again with --yes once the user agrees."
            ),
        ));
    }
    eprint!("{file} runs:\n{list}Run them? [y/N] ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    if matches!(line.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(error("declined", "not opened"))
    }
}

fn layout_save(a: &Args) -> Out {
    let here = here()?;
    let home = crate::paths::home_dir();
    let to = a.get("--to").map(|t| {
        crate::template::resolve_path(t, &here, home.as_deref()).unwrap_or_else(|| here.join(t))
    });
    let name = match (a.pos.first(), &to) {
        (Some(n), _) => n.clone(),
        (None, Some(t)) if t.ends_with(layouts::PROJECT_FILE) => t
            .parent()
            .and_then(std::path::Path::parent)
            .and_then(|d| d.file_name())
            .map_or_else(|| "project".into(), |n| n.to_string_lossy().into_owned()),
        (None, Some(t)) => t
            .file_stem()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        (None, None) => return Err(usage("give the layout a name (or --to FILE)")),
    };
    if a.pos.len() > 1 {
        return Err(usage("give one name"));
    }
    if to.is_none() && (name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.')) {
        return Err(usage(format!("{name:?} can't be a layout's name")));
    }
    let target = match to {
        Some(t) => t,
        None => crate::paths::layouts_dir()
            .ok_or_else(|| error("no_home", "no home directory for the layouts folder"))?
            .join(format!("{name}.toml")),
    };
    if target.exists() && !a.has("--force") {
        return Err(error(
            "exists",
            format!("{} exists; --force replaces it", target.display()),
        ));
    }
    let text = match a.get("--from") {
        Some(from) => {
            layouts::find(Some(from), &here)
                .map_err(|m| error("not_found", m))?
                .text
        }
        None => {
            let c = connect()?;
            let layout = c.layout()?;
            let panes = match c.request(|req| ClientMsg::Panes { req })? {
                Reply::Panes { panes } => panes,
                other => return Err(unexpected(other).into()),
            };
            let tab = match a.get("--tab") {
                Some(t) => Some(
                    t.strip_prefix('t')
                        .unwrap_or(t)
                        .parse::<TabId>()
                        .map_err(|_| usage(format!("not a tab id: {t}")))?,
                ),
                None => None,
            };
            let in_project = target
                .parent()
                .filter(|d| d.file_name().is_some_and(|n| n == ".kibann"))
                .and_then(std::path::Path::parent)
                .map(std::path::Path::to_path_buf);
            let relative_to = if a.has("--relative") {
                Some(here.clone())
            } else {
                in_project
            };
            layouts::snapshot(
                &layout,
                &panes,
                &layouts::SaveSpec {
                    name: Some(&name),
                    tab,
                    relative_to: relative_to.as_deref(),
                },
            )
            .map_err(|m| error("not_found", m))?
        }
    };
    if let Some(d) = target.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&target, &text)?;
    // Saved by the user: opening it again needn't ask.
    layouts::trust(&target, &text)?;
    Ok(Some(json!({"saved": target, "name": name})))
}

/// `kibann watch SECS -- CMD...`: run CMD every SECS seconds, showing its
/// output in place of the last. Works where `watch` isn't installed
/// (macOS) or doesn't exist (Windows).
fn watch(args: &[String]) -> Out {
    use std::io::Write;
    let secs: f64 = args
        .first()
        .and_then(|s| s.parse().ok())
        .filter(|s: &f64| *s >= 0.1 && s.is_finite())
        .ok_or_else(|| usage("watch SECS -- CMD...: SECS is how often, in seconds"))?;
    let mut cmd: &[String] = &args[1..];
    if cmd.first().is_some_and(|a| a == "--") {
        cmd = &cmd[1..];
    }
    let Some((prog, rest)) = cmd.split_first() else {
        return Err(usage("watch SECS -- CMD...: which command?"));
    };
    let shown = layouts::shell_line(cmd);
    loop {
        let mut screen = format!("\x1b[H\x1b[2mEvery {secs}s: {shown}\x1b[0m\x1b[K\n\x1b[K\n");
        match std::process::Command::new(prog)
            .args(rest)
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(o) => {
                for part in [&o.stdout, &o.stderr] {
                    for line in String::from_utf8_lossy(part).lines() {
                        screen.push_str(line);
                        screen.push_str("\x1b[K\n");
                    }
                }
            }
            Err(e) => screen.push_str(&format!("kibann watch: can't run {prog}: {e}\x1b[K\n")),
        }
        // Clear what's left of the last run below.
        screen.push_str("\x1b[J");
        let mut out = io::stdout().lock();
        if out
            .write_all(screen.as_bytes())
            .and_then(|_| out.flush())
            .is_err()
        {
            return Ok(None);
        }
        drop(out);
        std::thread::sleep(Duration::from_secs_f64(secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_folders() {
        let parse =
            |v: &[&str]| Args::parse(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
        assert_eq!(skill_targets(&parse(&[]), false), ["claude"]);
        assert_eq!(skill_targets(&parse(&[]), true), ["claude", "codex"]);
        assert_eq!(skill_targets(&parse(&["--codex"]), false), ["codex"]);
        assert_eq!(skill_targets(&parse(&["--opencode"]), true), ["claude"]);
        assert_eq!(
            skill_targets(&parse(&["--claude", "--opencode"]), true),
            ["claude"]
        );
    }

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
