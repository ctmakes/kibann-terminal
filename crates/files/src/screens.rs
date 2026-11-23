//! Screens and parts (see [`proto::screen`]): where they're found (the
//! user's in the config folder, else the ones Kibann comes with), reading
//! and checking them, serving a part's files to the app, and what each
//! part was allowed to do.
//!
//! The app shows them; `kibann screen` lists, checks and copies them.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};

use proto::config::valid_name;
use proto::screen::{
    Content, Node, NodeKind, PartManifest, PartNow, Permissions, STYLE_PROPERTIES, Screen,
    ScreenEntry, ScreenNow, Size,
};

mod builtin {
    include!(concat!(env!("OUT_DIR"), "/builtin_files.rs"));
}

/// Parts drawn by the app itself rather than a web page, by name, with
/// what they are. Each can be in a screen once.
pub const NATIVE_PARTS: &[(&str, &str)] = &[
    (
        "sidebar",
        "Kibann's sidebar: every pane, grouped by repository, with its agent",
    ),
    (
        "tabs",
        "Kibann's tab bar (shown when there's more than one tab)",
    ),
];

/// The file every part's page gets first: the `kibann` object.
pub const LIBRARY: &str = "kibann.js";
/// Its types, for agents writing a part.
pub const TYPES: &str = "kibann.d.ts";

/// A file Kibann comes with, by its path from the repository's root
/// (`screens/board/screen.json`, `parts/kibann.js`).
pub fn builtin_file(path: &str) -> Option<&'static [u8]> {
    builtin::FILES
        .iter()
        .find(|(p, _)| *p == path)
        .map(|(_, b)| *b)
}

/// A built-in file to serve. A debug build reads it from the repository
/// when it's there, so edits to Kibann's own parts show without a rebuild.
fn served_builtin(path: &str) -> Option<Cow<'static, [u8]>> {
    let embedded = builtin_file(path)?;
    if cfg!(debug_assertions) {
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path);
        if let Ok(b) = std::fs::read(file) {
            return Some(Cow::Owned(b));
        }
    }
    Some(Cow::Borrowed(embedded))
}

fn builtin_text(path: &str) -> Option<&'static str> {
    builtin_file(path).and_then(|b| std::str::from_utf8(b).ok())
}

/// The names under `dir/` (`screens` or `parts`) that Kibann comes with.
fn builtin_names(dir: &str, file: &str) -> Vec<&'static str> {
    let mut v: Vec<&str> = builtin::FILES
        .iter()
        .filter_map(|(p, _)| {
            let rest = p.strip_prefix(dir)?.strip_prefix('/')?;
            let (name, f) = rest.split_once('/')?;
            (f == file).then_some(name)
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Where a screen or part is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    /// Its folder.
    User(PathBuf),
}

fn user_folder(dir: Option<PathBuf>, name: &str, file: &str) -> Option<PathBuf> {
    let d = dir?.join(name);
    d.join(file).is_file().then_some(d)
}

// ---- Screens ----

/// Every screen: the user's, then the built-in ones they don't replace.
pub fn list() -> Vec<ScreenEntry> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    if let Some(dir) = proto::paths::screens_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join("screen.json").is_file())
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| valid_name(n))
            .collect();
        names.sort();
        for name in names {
            let path = dir.join(&name).join("screen.json");
            let about = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| parse(&t).ok())
                .and_then(|s| s.about);
            seen.insert(name.clone());
            out.push(ScreenEntry {
                name,
                about,
                builtin: false,
                path: Some(path.to_string_lossy().into_owned()),
            });
        }
    }
    for name in builtin_names("screens", "screen.json") {
        if seen.contains(name) {
            continue;
        }
        let about = builtin_text(&format!("screens/{name}/screen.json"))
            .and_then(|t| parse(t).ok())
            .and_then(|s| s.about);
        out.push(ScreenEntry {
            name: name.to_owned(),
            about,
            builtin: true,
            path: None,
        });
    }
    out
}

/// The screen named `name`, its text and where it is.
pub fn find(name: &str) -> Result<(String, Origin), String> {
    if !valid_name(name) {
        return Err(format!(
            "{name:?} can't be a screen's name (lowercase letters, digits and dashes)"
        ));
    }
    if let Some(d) = user_folder(proto::paths::screens_dir(), name, "screen.json") {
        let text = std::fs::read_to_string(d.join("screen.json"))
            .map_err(|e| format!("{}: {e}", d.join("screen.json").display()))?;
        return Ok((text, Origin::User(d)));
    }
    builtin_text(&format!("screens/{name}/screen.json"))
        .map(|t| (t.to_owned(), Origin::Builtin))
        .ok_or_else(|| format!("no screen named {name} (kibann screen list shows them)"))
}

/// A screen's text, read: `line:column: what's wrong` when it can't be.
pub fn parse(text: &str) -> Result<Screen, String> {
    serde_json::from_str(text).map_err(|e| {
        let msg = e.to_string();
        let msg = match msg.rfind(" at line ") {
            Some(i) => &msg[..i],
            None => &msg,
        };
        format!("{}:{}: {msg}", e.line(), e.column())
    })
}

/// Everything wrong with `screen` that reading it didn't catch, given
/// which parts there are (`part` says why one can't be used).
pub fn check(screen: &Screen, part: &dyn Fn(&str) -> Result<(), String>) -> Vec<String> {
    let mut c = Checker {
        problems: Vec::new(),
        ids: HashSet::new(),
        terminals: 0,
        natives: HashSet::new(),
        part,
    };
    if screen.root.hidden {
        c.problems.push("root: can't be hidden".into());
    }
    c.node(&screen.root, "root", None);
    for (i, f) in screen.floating.iter().enumerate() {
        let at = format!("floating[{i}] ({})", f.id);
        c.id(&f.id, &at);
        c.content(&f.content, &at);
        c.style(&f.style, &at);
        if !(f.depth.is_finite() && f.depth > 0.0 && f.depth <= 4000.0) {
            c.problems
                .push(format!("{at}: depth must be 1 to 4000 pixels"));
        }
        if let Some(l) = f.length
            && !(l.is_finite() && l > 0.0 && l <= 8000.0)
        {
            c.problems
                .push(format!("{at}: length must be 1 to 8000 pixels"));
        }
        if !(f.peek.is_finite() && f.peek >= 0.0 && f.peek <= f.depth) {
            c.problems
                .push(format!("{at}: peek must be 0 to its depth"));
        }
    }
    for (k, v) in &screen.chrome.colors {
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            c.problems.push(format!("chrome.colors: bad name {k:?}"));
        }
        if !safe_css(v) {
            c.problems
                .push(format!("chrome.colors.{k}: not a color: {v:?}"));
        }
    }
    for v in [&screen.chrome.background, &screen.chrome.font]
        .into_iter()
        .flatten()
    {
        if !safe_css(v) {
            c.problems.push(format!("chrome: can't use {v:?}"));
        }
    }
    c.problems
}

/// A CSS value that can't reach outside the window or escape its
/// property: no `url(`, no `;` or braces.
fn safe_css(v: &str) -> bool {
    let low = v.to_ascii_lowercase();
    v.len() <= 300
        && !v.contains([';', '{', '}', '<', '>', '\\', '\n'])
        && !low.contains("url(")
        && !low.contains("image-set(")
        && !low.contains("expression(")
        && !low.contains("@import")
}

struct Checker<'a> {
    problems: Vec<String>,
    ids: HashSet<String>,
    terminals: usize,
    natives: HashSet<String>,
    part: &'a dyn Fn(&str) -> Result<(), String>,
}

impl Checker<'_> {
    fn id(&mut self, id: &str, at: &str) {
        if id.is_empty()
            || id.len() > 64
            || !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            self.problems
                .push(format!("{at}: id {id:?} must be letters, digits, - and _"));
        } else if !self.ids.insert(id.to_owned()) {
            self.problems
                .push(format!("{at}: another node has the id {id:?}"));
        }
    }

    fn style(&mut self, style: &BTreeMap<String, String>, at: &str) {
        for (k, v) in style {
            if !STYLE_PROPERTIES.contains(&k.as_str()) {
                self.problems.push(format!(
                    "{at}: style can't set {k:?} (it can set {})",
                    STYLE_PROPERTIES.join(", ")
                ));
            } else if !safe_css(v) {
                self.problems
                    .push(format!("{at}: style {k} can't be {v:?}"));
            }
        }
    }

    fn node(&mut self, n: &Node, at: &str, parent: Option<NodeKind>) {
        let at = match &n.id {
            Some(id) => format!("{at} ({id})"),
            None => at.to_owned(),
        };
        if let Some(id) = &n.id {
            self.id(id, &at);
        }
        self.style(&n.style, &at);
        match n.size {
            Some(Size::Px { px }) if !(px.is_finite() && (0.0..=10000.0).contains(&px)) => {
                self.problems.push(format!("{at}: px must be 0 to 10000"));
            }
            Some(Size::Weight { weight })
                if !(weight.is_finite() && weight > 0.0 && weight <= 1000.0) =>
            {
                self.problems
                    .push(format!("{at}: weight must be more than 0, up to 1000"));
            }
            _ => {}
        }
        for (what, v) in [("min", n.min), ("max", n.max)] {
            if let Some(v) = v
                && !(v.is_finite() && (0.0..=10000.0).contains(&v))
            {
                self.problems
                    .push(format!("{at}: {what} must be 0 to 10000"));
            }
        }
        if let (Some(a), Some(b)) = (n.min, n.max)
            && a > b
        {
            self.problems.push(format!("{at}: min is more than max"));
        }
        if n.resizable {
            if !matches!(n.size, Some(Size::Px { .. })) {
                self.problems
                    .push(format!("{at}: resizable needs a px size"));
            }
            if n.id.is_none() {
                self.problems.push(format!(
                    "{at}: resizable needs an id (its size is kept by it)"
                ));
            }
            if !matches!(parent, Some(NodeKind::Row | NodeKind::Column)) {
                self.problems
                    .push(format!("{at}: only a row's or column's child is resizable"));
            }
        }
        match n.kind {
            NodeKind::Block => {
                if !n.children.is_empty() {
                    self.problems
                        .push(format!("{at}: a block has no children (it has content)"));
                }
                if n.active.is_some() {
                    self.problems.push(format!("{at}: active is a stack's"));
                }
                match &n.content {
                    Some(c) => self.content(c, &at),
                    None => self.problems.push(format!(
                        "{at}: a block needs content: {{\"tab\": \"active\"}}, {{\"pane\": \"focused\"}} or {{\"part\": NAME}}"
                    )),
                }
            }
            kind => {
                if n.content.is_some() {
                    self.problems.push(format!("{at}: content is a block's"));
                }
                if n.children.is_empty() {
                    self.problems.push(format!("{at}: needs children"));
                }
                if kind != NodeKind::Stack && n.active.is_some() {
                    self.problems.push(format!("{at}: active is a stack's"));
                }
                if let Some(a) = &n.active
                    && !n.children.iter().any(|c| c.id.as_deref() == Some(a))
                {
                    self.problems
                        .push(format!("{at}: no child has the id {a:?} (active)"));
                }
                for (i, ch) in n.children.iter().enumerate() {
                    self.node(ch, &format!("{at}.children[{i}]"), Some(kind));
                }
            }
        }
    }

    fn content(&mut self, c: &Content, at: &str) {
        let set = [c.tab.is_some(), c.pane.is_some(), c.part.is_some()]
            .iter()
            .filter(|x| **x)
            .count();
        if set != 1 {
            self.problems
                .push(format!("{at}: content is one of tab, pane or part"));
            return;
        }
        if c.props.is_some() && c.part.is_none() {
            self.problems.push(format!("{at}: props are a part's"));
        }
        if let Some(t) = &c.tab {
            if t != "active" {
                self.problems.push(format!("{at}: tab can be \"active\""));
            }
            self.terminals += 1;
        }
        if let Some(p) = &c.pane {
            if p != "focused" {
                self.problems.push(format!("{at}: pane can be \"focused\""));
            }
            self.terminals += 1;
        }
        if self.terminals > 1 && (c.tab.is_some() || c.pane.is_some()) {
            self.problems.push(format!(
                "{at}: the terminals are in another block already (a screen shows them once)"
            ));
        }
        if let Some(name) = &c.part {
            if NATIVE_PARTS.iter().any(|(n, _)| n == name) {
                if !self.natives.insert(name.clone()) {
                    self.problems
                        .push(format!("{at}: {name} is in another block already"));
                }
            } else if let Err(e) = (self.part)(name) {
                self.problems.push(format!("{at}: {e}"));
            }
        }
    }
}

/// Every web part `screen` uses, once each, in order.
pub fn parts_of(screen: &Screen) -> Vec<String> {
    fn walk(n: &Node, out: &mut Vec<String>) {
        if let Some(p) = n.content.as_ref().and_then(|c| c.part.as_ref()) {
            out.push(p.clone());
        }
        for c in &n.children {
            walk(c, out);
        }
    }
    let mut all = Vec::new();
    walk(&screen.root, &mut all);
    all.extend(
        screen
            .floating
            .iter()
            .filter_map(|f| f.content.part.clone()),
    );
    let mut seen = HashSet::new();
    all.retain(|p| !NATIVE_PARTS.iter().any(|(n, _)| n == p) && seen.insert(p.clone()));
    all
}

/// Read and check the screen `name`.
pub fn read(name: &str) -> Result<(Screen, Origin), String> {
    let (text, origin) = find(name)?;
    let file = match &origin {
        Origin::User(d) => d.join("screen.json").to_string_lossy().into_owned(),
        Origin::Builtin => format!("{name} (built in)"),
    };
    let screen = parse(&text).map_err(|e| format!("{file}:{e}"))?;
    let problems = check(&screen, &|p| part(p).map(|_| ()));
    if !problems.is_empty() {
        return Err(format!("{file}: {}", problems.join("; ")));
    }
    Ok((screen, origin))
}

/// The screen `name` as the app shows it: its parts' pages are served
/// from `base` (`kpart://localhost/`).
pub fn now(name: &str, base: &str) -> Result<ScreenNow, String> {
    let (screen, _) = read(name)?;
    let mut parts = BTreeMap::new();
    for p in parts_of(&screen) {
        let (manifest, origin) = part(&p)?;
        let granted = granted(&origin, &manifest);
        parts.insert(
            p.clone(),
            PartNow {
                url: format!("{base}{p}/index.html"),
                manifest,
                granted,
            },
        );
    }
    Ok(ScreenNow {
        name: name.to_owned(),
        screen,
        parts,
        problem: String::new(),
    })
}

// ---- Parts ----

/// The part `name`'s manifest and where it is.
pub fn part(name: &str) -> Result<(PartManifest, Origin), String> {
    if !valid_name(name) {
        return Err(format!(
            "{name:?} can't be a part's name (lowercase letters, digits and dashes)"
        ));
    }
    if NATIVE_PARTS.iter().any(|(n, _)| *n == name) {
        return Err(format!("{name} is drawn by Kibann, not a page"));
    }
    let (text, origin, file) =
        if let Some(d) = user_folder(proto::paths::parts_dir(), name, "part.json") {
            let file = d.join("part.json");
            let text =
                std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            (text, Origin::User(d), file.to_string_lossy().into_owned())
        } else if let Some(t) = builtin_text(&format!("parts/{name}/part.json")) {
            (
                t.to_owned(),
                Origin::Builtin,
                format!("part {name} (built in)"),
            )
        } else {
            return Err(format!(
                "no part named {name} (kibann screen parts lists them)"
            ));
        };
    let m: PartManifest = serde_json::from_str(&text).map_err(|e| {
        let msg = e.to_string();
        let msg = msg.rfind(" at line ").map_or(&*msg, |i| &msg[..i]);
        format!("{file}:{}:{}: {msg}", e.line(), e.column())
    })?;
    for p in &m.permissions.exec {
        if !valid_program(p) {
            return Err(format!(
                "{file}: exec lists {p:?}; name programs (git), not paths or commands"
            ));
        }
    }
    for h in &m.permissions.net {
        if !valid_host(h) {
            return Err(format!(
                "{file}: net lists {h:?}; name hosts (api.github.com, *.example.com)"
            ));
        }
    }
    let has_page = match &origin {
        Origin::User(d) => d.join("index.html").is_file(),
        Origin::Builtin => builtin_file(&format!("parts/{name}/index.html")).is_some(),
    };
    if !has_page {
        return Err(format!("part {name} has no index.html"));
    }
    Ok((m, origin))
}

/// Every part: Kibann's own, the user's, then the built-in web parts they
/// don't replace. `(name, about, builtin)`.
pub fn list_parts() -> Vec<(String, Option<String>, bool)> {
    let mut out: Vec<(String, Option<String>, bool)> = NATIVE_PARTS
        .iter()
        .map(|(n, a)| ((*n).to_owned(), Some((*a).to_owned()), true))
        .collect();
    let mut seen = HashSet::new();
    if let Some(dir) = proto::paths::parts_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.path().join("part.json").is_file())
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|n| valid_name(n))
            .collect();
        names.sort();
        for n in names {
            let about = part(&n).ok().and_then(|(m, _)| m.about);
            seen.insert(n.clone());
            out.push((n, about, false));
        }
    }
    for n in builtin_names("parts", "part.json") {
        if !seen.contains(n) {
            let about = part(n).ok().and_then(|(m, _)| m.about);
            out.push((n.to_owned(), about, true));
        }
    }
    out
}

fn valid_program(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 64
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'))
        && !p.starts_with('.')
}

fn valid_host(h: &str) -> bool {
    let h = h.strip_prefix("*.").unwrap_or(h);
    !h.is_empty()
        && h.len() <= 253
        && h.contains('.')
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

/// What the part may do: everything its manifest asks for when it's
/// Kibann's or the user agreed to exactly those permissions, else nothing.
pub fn granted(origin: &Origin, m: &PartManifest) -> Permissions {
    if m.permissions.is_empty() || needs_asking(origin, m).is_none() {
        m.permissions.clone()
    } else {
        Permissions::default()
    }
}

fn permissions_digest(p: &Permissions) -> String {
    crate::workspaces::digest(&serde_json::to_string(p).unwrap_or_default())
}

/// What to ask before the part may do what it asks for: one line each.
/// `None` when nothing needs asking (it asks for nothing, it's built in,
/// or the user agreed to exactly these).
pub fn needs_asking(origin: &Origin, m: &PartManifest) -> Option<Vec<String>> {
    let Origin::User(dir) = origin else {
        return None;
    };
    if m.permissions.is_empty() {
        return None;
    }
    let store = proto::paths::trusted_file();
    let agreed = store
        .as_deref()
        .map(crate::workspaces::load_trusted)
        .and_then(|t| t.parts.get(&crate::workspaces::key(dir)).cloned());
    if agreed.as_deref() == Some(&permissions_digest(&m.permissions)) {
        return None;
    }
    Some(m.permissions.describe())
}

/// Remember that the part in `dir` may do what `m` asks.
pub fn trust(dir: &Path, m: &PartManifest) -> io::Result<()> {
    let Some(store) = proto::paths::trusted_file() else {
        return Ok(());
    };
    let digest = permissions_digest(&m.permissions);
    crate::workspaces::update_trusted(&store, |t| {
        t.parts.insert(crate::workspaces::key(dir), digest);
    })
}

/// A file of a part, as served to the app: `path` is what follows the
/// base URL (`board/index.html`, `kibann.js`). A page gets
/// [`LIBRARY`] loaded first. `None` when there's no such file (or the path
/// leaves the part's folder).
pub fn serve(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let path = path.trim_start_matches('/');
    let path = path.split(['?', '#']).next().unwrap_or("");
    let path = percent_decode(path)?;
    if path == LIBRARY || path == TYPES {
        return served_builtin(&format!("parts/{path}")).map(|b| (b, mime(&path)));
    }
    let (name, rest) = path.split_once('/')?;
    let rest = if rest.is_empty() { "index.html" } else { rest };
    if !valid_name(name)
        || !Path::new(rest)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let bytes: Cow<'static, [u8]> = match user_folder(proto::paths::parts_dir(), name, "part.json")
    {
        Some(dir) => {
            let file = dir.join(rest);
            // A link out of the folder isn't the part's.
            let real = file.canonicalize().ok()?;
            if !real.starts_with(dir.canonicalize().ok()?) || !real.is_file() {
                return None;
            }
            Cow::Owned(std::fs::read(real).ok()?)
        }
        None => served_builtin(&format!("parts/{name}/{rest}"))?,
    };
    let mime = mime(rest);
    if rest == "index.html" {
        return Some((Cow::Owned(with_library(&bytes)), mime));
    }
    Some((bytes, mime))
}

/// `page` with a script tag for [`LIBRARY`] at the start of its head.
fn with_library(page: &[u8]) -> Vec<u8> {
    let tag = format!("<script src=\"../{LIBRARY}\"></script>");
    let text = String::from_utf8_lossy(page);
    let low = text.to_ascii_lowercase();
    let at = low
        .find("<head")
        .and_then(|i| low[i..].find('>').map(|j| i + j + 1))
        .or_else(|| {
            low.find("<html")
                .and_then(|i| low[i..].find('>').map(|j| i + j + 1))
        })
        .unwrap_or_else(|| {
            // After a doctype, so the page keeps standards mode.
            if low.trim_start().starts_with("<!doctype") {
                low.find('>').map_or(0, |i| i + 1)
            } else {
                0
            }
        });
    let mut out = String::with_capacity(text.len() + tag.len());
    out.push_str(&text[..at]);
    out.push_str(&tag);
    out.push_str(&text[at..]);
    out.into_bytes()
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A file's type by its extension.
pub fn mime(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "txt" | "md" => "text/plain; charset=utf-8",
        "ts" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The Content-Security-Policy for a part's files: its own files, inline
/// script and style, images from data and blob URLs, and connections only
/// to the hosts it was allowed. `origin` is where parts are served from
/// (`kpart://localhost`).
pub fn csp(origin: &str, granted: &Permissions) -> String {
    let connect = if granted.net.is_empty() {
        "'none'".to_owned()
    } else {
        granted
            .net
            .iter()
            .map(|h| format!("https://{h} wss://{h}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "default-src {origin}; script-src {origin} 'unsafe-inline'; style-src {origin} 'unsafe-inline'; \
         img-src {origin} data: blob:; font-src {origin} data:; media-src {origin} data: blob:; \
         connect-src {connect}; frame-src 'none'; form-action 'none'; base-uri 'none'"
    )
}

/// What a part's permissions are, read from its folder in the request
/// (`board/index.html` → the part `board`), for its CSP.
pub fn granted_for_path(path: &str) -> Permissions {
    let name = path.trim_start_matches('/').split('/').next().unwrap_or("");
    part(name).map(|(m, o)| granted(&o, &m)).unwrap_or_default()
}

/// Copy screen `from` (the user's or built in) to the user's screens as
/// `to`, with any parts it uses that are built in left as they are. The
/// new folder.
pub fn copy(from: &str, to: &str, force: bool) -> Result<PathBuf, String> {
    if !valid_name(to) {
        return Err(format!(
            "{to:?} can't be a screen's name (lowercase letters, digits and dashes)"
        ));
    }
    let (text, origin) = find(from)?;
    let dir = proto::paths::screens_dir()
        .ok_or("no home directory for the screens folder")?
        .join(to);
    if dir.join("screen.json").exists() && !force {
        return Err(format!(
            "{} exists (--force replaces it)",
            dir.join("screen.json").display()
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    std::fs::write(dir.join("screen.json"), text).map_err(|e| e.to_string())?;
    // Its preview, when it has one.
    let preview: Option<Vec<u8>> = match origin {
        Origin::User(d) => std::fs::read(d.join("preview.png")).ok(),
        Origin::Builtin => builtin_file(&format!("screens/{from}/preview.png")).map(<[u8]>::to_vec),
    };
    if let Some(p) = preview {
        let _ = std::fs::write(dir.join("preview.png"), p);
    }
    Ok(dir)
}

/// Copy the built-in part `from` to the user's parts as `to`, to change
/// it. The new folder.
pub fn copy_part(from: &str, to: &str, force: bool) -> Result<PathBuf, String> {
    if !valid_name(to) || NATIVE_PARTS.iter().any(|(n, _)| *n == to) {
        return Err(format!("{to:?} can't be a part's name"));
    }
    let (_, origin) = part(from)?;
    let dir = proto::paths::parts_dir()
        .ok_or("no home directory for the parts folder")?
        .join(to);
    if dir.join("part.json").exists() && !force {
        return Err(format!(
            "{} exists (--force replaces it)",
            dir.join("part.json").display()
        ));
    }
    let files: Vec<(String, Vec<u8>)> = match origin {
        Origin::Builtin => {
            let prefix = format!("parts/{from}/");
            builtin::FILES
                .iter()
                .filter_map(|(p, b)| Some((p.strip_prefix(&prefix)?.to_owned(), b.to_vec())))
                .collect()
        }
        Origin::User(src) => {
            let mut all = Vec::new();
            collect(&src, &src, &mut all).map_err(|e| e.to_string())?;
            all
        }
    };
    for (rel, bytes) in files {
        let f = dir.join(&rel);
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&f, bytes).map_err(|e| format!("{}: {e}", f.display()))?;
    }
    Ok(dir)
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if e.file_type()?.is_dir() {
            collect(root, &p, out)?;
        } else if e.file_type()?.is_file() {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, std::fs::read(&p)?));
        }
    }
    Ok(())
}

/// When a screen or part last changed on disk: for noticing edits.
pub fn stamp() -> Vec<(PathBuf, Option<std::time::SystemTime>)> {
    let mut out = Vec::new();
    for dir in [proto::paths::screens_dir(), proto::paths::parts_dir()]
        .into_iter()
        .flatten()
    {
        walk_stamps(&dir, 0, &mut out);
    }
    out.sort();
    out
}

fn walk_stamps(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, Option<std::time::SystemTime>)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() && depth < 4 {
            walk_stamps(&p, depth + 1, out);
        } else if t.is_file() {
            let m = e.metadata().and_then(|m| m.modified()).ok();
            out.push((p, m));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_parts(p: &str) -> Result<(), String> {
        Err(format!("no part named {p}"))
    }

    fn problems(json: &str) -> Vec<String> {
        check(&parse(json).unwrap(), &no_parts)
    }

    #[test]
    fn every_builtin_screen_reads_and_checks() {
        let names = builtin_names("screens", "screen.json");
        assert!(names.contains(&"default"), "{names:?}");
        for n in names {
            let text = builtin_text(&format!("screens/{n}/screen.json")).unwrap();
            let s = parse(text).unwrap_or_else(|e| panic!("{n}: {e}"));
            let p = check(&s, &|p| {
                builtin_file(&format!("parts/{p}/part.json"))
                    .map(|_| ())
                    .ok_or_else(|| format!("no built-in part {p}"))
            });
            assert!(p.is_empty(), "{n}: {p:?}");
            assert!(s.about.is_some(), "{n} says what it's for");
        }
    }

    #[test]
    fn every_builtin_part_reads() {
        for n in builtin_names("parts", "part.json") {
            let text = builtin_text(&format!("parts/{n}/part.json")).unwrap();
            let m: PartManifest = serde_json::from_str(text).unwrap_or_else(|e| panic!("{n}: {e}"));
            assert!(m.about.is_some(), "{n} says what it is");
            assert!(
                builtin_file(&format!("parts/{n}/index.html")).is_some(),
                "{n}"
            );
        }
        assert!(builtin_file("parts/kibann.js").is_some());
        assert!(builtin_file("parts/kibann.d.ts").is_some());
    }

    /// The examples in the guide for agents read and check.
    #[test]
    fn the_guides_examples_are_right() {
        let guide = include_str!("../../../skills/kibann-screens/SKILL.md");
        let blocks: Vec<&str> = guide
            .split("```json\n")
            .skip(1)
            .map(|b| b.split("```").next().unwrap())
            .collect();
        assert_eq!(blocks.len(), 2, "a screen and a part.json");
        let screen = parse(blocks[0]).unwrap_or_else(|e| panic!("{e}"));
        let p = check(&screen, &|p| {
            builtin_file(&format!("parts/{p}/part.json"))
                .map(|_| ())
                .ok_or_else(|| format!("no built-in part {p}"))
        });
        assert!(p.is_empty(), "{p:?}");
        let m: PartManifest = serde_json::from_str(blocks[1]).unwrap();
        assert_eq!(m.permissions.exec, vec!["gh"]);
    }

    #[test]
    fn mistakes_say_where() {
        let e = parse("{\n  \"root\": {\"type\": \"block\", \"colour\": 1}\n}").unwrap_err();
        assert!(e.starts_with("2:"), "{e}");
        assert!(e.contains("unknown field `colour`"), "{e}");
        let e = parse("{\"root\": {\"type\": \"grid\"}}").unwrap_err();
        assert!(e.contains("unknown variant `grid`"), "{e}");
    }

    #[test]
    fn what_check_catches() {
        let p = problems(
            r#"{"root": {"type": "row", "children": [
                {"type": "block", "id": "a", "content": {"tab": "active"}},
                {"type": "block", "id": "a", "content": {"pane": "focused"}},
                {"type": "block", "content": {"part": "nope", "props": 1}},
                {"type": "block", "content": {"tab": "active", "part": "sidebar"}},
                {"type": "block"},
                {"type": "column", "children": []},
                {"type": "block", "resizable": true, "content": {"part": "sidebar"}},
                {"type": "block", "content": {"part": "sidebar"}, "style": {"position": "fixed", "background": "url(https://x)"}},
                {"type": "stack", "active": "zz", "children": [{"type": "block", "content": {"part": "tabs"}}]}
            ]}}"#,
        );
        let has = |s: &str| p.iter().any(|x| x.contains(s));
        assert!(has("another node has the id \"a\""), "{p:#?}");
        assert!(has("the terminals are in another block already"), "{p:#?}");
        assert!(has("no part named nope"), "{p:#?}");
        assert!(has("content is one of tab, pane or part"), "{p:#?}");
        assert!(has("a block needs content"), "{p:#?}");
        assert!(has("needs children"), "{p:#?}");
        assert!(has("resizable needs a px size"), "{p:#?}");
        assert!(has("resizable needs an id"), "{p:#?}");
        assert!(has("sidebar is in another block already"), "{p:#?}");
        assert!(has("style can't set \"position\""), "{p:#?}");
        assert!(has("style background can't be"), "{p:#?}");
        assert!(has("no child has the id \"zz\""), "{p:#?}");
    }

    #[test]
    fn a_plain_screen_is_fine() {
        let p = problems(
            r##"{"root": {"type": "row", "children": [
                {"type": "block", "size": "auto", "content": {"part": "sidebar"}},
                {"type": "column", "children": [
                    {"type": "block", "size": "auto", "content": {"part": "tabs"}},
                    {"type": "block", "content": {"tab": "active"}, "style": {"padding": "4px"}}
                ]}
            ]}, "chrome": {"colors": {"sidebar": "#101010"}}}"##,
        );
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn pages_get_the_library_first() {
        let page = with_library(b"<!doctype html><html><head><title>x</title></head></html>");
        assert_eq!(
            String::from_utf8(page).unwrap(),
            "<!doctype html><html><head><script src=\"../kibann.js\"></script><title>x</title></head></html>"
        );
        let page = with_library(b"<!DOCTYPE html><div>hi</div>");
        assert_eq!(
            String::from_utf8(page).unwrap(),
            "<!DOCTYPE html><script src=\"../kibann.js\"></script><div>hi</div>"
        );
    }

    #[test]
    fn serving_stays_in_the_part() {
        assert!(serve("kibann.js").is_some());
        assert!(serve("board/../../etc/passwd").is_none());
        assert!(serve("board/%2e%2e/x").is_none());
        assert!(serve("Board/index.html").is_none());
        let (page, mime) = serve("board/index.html").unwrap();
        assert!(mime.starts_with("text/html"));
        assert!(String::from_utf8_lossy(&page).contains("../kibann.js"));
    }

    #[test]
    fn csp_lets_out_only_what_was_allowed() {
        let none = csp("kpart://localhost", &Permissions::default());
        assert!(none.contains("connect-src 'none'"));
        let net = csp(
            "kpart://localhost",
            &Permissions {
                net: vec!["api.github.com".into()],
                ..Default::default()
            },
        );
        assert!(net.contains("connect-src https://api.github.com wss://api.github.com;"));
    }
}
