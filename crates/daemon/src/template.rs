//! Layout files: tabs and splits, with a directory and a command per pane,
//! written by hand (or by `kibann layout save`) and opened with `kibann
//! layout open`. TOML, because splits nest. See `plans/layouts.md`.
//!
//! [`parse`] turns a file into a [`Template`] (directories made absolute and
//! checked, commands picked for this system, splits of N panes turned into
//! binary ones); [`write`] goes the other way, from the layout on screen.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use proto::layout::{Dir, PaneId, Run, TabTemplate, Template, TemplateTree, Tree};
use serde::Deserialize;
use toml::Spanned;

pub const MAX_TABS: usize = 16;
pub const MAX_PANES: usize = 32;
/// A pane's share of its split, in percent.
const MIN_SIZE: f64 = 5.0;
const MAX_SIZE: f64 = 95.0;

/// Something wrong with a layout file, at a line and column (both from 1).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Problem {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.message)
    }
}

/// Every problem found, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct Problems(pub Vec<Problem>);

impl fmt::Display for Problems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, p) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{p}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Problems {}

/// A layout file's text and where it's being opened.
pub struct Source<'a> {
    pub text: &'a str,
    /// The file, if it is one (a built-in layout isn't). Relative paths in
    /// it start from its directory.
    pub file: Option<&'a Path>,
    /// Where it's opened from: the CLI's directory, or the focused pane's.
    /// Absolute.
    pub opened_from: &'a Path,
    /// For `~`.
    pub home: Option<&'a Path>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    name: Option<String>,
    root: Option<Spanned<String>>,
    #[serde(default, rename = "tab")]
    tabs: Vec<Spanned<Node>>,
}

/// A tab, a split or a pane: which one depends on the keys given.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    title: Option<Spanned<String>>,
    focus: Option<Spanned<bool>>,
    cwd: Option<Spanned<String>>,
    command: Option<Spanned<toml::Value>>,
    exec: Option<Spanned<Vec<String>>>,
    missing: Option<Spanned<String>>,
    split: Option<Spanned<SplitDir>>,
    panes: Option<Spanned<Vec<Spanned<Node>>>>,
    size: Option<Spanned<f64>>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum SplitDir {
    Right,
    Down,
}

/// The systems `command` can be given for, by table key.
const SYSTEMS: [&str; 4] = ["macos", "linux", "windows", "default"];

fn this_system() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "default"
    }
}

/// The commands to try on this system, from a `command` value: a string,
/// a list to try in order, or a table of either by system. `Ok(None)` when
/// the table gives none for this system. Every entry is checked, not only
/// this system's, so a mistake shows wherever the file is opened.
fn commands_for_this_system(v: &toml::Value) -> Result<Option<Vec<String>>, String> {
    fn choices(v: &toml::Value) -> Result<Vec<String>, String> {
        let list = match v {
            toml::Value::String(s) => vec![s.clone()],
            toml::Value::Array(a) => a
                .iter()
                .map(|x| x.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or("each command in the list is a string")?,
            _ => {
                return Err(
                    "`command` is a string, a list of them to try in order, or a table of them by system (macos, linux, windows, default)".into(),
                )
            }
        };
        if list.is_empty() {
            return Err("the list of commands is empty".into());
        }
        if list.iter().any(|c| c.trim().is_empty()) {
            return Err("a command is empty".into());
        }
        Ok(list)
    }
    let toml::Value::Table(t) = v else {
        return choices(v).map(Some);
    };
    let mut found = HashMap::new();
    for (k, c) in t {
        if !SYSTEMS.contains(&k.as_str()) {
            return Err(format!(
                "no system `{k}` (use macos, linux, windows or default)"
            ));
        }
        found.insert(k.as_str(), choices(c)?);
    }
    Ok(found
        .remove(this_system())
        .or_else(|| found.remove("default")))
}

/// Line and column (from 1, the column in characters) of byte `at`.
fn position(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let start = before.rfind('\n').map_or(0, |i| i + 1);
    (line, before[start..].chars().count() + 1)
}

struct Ctx<'a> {
    text: &'a str,
    home: Option<&'a Path>,
    problems: Vec<Problem>,
    panes: usize,
}

impl Ctx<'_> {
    fn problem(&mut self, span: Range<usize>, message: impl Into<String>) {
        let (line, col) = position(self.text, span.start);
        self.problems.push(Problem {
            line,
            col,
            message: message.into(),
        });
    }

    /// `raw` made absolute against `base`; a problem (and `None`) when it
    /// uses `~` and there's no home.
    fn path(&mut self, raw: &Spanned<String>, base: &Path) -> Option<PathBuf> {
        match resolve_path(raw.get_ref(), base, self.home) {
            Some(p) => Some(p),
            None => {
                self.problem(raw.span(), "`~` needs a home directory, and there is none");
                None
            }
        }
    }

    /// Build a tab's tree. `focus` collects the index of the leaf asking for
    /// focus, by reading order.
    fn node(
        &mut self,
        node: &Spanned<Node>,
        parent_cwd: &Path,
        top: bool,
        leaves: &mut usize,
        focus: &mut Option<(usize, Range<usize>)>,
    ) -> TemplateTree {
        let n = node.get_ref();
        if !top && let Some(t) = &n.title {
            self.problem(t.span(), "`title` is for tabs, not panes");
        }
        let cwd = match &n.cwd {
            Some(raw) => self
                .path(raw, parent_cwd)
                .unwrap_or_else(|| parent_cwd.to_path_buf()),
            None => parent_cwd.to_path_buf(),
        };
        if n.split.is_some() || n.panes.is_some() {
            self.split(node, &cwd, top, leaves, focus)
        } else {
            self.leaf(node, cwd, top, leaves, focus)
        }
    }

    fn split(
        &mut self,
        node: &Spanned<Node>,
        cwd: &Path,
        top: bool,
        leaves: &mut usize,
        focus: &mut Option<(usize, Range<usize>)>,
    ) -> TemplateTree {
        let n = node.get_ref();
        for (key, span) in [
            ("command", n.command.as_ref().map(Spanned::span)),
            ("exec", n.exec.as_ref().map(Spanned::span)),
            ("missing", n.missing.as_ref().map(Spanned::span)),
        ] {
            if let Some(span) = span {
                self.problem(
                    span,
                    format!("a split has no `{key}`: give it to one of its panes"),
                );
            }
        }
        if !top && let Some(f) = &n.focus {
            self.problem(f.span(), "`focus` is for a pane or a tab, not a split");
        }
        let dir = match &n.split {
            Some(d) => match d.get_ref() {
                SplitDir::Right => Dir::Row,
                SplitDir::Down => Dir::Column,
            },
            None => {
                self.problem(
                    node.span(),
                    "`panes` needs `split = \"right\"` (side by side) or `split = \"down\"` (stacked)",
                );
                Dir::Row
            }
        };
        let empty = Vec::new();
        let (panes, panes_span) = match &n.panes {
            Some(p) => (p.get_ref(), p.span()),
            None => {
                self.problem(node.span(), "`split` needs `panes`, two or more of them");
                (&empty, node.span())
            }
        };
        if panes.len() == 1 {
            self.problem(panes_span.clone(), "a split needs two or more panes");
        }
        if panes.is_empty() {
            // Stand-in, so the tab still has a tree; the problem is reported.
            return self.leaf(node, cwd.to_path_buf(), true, leaves, focus);
        }
        let shares = self.shares(panes, panes_span);
        let kids: Vec<TemplateTree> = panes
            .iter()
            .map(|p| self.node(p, cwd, false, leaves, focus))
            .collect();
        chain(dir, kids, &shares)
    }

    fn leaf(
        &mut self,
        node: &Spanned<Node>,
        cwd: PathBuf,
        top: bool,
        leaves: &mut usize,
        focus: &mut Option<(usize, Range<usize>)>,
    ) -> TemplateTree {
        let n = node.get_ref();
        let index = *leaves;
        *leaves += 1;
        self.panes += 1;
        if !top && let Some(f) = n.focus.as_ref().filter(|f| *f.get_ref()) {
            if focus.is_some() {
                self.problem(f.span(), "only one pane in a tab can have `focus = true`");
            } else {
                *focus = Some((index, f.span()));
            }
        }
        if let (Some(_), Some(e)) = (&n.command, &n.exec) {
            self.problem(
                e.span(),
                "give `command` (typed into the shell) or `exec` (run instead of it), not both",
            );
        }
        if let (Some(m), None) = (&n.missing, &n.command) {
            self.problem(
                m.span(),
                "`missing` goes with `command`: it's shown when none of its commands is installed",
            );
        }
        let missing = n.missing.as_ref().map(|m| m.get_ref().clone());
        let run = if let Some(c) = &n.command {
            let span = c.span();
            match commands_for_this_system(c.get_ref()) {
                Err(m) => {
                    self.problem(span, m);
                    None
                }
                Ok(Some(v)) => Some(Run::Type {
                    commands: v,
                    missing,
                }),
                // None for this system: a plain shell, with the note if any.
                Ok(None) => missing.map(|m| Run::Type {
                    commands: Vec::new(),
                    missing: Some(m),
                }),
            }
        } else if let Some(e) = &n.exec {
            let argv = e.get_ref().clone();
            if argv.first().is_none_or(|a| a.trim().is_empty()) {
                self.problem(
                    e.span(),
                    "`exec` needs the program first: exec = [\"htop\"]",
                );
            }
            Some(Run::Exec { argv })
        } else {
            None
        };
        if !cwd.is_dir() {
            let span = n.cwd.as_ref().map_or(node.span(), Spanned::span);
            self.problem(span, format!("no directory {}", cwd.display()));
        }
        TemplateTree::Leaf {
            cwd: cwd.to_string_lossy().into_owned(),
            run,
        }
    }

    /// Each pane's share of the split (adding up to 1), from the sizes
    /// given; panes without one share what's left equally.
    fn shares(&mut self, panes: &[Spanned<Node>], span: Range<usize>) -> Vec<f64> {
        let mut given = 0.0;
        let mut open = 0usize;
        let mut ok = true;
        for p in panes {
            match &p.get_ref().size {
                Some(s) => {
                    let v = *s.get_ref();
                    if !(MIN_SIZE..=MAX_SIZE).contains(&v) {
                        self.problem(
                            s.span(),
                            format!("`size` is a percent from {MIN_SIZE} to {MAX_SIZE}, not {v}"),
                        );
                        ok = false;
                    }
                    given += v;
                }
                None => open += 1,
            }
        }
        if ok {
            if given > 100.5 || (open > 0 && given >= 100.0) {
                self.problem(span, format!("the sizes add up to {given}, more than 100"));
                ok = false;
            } else if open == 0 && (given - 100.0).abs() > 0.5 {
                self.problem(span, format!("the sizes add up to {given}, not 100"));
                ok = false;
            } else if open > 0 && (100.0 - given) / (open as f64) < MIN_SIZE {
                self.problem(
                    span,
                    format!(
                        "the sizes leave {}% for {open} pane(s); each needs at least {MIN_SIZE}%",
                        100.0 - given
                    ),
                );
                ok = false;
            }
        }
        if !ok {
            return vec![1.0 / panes.len() as f64; panes.len()];
        }
        let rest = if open > 0 {
            (100.0 - given) / open as f64
        } else {
            0.0
        };
        let raw: Vec<f64> = panes
            .iter()
            .map(|p| p.get_ref().size.as_ref().map_or(rest, |s| *s.get_ref()))
            .collect();
        let total: f64 = raw.iter().sum();
        raw.iter().map(|v| v / total).collect()
    }
}

/// Panes side by side (or stacked) with the given shares, as nested
/// binary splits: the first pane against the rest, and so on.
fn chain(dir: Dir, mut kids: Vec<TemplateTree>, shares: &[f64]) -> TemplateTree {
    if kids.len() == 1 {
        return kids.pop().unwrap();
    }
    let total: f64 = shares.iter().sum();
    let first = kids.remove(0);
    TemplateTree::Split {
        dir,
        ratio: shares[0] / total,
        a: Box::new(first),
        b: Box::new(chain(dir, kids, &shares[1..])),
    }
}

/// `raw` made absolute: `~` is home, a relative path starts at `base`.
/// `.` and `..` are taken out. `None` for `~` without a home.
pub fn resolve_path(raw: &str, base: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let p = if raw == "~" {
        home?.to_path_buf()
    } else if let Some(rest) = raw.strip_prefix("~/").or_else(|| {
        if cfg!(windows) {
            raw.strip_prefix("~\\")
        } else {
            None
        }
    }) {
        home?.join(rest)
    } else if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        base.join(raw)
    };
    Some(normalize(&p))
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Where relative paths start when the file has no `root`: the directory
/// holding `.kibann/` for a project's file, else where it's opened from.
fn default_root(file_dir: Option<&Path>, opened_from: &Path) -> PathBuf {
    match file_dir {
        Some(d) if d.file_name().is_some_and(|n| n == ".kibann") => {
            d.parent().unwrap_or(d).to_path_buf()
        }
        _ => opened_from.to_path_buf(),
    }
}

/// Read a layout file: every problem in it, or the layout to open.
pub fn parse(src: &Source<'_>) -> Result<Template, Problems> {
    let doc: Doc = match toml::from_str(src.text) {
        Ok(d) => d,
        Err(e) => {
            let (line, col) = position(src.text, e.span().map_or(0, |s| s.start));
            return Err(Problems(vec![Problem {
                line,
                col,
                message: e.message().trim().to_owned(),
            }]));
        }
    };
    let mut cx = Ctx {
        text: src.text,
        home: src.home,
        problems: Vec::new(),
        panes: 0,
    };
    let file = src.file.map(|f| src.opened_from.join(f));
    let file_dir = file.as_deref().and_then(Path::parent);
    let root = match &doc.root {
        Some(r) => cx
            .path(r, file_dir.unwrap_or(src.opened_from))
            .unwrap_or_else(|| src.opened_from.to_path_buf()),
        None => default_root(file_dir, src.opened_from),
    };
    if doc.tabs.is_empty() {
        cx.problem(0..0, "a layout needs at least one [[tab]]");
    }
    if let Some(t) = doc.tabs.get(MAX_TABS) {
        cx.problem(
            t.span(),
            format!("a layout can have at most {MAX_TABS} tabs"),
        );
    }
    let mut tabs = Vec::new();
    let mut active: Option<(usize, Range<usize>)> = None;
    for (i, t) in doc.tabs.iter().take(MAX_TABS).enumerate() {
        let n = t.get_ref();
        if let Some(s) = &n.size {
            cx.problem(s.span(), "`size` is for panes in a split, not tabs");
        }
        if let Some(f) = n.focus.as_ref().filter(|f| *f.get_ref()) {
            if active.is_some() {
                cx.problem(f.span(), "only one tab can have `focus = true`");
            } else {
                active = Some((i, f.span()));
            }
        }
        let mut leaves = 0;
        let mut focus = None;
        let tree = cx.node(t, &root, true, &mut leaves, &mut focus);
        tabs.push(TabTemplate {
            title: n.title.as_ref().map(|t| t.get_ref().clone()),
            tree,
            focused: focus.map_or(0, |(i, _)| i as u32),
        });
    }
    if cx.panes > MAX_PANES {
        cx.problem(
            0..0,
            format!(
                "a layout can have at most {MAX_PANES} panes, and this one has {}",
                cx.panes
            ),
        );
    }
    if !cx.problems.is_empty() {
        let mut p = cx.problems;
        p.sort_by_key(|p| (p.line, p.col));
        return Err(Problems(p));
    }
    let name = doc.name.or_else(|| {
        file.as_deref()
            .and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
    });
    Ok(Template {
        name,
        tabs,
        active: active.map_or(0, |(i, _)| i as u32),
    })
}

// ---- Writing: the layout on screen as a file ----

/// A tab to write.
pub struct SaveTab<'a> {
    pub title: Option<&'a str>,
    pub tree: &'a Tree,
    pub focused: PaneId,
}

/// What's known about a pane being written.
#[derive(Debug, Clone, Default)]
pub struct SavePane {
    pub cwd: Option<String>,
    /// The command line to type, when something other than the shell runs.
    pub command: Option<String>,
}

pub struct SaveOptions<'a> {
    pub name: Option<&'a str>,
    /// Index of the tab shown.
    pub active: usize,
    /// Write directories under this one relative to it (for a project's
    /// `.kibann/layout.toml`).
    pub relative_to: Option<&'a Path>,
    /// Write directories under home as `~/...`.
    pub home: Option<&'a Path>,
}

/// A layout file for `tabs`, which [`parse`] reads back as the same tabs.
pub fn write(
    tabs: &[SaveTab<'_>],
    panes: &HashMap<PaneId, SavePane>,
    opts: &SaveOptions<'_>,
) -> String {
    let mut out = String::new();
    if let Some(n) = opts.name {
        out.push_str(&format!("name = {}\n", quote(n)));
    }
    let w = Writer { panes, opts };
    for (i, t) in tabs.iter().enumerate() {
        out.push_str(if out.is_empty() {
            "[[tab]]\n"
        } else {
            "\n[[tab]]\n"
        });
        if let Some(title) = t.title {
            out.push_str(&format!("title = {}\n", quote(title)));
        }
        if i == opts.active && i != 0 {
            out.push_str("focus = true\n");
        }
        let order = t.tree.panes();
        let dirs: Vec<Option<String>> = order.iter().map(|p| w.cwd(*p)).collect();
        // One directory for the whole tab when every pane shares it.
        let common = dirs
            .first()
            .cloned()
            .flatten()
            .filter(|d| dirs.iter().all(|x| x.as_deref() == Some(d)));
        if let Some(d) = &common {
            out.push_str(&format!("cwd = {}\n", quote(d)));
        }
        let focus = (order.first() != Some(&t.focused)).then_some(t.focused);
        match t.tree {
            Tree::Leaf { pane } => {
                if let Some(c) = &w.pane(*pane).command {
                    out.push_str(&format!("command = {}\n", quote(c)));
                }
            }
            Tree::Split { dir, .. } => {
                out.push_str(&format!("split = {}\n", quote(dir_name(*dir))));
                out.push_str("panes = ");
                w.panes(&mut out, t.tree, *dir, common.as_deref(), focus, 0);
                out.push('\n');
            }
        }
    }
    out
}

struct Writer<'a> {
    panes: &'a HashMap<PaneId, SavePane>,
    opts: &'a SaveOptions<'a>,
}

impl Writer<'_> {
    fn pane(&self, p: PaneId) -> SavePane {
        self.panes.get(&p).cloned().unwrap_or_default()
    }

    fn cwd(&self, p: PaneId) -> Option<String> {
        let d = self.pane(p).cwd?;
        Some(show_path(
            Path::new(&d),
            self.opts.relative_to,
            self.opts.home,
        ))
    }

    /// The `panes = [...]` list of a split in `dir`, flattening splits in
    /// the same direction into it.
    fn panes(
        &self,
        out: &mut String,
        tree: &Tree,
        dir: Dir,
        tab_cwd: Option<&str>,
        focus: Option<PaneId>,
        depth: usize,
    ) {
        let items = flatten(tree, dir, 1.0);
        let sizes = percents(&items.iter().map(|(_, s)| *s).collect::<Vec<_>>());
        let pad = "  ".repeat(depth + 1);
        out.push_str("[\n");
        for (k, (node, _)) in items.iter().enumerate() {
            let mut keys: Vec<String> = Vec::new();
            match node {
                Tree::Leaf { pane } => {
                    let cwd = self.cwd(*pane);
                    if let Some(c) = cwd.as_deref().filter(|c| Some(*c) != tab_cwd) {
                        keys.push(format!("cwd = {}", quote(c)));
                    }
                    if let Some(c) = &self.pane(*pane).command {
                        keys.push(format!("command = {}", quote(c)));
                    }
                    if let Some(s) = &sizes {
                        keys.push(format!("size = {}", s[k]));
                    }
                    if focus == Some(*pane) {
                        keys.push("focus = true".into());
                    }
                    out.push_str(&format!("{pad}{},\n", inline(&keys)));
                }
                Tree::Split { dir: d, .. } => {
                    keys.push(format!("split = {}", quote(dir_name(*d))));
                    if let Some(s) = &sizes {
                        keys.push(format!("size = {}", s[k]));
                    }
                    out.push_str(&format!("{pad}{{ {}, panes = ", keys.join(", ")));
                    self.panes(out, node, *d, tab_cwd, focus, depth + 1);
                    out.push_str(" },\n");
                }
            }
        }
        out.push_str(&"  ".repeat(depth));
        out.push(']');
    }
}

fn inline(keys: &[String]) -> String {
    if keys.is_empty() {
        "{}".into()
    } else {
        format!("{{ {} }}", keys.join(", "))
    }
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::Row => "right",
        Dir::Column => "down",
    }
}

/// The parts of a split in `dir`, splits in the same direction opened up
/// into it, each with its share of the whole.
fn flatten(tree: &Tree, dir: Dir, share: f64) -> Vec<(&Tree, f64)> {
    match tree {
        Tree::Split {
            dir: d,
            ratio,
            a,
            b,
            ..
        } if *d == dir => {
            let mut v = flatten(a, dir, share * ratio);
            v.extend(flatten(b, dir, share * (1.0 - ratio)));
            v
        }
        _ => vec![(tree, share)],
    }
}

/// Whole percents for `shares` (adding up to 100, each at least the
/// smallest size allowed), or `None` when they're all equal anyway.
fn percents(shares: &[f64]) -> Option<Vec<i64>> {
    let total: f64 = shares.iter().sum();
    let even = 100.0 / shares.len() as f64;
    if shares
        .iter()
        .all(|s| (s / total * 100.0 - even).abs() < 0.5)
    {
        return None;
    }
    let mut v: Vec<i64> = shares
        .iter()
        .map(|s| ((s / total * 100.0).round() as i64).clamp(MIN_SIZE as i64, MAX_SIZE as i64))
        .collect();
    let diff = 100 - v.iter().sum::<i64>();
    let biggest = (0..v.len()).max_by_key(|&i| v[i]).unwrap_or(0);
    v[biggest] += diff;
    Some(v)
}

/// A directory as a layout file writes it.
fn show_path(p: &Path, relative_to: Option<&Path>, home: Option<&Path>) -> String {
    let slashes = |p: &Path| {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    };
    if let Some(rest) = relative_to.and_then(|r| p.strip_prefix(r).ok()) {
        return if rest.as_os_str().is_empty() {
            ".".into()
        } else {
            slashes(rest)
        };
    }
    if let Some(rest) = home.and_then(|h| p.strip_prefix(h).ok()) {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", slashes(rest))
        };
    }
    p.to_string_lossy().into_owned()
}

/// A TOML basic string.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory with `subdirs` in it, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(subdirs: &[&str]) -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("kibann-template-{}-{n}", std::process::id()));
            for d in subdirs {
                std::fs::create_dir_all(root.join(d)).unwrap();
            }
            std::fs::create_dir_all(&root).unwrap();
            // Canonical, so comparisons don't trip on /var -> /private/var.
            Self(root.canonicalize().unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn parse_in(dir: &Path, text: &str) -> Result<Template, Problems> {
        parse(&Source {
            text,
            file: None,
            opened_from: dir,
            home: Some(dir),
        })
    }

    fn messages(r: Result<Template, Problems>) -> Vec<String> {
        r.unwrap_err()
            .0
            .into_iter()
            .map(|p| p.to_string())
            .collect()
    }

    fn cwd_of(t: &TemplateTree) -> &str {
        match t {
            TemplateTree::Leaf { cwd, .. } => cwd,
            _ => panic!("not a leaf"),
        }
    }

    const PLAN_EXAMPLE: &str = r#"
name = "myapp"
root = "."

[[tab]]
title = "agents"
split = "right"
panes = [
  { command = "claude", size = 60, focus = true },
  { split = "down", panes = [
      { command = "npm run dev" },
      { cwd = "docs" },
  ] },
]

[[tab]]
title = "git"
command = "lazygit"
"#;

    #[test]
    fn the_plans_example_opens() {
        let s = Scratch::new(&["docs"]);
        let t = parse_in(&s.0, PLAN_EXAMPLE).unwrap();
        assert_eq!(t.name.as_deref(), Some("myapp"));
        assert_eq!(t.tabs.len(), 2);
        assert_eq!(t.active, 0);
        let agents = &t.tabs[0];
        assert_eq!(agents.title.as_deref(), Some("agents"));
        assert_eq!(agents.focused, 0);
        let TemplateTree::Split { dir, ratio, a, b } = &agents.tree else {
            panic!()
        };
        assert_eq!(*dir, Dir::Row);
        assert!((ratio - 0.6).abs() < 1e-9);
        assert_eq!(
            **a,
            TemplateTree::Leaf {
                cwd: s.0.to_string_lossy().into_owned(),
                run: Some(Run::Type {
                    commands: vec!["claude".into()],
                    missing: None
                }),
            }
        );
        let TemplateTree::Split {
            dir,
            ratio,
            b: docs,
            ..
        } = &**b
        else {
            panic!()
        };
        assert_eq!(*dir, Dir::Column);
        assert!((ratio - 0.5).abs() < 1e-9);
        assert_eq!(cwd_of(docs), s.0.join("docs").to_string_lossy());
        assert_eq!(
            t.tabs[1].tree,
            TemplateTree::Leaf {
                cwd: s.0.to_string_lossy().into_owned(),
                run: Some(Run::Type {
                    commands: vec!["lazygit".into()],
                    missing: None
                }),
            }
        );
    }

    #[test]
    fn panes_without_a_size_share_the_rest() {
        let s = Scratch::new(&[]);
        let t = parse_in(
            &s.0,
            r#"
[[tab]]
split = "right"
panes = [{ size = 40 }, {}, {}]
"#,
        )
        .unwrap();
        let TemplateTree::Split { ratio, b, .. } = &t.tabs[0].tree else {
            panic!()
        };
        assert!((ratio - 0.4).abs() < 1e-9);
        let TemplateTree::Split { ratio, .. } = &**b else {
            panic!()
        };
        assert!((ratio - 0.5).abs() < 1e-9);
        assert_eq!(t.tabs[0].tree.leaves().len(), 3);
    }

    #[test]
    fn tables_of_panes_work_too() {
        let s = Scratch::new(&[]);
        let t = parse_in(
            &s.0,
            r#"
[[tab]]
split = "down"

[[tab.panes]]
command = "htop"

[[tab.panes]]
exec = ["top", "-o", "cpu"]
focus = true
"#,
        )
        .unwrap();
        assert_eq!(t.tabs[0].focused, 1);
        let leaves = t.tabs[0].tree.leaves();
        assert!(matches!(
            leaves[1],
            TemplateTree::Leaf { run: Some(Run::Exec { argv }), .. } if argv[0] == "top"
        ));
    }

    #[test]
    fn relative_cwds_start_at_the_parent() {
        let s = Scratch::new(&["web/src", "web/test"]);
        let t = parse_in(
            &s.0,
            r#"
[[tab]]
cwd = "web"
split = "right"
panes = [{ cwd = "src" }, { cwd = "./test/../test" }, { cwd = "~" }]
"#,
        )
        .unwrap();
        let leaves = t.tabs[0].tree.leaves();
        assert_eq!(cwd_of(leaves[0]), s.0.join("web/src").to_string_lossy());
        assert_eq!(cwd_of(leaves[1]), s.0.join("web/test").to_string_lossy());
        assert_eq!(cwd_of(leaves[2]), s.0.to_string_lossy());
    }

    #[test]
    fn a_projects_file_opens_in_the_project() {
        let s = Scratch::new(&[".kibann", "elsewhere"]);
        let file = s.0.join(".kibann/layout.toml");
        let t = parse(&Source {
            text: "[[tab]]\n",
            file: Some(&file),
            opened_from: &s.0.join("elsewhere"),
            home: None,
        })
        .unwrap();
        assert_eq!(cwd_of(&t.tabs[0].tree), s.0.to_string_lossy());
        assert_eq!(t.name.as_deref(), Some("layout"));
    }

    #[test]
    fn root_is_relative_to_the_file() {
        let s = Scratch::new(&["layouts", "proj", "here"]);
        let file = s.0.join("layouts/x.toml");
        let t = parse(&Source {
            text: "root = \"../proj\"\n[[tab]]\n",
            file: Some(&file),
            opened_from: &s.0.join("here"),
            home: None,
        })
        .unwrap();
        assert_eq!(cwd_of(&t.tabs[0].tree), s.0.join("proj").to_string_lossy());

        // Without a root, a file outside .kibann opens where it's opened.
        let t = parse(&Source {
            text: "[[tab]]\n",
            file: Some(&file),
            opened_from: &s.0.join("here"),
            home: None,
        })
        .unwrap();
        assert_eq!(cwd_of(&t.tabs[0].tree), s.0.join("here").to_string_lossy());
    }

    #[test]
    fn commands_by_system_and_alternatives() {
        let s = Scratch::new(&[]);
        let t = parse_in(
            &s.0,
            r#"
[[tab]]
split = "right"
panes = [
  { command = ["btop", "htop", "top"], missing = "brew install btop" },
  { command = { macos = "macmon", linux = ["nvtop", "nvidia-smi -l 2"], windows = "nvidia-smi -l 2" } },
  { command = { linux = "nvtop", default = "true" } },
  { command = { plan9 = "x" } },
]
"#,
        );
        // The last pane names a system that doesn't exist.
        let m = messages(t);
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(m[0].starts_with("8:15: no system `plan9`"), "{m:?}");

        let t = parse_in(
            &s.0,
            r#"
[[tab]]
split = "right"
panes = [
  { command = ["btop", "htop", "top"], missing = "brew install btop" },
  { command = { macos = "macmon", linux = ["nvtop", "nvidia-smi -l 2"], windows = "nvidia-smi -l 2" } },
  { command = { plan9 = "x", default = "y" }, missing = "no" },
]
"#,
        );
        assert!(t.is_err());

        let t = parse_in(
            &s.0,
            r#"
[[tab]]
split = "right"
panes = [
  { command = ["btop", "htop", "top"], missing = "brew install btop" },
  { command = { macos = "macmon", linux = ["nvtop", "nvidia-smi -l 2"], windows = "nvidia-smi -l 2" } },
  { command = { windows = "x" }, missing = "only on Windows" },
]
"#,
        )
        .unwrap();
        let leaves = t.tabs[0].tree.leaves();
        let run = |i: usize| match leaves[i] {
            TemplateTree::Leaf { run, .. } => run.clone(),
            _ => unreachable!(),
        };
        assert_eq!(
            run(0),
            Some(Run::Type {
                commands: vec!["btop".into(), "htop".into(), "top".into()],
                missing: Some("brew install btop".into())
            })
        );
        if cfg!(target_os = "macos") {
            assert_eq!(
                run(1),
                Some(Run::Type {
                    commands: vec!["macmon".into()],
                    missing: None
                })
            );
            assert_eq!(
                run(2),
                Some(Run::Type {
                    commands: vec![],
                    missing: Some("only on Windows".into())
                })
            );
        }
    }

    #[test]
    fn problems_name_the_line() {
        let s = Scratch::new(&[]);
        let m = messages(parse_in(
            &s.0,
            "[[tab]]\nsplit = \"right\"\npanes = [{ comand = \"x\" }, {}]\n",
        ));
        assert_eq!(m.len(), 1);
        assert!(m[0].starts_with("3:"), "{m:?}");
        assert!(m[0].contains("comand"), "{m:?}");

        let m = messages(parse_in(
            &s.0,
            "[[tab]]\nsplit = \"left\"\npanes = [{}, {}]\n",
        ));
        assert!(m[0].starts_with("2:9:"), "{m:?}");
        assert!(m[0].contains("right"), "{m:?}");

        let m = messages(parse_in(&s.0, "[[tab]\n"));
        assert!(m[0].starts_with("1:"), "{m:?}");
    }

    #[test]
    fn mistakes_are_all_reported() {
        let s = Scratch::new(&[]);
        let m = messages(parse_in(
            &s.0,
            r#"
[[tab]]
split = "right"
command = "x"
panes = [
  { size = 70, title = "no", command = "a", exec = ["b"] },
  { size = 40, cwd = "nope", focus = true },
  { panes = [{}, {}], focus = true },
]

[[tab]]
focus = true
size = 50

[[tab]]
focus = true
split = "down"
panes = [{ missing = "m" }]
"#,
        ));
        let want = [
            "4:11: a split has no `command`",
            "5:9: the sizes add up to 110, more than 100",
            "6:24: `title` is for tabs",
            "6:52: give `command`",
            "7:22: no directory",
            "8:3: `panes` needs `split",
            "8:31: `focus` is for a pane or a tab",
            "13:8: `size` is for panes",
            "16:9: only one tab",
            "18:9: a split needs two or more",
            "18:22: `missing` goes with `command`",
        ];
        for w in want {
            assert!(m.iter().any(|m| m.starts_with(w)), "no {w:?} in {m:#?}");
        }
    }

    #[test]
    fn limits() {
        let s = Scratch::new(&[]);
        assert!(messages(parse_in(&s.0, "name = \"x\"\n"))[0].contains("at least one [[tab]]"));
        let many = "[[tab]]\n".repeat(MAX_TABS + 1);
        assert!(messages(parse_in(&s.0, &many))[0].contains("at most 16 tabs"));
        let wide = format!(
            "[[tab]]\nsplit = \"right\"\npanes = [{}]\n",
            vec!["{}"; 33].join(",")
        );
        let m = messages(parse_in(&s.0, &wide));
        assert!(m.iter().any(|m| m.contains("at most 32 panes")), "{m:?}");
    }

    #[test]
    fn sizes_must_add_up() {
        let s = Scratch::new(&[]);
        let m = messages(parse_in(
            &s.0,
            "[[tab]]\nsplit = \"right\"\npanes = [{ size = 50 }, { size = 30 }]\n",
        ));
        assert!(m[0].contains("add up to 80"), "{m:?}");
        let m = messages(parse_in(
            &s.0,
            "[[tab]]\nsplit = \"right\"\npanes = [{ size = 2 }, {}]\n",
        ));
        assert!(m[0].contains("from 5 to 95, not 2"), "{m:?}");
    }

    fn roundtrip(tree: Tree, focused: PaneId, dirs: &[(PaneId, &str, Option<&str>)], s: &Scratch) {
        let panes: HashMap<PaneId, SavePane> = dirs
            .iter()
            .map(|(p, d, c)| {
                (
                    *p,
                    SavePane {
                        cwd: Some(normalize(&s.0.join(d)).to_string_lossy().into_owned()),
                        command: c.map(str::to_owned),
                    },
                )
            })
            .collect();
        let text = write(
            &[SaveTab {
                title: Some("t \"one\""),
                tree: &tree,
                focused,
            }],
            &panes,
            &SaveOptions {
                name: Some("saved"),
                active: 0,
                relative_to: Some(&s.0),
                home: None,
            },
        );
        let t = parse_in(&s.0, &text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(t.name.as_deref(), Some("saved"));
        assert_eq!(t.tabs[0].title.as_deref(), Some("t \"one\""));
        // Same panes in the same places: compare each pane's box.
        let want = boxes_of_tree(&tree, (0.0, 0.0, 1.0, 1.0));
        let got = boxes_of_template(&t.tabs[0].tree, (0.0, 0.0, 1.0, 1.0));
        assert_eq!(want.len(), got.len(), "{text}");
        for (w, (g, cwd, run)) in want.iter().zip(&got) {
            let (pane, wb) = w;
            for k in 0..4 {
                assert!((wb[k] - g[k]).abs() < 0.011, "{text}\n{wb:?} vs {g:?}");
            }
            let (_, d, c) = dirs.iter().find(|(p, ..)| p == pane).unwrap();
            assert_eq!(*cwd, normalize(&s.0.join(d)).to_string_lossy());
            assert_eq!(
                run.clone(),
                c.map(|c| Run::Type {
                    commands: vec![c.into()],
                    missing: None
                })
            );
        }
        let order = tree.panes();
        let at = order.iter().position(|p| *p == focused).unwrap();
        assert_eq!(t.tabs[0].focused as usize, at, "{text}");
    }

    type Box4 = [f64; 4];
    type Rect = (f64, f64, f64, f64);

    fn cut(dir: Dir, r: f64, (x, y, w, h): Rect) -> (Rect, Rect) {
        match dir {
            Dir::Row => ((x, y, w * r, h), (x + w * r, y, w * (1.0 - r), h)),
            Dir::Column => ((x, y, w, h * r), (x, y + h * r, w, h * (1.0 - r))),
        }
    }

    fn boxes_of_tree(t: &Tree, b: Rect) -> Vec<(PaneId, Box4)> {
        match t {
            Tree::Leaf { pane } => vec![(*pane, [b.0, b.1, b.2, b.3])],
            Tree::Split {
                dir,
                ratio,
                a,
                b: bb,
            } => {
                let (l, r) = cut(*dir, *ratio, b);
                let mut v = boxes_of_tree(a, l);
                v.extend(boxes_of_tree(bb, r));
                v
            }
        }
    }

    fn boxes_of_template(t: &TemplateTree, b: Rect) -> Vec<(Box4, String, Option<Run>)> {
        match t {
            TemplateTree::Leaf { cwd, run } => {
                vec![([b.0, b.1, b.2, b.3], cwd.clone(), run.clone())]
            }
            TemplateTree::Split {
                dir,
                ratio,
                a,
                b: bb,
            } => {
                let (l, r) = cut(*dir, *ratio, b);
                let mut v = boxes_of_template(a, l);
                v.extend(boxes_of_template(bb, r));
                v
            }
        }
    }

    fn split(dir: Dir, ratio: f64, a: Tree, b: Tree) -> Tree {
        Tree::Split {
            dir,
            ratio,
            a: Box::new(a),
            b: Box::new(b),
        }
    }

    #[test]
    fn saved_layouts_read_back_the_same() {
        let s = Scratch::new(&["a", "b/c"]);
        // Split right, then the right side down, then its top right again.
        let tree = split(
            Dir::Row,
            0.6,
            Tree::leaf(1),
            split(
                Dir::Column,
                0.3,
                split(Dir::Row, 0.5, Tree::leaf(2), Tree::leaf(3)),
                Tree::leaf(4),
            ),
        );
        roundtrip(
            tree,
            3,
            &[
                (1, ".", Some("claude")),
                (2, "a", None),
                (3, "b/c", Some("npm run \"dev\"")),
                (4, ".", None),
            ],
            &s,
        );
        // The left side split again to the right: one list of three.
        let tree = split(
            Dir::Row,
            0.5,
            split(Dir::Row, 0.5, Tree::leaf(1), Tree::leaf(2)),
            Tree::leaf(3),
        );
        roundtrip(
            tree,
            1,
            &[(1, "a", None), (2, "a", None), (3, "a", None)],
            &s,
        );
        roundtrip(Tree::leaf(9), 9, &[(9, "b/c", Some("htop"))], &s);
    }

    #[test]
    fn writes_what_people_would() {
        let s = Scratch::new(&[]);
        let home = s.0.join("home");
        let tree = split(
            Dir::Row,
            0.5,
            Tree::leaf(1),
            split(Dir::Column, 0.5, Tree::leaf(2), Tree::leaf(3)),
        );
        let panes = HashMap::from([
            (
                1,
                SavePane {
                    cwd: Some(home.join("src/app").to_string_lossy().into()),
                    command: Some("claude".into()),
                },
            ),
            (
                2,
                SavePane {
                    cwd: Some(home.join("src/app").to_string_lossy().into()),
                    command: None,
                },
            ),
            (
                3,
                SavePane {
                    cwd: Some(home.join("src/app").to_string_lossy().into()),
                    command: Some("npm run dev".into()),
                },
            ),
        ]);
        let text = write(
            &[
                SaveTab {
                    title: None,
                    tree: &tree,
                    focused: 1,
                },
                SaveTab {
                    title: Some("git"),
                    tree: &Tree::leaf(1),
                    focused: 1,
                },
            ],
            &panes,
            &SaveOptions {
                name: Some("app"),
                active: 1,
                relative_to: None,
                home: Some(&home),
            },
        );
        assert_eq!(
            text,
            r#"name = "app"

[[tab]]
cwd = "~/src/app"
split = "right"
panes = [
  { command = "claude" },
  { split = "down", panes = [
    {},
    { command = "npm run dev" },
  ] },
]

[[tab]]
title = "git"
focus = true
cwd = "~/src/app"
command = "claude"
"#
        );
    }

    #[test]
    fn paths_resolve() {
        let base = Path::new("/a/b");
        let home = Path::new("/h");
        assert_eq!(
            resolve_path("c/../d/.", base, None).unwrap(),
            Path::new("/a/b/d")
        );
        assert_eq!(
            resolve_path("../../..", base, None).unwrap(),
            Path::new("/")
        );
        assert_eq!(
            resolve_path("~/x", base, Some(home)).unwrap(),
            Path::new("/h/x")
        );
        assert_eq!(
            resolve_path("~", base, Some(home)).unwrap(),
            Path::new("/h")
        );
        assert_eq!(resolve_path("~", base, None), None);
        assert_eq!(resolve_path("/abs", base, None).unwrap(), Path::new("/abs"));
        assert_eq!(
            resolve_path("~x", base, None).unwrap(),
            Path::new("/a/b/~x")
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("a\"b\\c\nd\u{1}"), r#""a\"b\\c\nd\u0001""#);
    }
}
