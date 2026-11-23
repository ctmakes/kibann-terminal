//! Workspace files around [`crate::workspace`]: where they're found (the
//! built-in ones, the user's, a project's), which may run their commands
//! without asking, and making one from the panes on screen. Shared by the
//! `kibann workspace` commands and the app.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::{Path, PathBuf};

use proto::layout::{Layout, PaneId, Run, TabId, Workspace, WorkspaceTree};
use proto::wire::PaneInfo;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::workspace::{self as file, Problems, SaveOptions, SavePane, SaveTab, Source};

/// The workspaces Kibann comes with, by name.
pub const BUILTINS: &[(&str, &str)] = &[
    ("system", include_str!("../../../workspaces/system.toml")),
    ("ports", include_str!("../../../workspaces/ports.toml")),
    ("agents", include_str!("../../../workspaces/agents.toml")),
    (
        "agents-4",
        include_str!("../../../workspaces/agents-4.toml"),
    ),
    ("review", include_str!("../../../workspaces/review.toml")),
];

/// A project's workspace, inside the project.
pub const PROJECT_FILE: &str = ".kibann/workspace.toml";

/// Where a workspace came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "WorkspaceOrigin")]
pub enum Origin {
    /// Comes with Kibann.
    Builtin,
    /// In the user's workspaces folder.
    User,
    /// A project's `.kibann/workspace.toml`.
    Project,
    /// Any other file, named by its path.
    File,
}

/// A workspace file, read.
#[derive(Debug, Clone)]
pub struct Found {
    pub name: String,
    pub origin: Origin,
    /// `None` for a built-in one.
    pub path: Option<PathBuf>,
    pub text: String,
}

fn builtin(name: &str) -> Option<&'static str> {
    BUILTINS.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

/// Whether `arg` names a file rather than a workspace.
fn is_path(arg: &str) -> bool {
    arg.contains('/') || arg.contains('\\') || arg.ends_with(".toml")
}

/// The project's workspace for `cwd`: `.kibann/workspace.toml` there or in a
/// directory above, up to the repository's root (or home, outside one).
pub fn project_workspace(cwd: &Path) -> Option<PathBuf> {
    let home = proto::paths::home_dir();
    for dir in cwd.ancestors() {
        let f = dir.join(PROJECT_FILE);
        if f.is_file() {
            return Some(f);
        }
        if dir.join(".git").exists() || home.as_deref() == Some(dir) {
            return None;
        }
    }
    None
}

/// The workspace `arg` names, read: a path, a name (the user's workspaces first,
/// then the built-in ones), or with no `arg` the project's.
pub fn find(arg: Option<&str>, cwd: &Path) -> Result<Found, String> {
    let read = |path: PathBuf, name: String, origin: Origin| {
        std::fs::read_to_string(&path)
            .map(|text| Found {
                name,
                origin,
                path: Some(path.clone()),
                text,
            })
            .map_err(|e| format!("can't read {}: {e}", path.display()))
    };
    match arg {
        None => {
            let path = project_workspace(cwd).ok_or_else(|| {
                format!(
                    "no {PROJECT_FILE} in {} or above it; name a workspace instead (kibann workspace list)",
                    cwd.display()
                )
            })?;
            let name = project_name(&path);
            read(path, name, Origin::Project)
        }
        Some(a) if is_path(a) => {
            let path = file::resolve_path(a, cwd, proto::paths::home_dir().as_deref())
                .unwrap_or_else(|| cwd.join(a));
            let origin = if path.ends_with(PROJECT_FILE) {
                Origin::Project
            } else if proto::paths::workspaces_dir().is_some_and(|d| path.parent() == Some(&*d)) {
                Origin::User
            } else {
                Origin::File
            };
            let name = if origin == Origin::Project {
                project_name(&path)
            } else {
                stem(&path)
            };
            read(path, name, origin)
        }
        Some(name) => {
            if let Some(dir) = proto::paths::workspaces_dir() {
                let path = dir.join(format!("{name}.toml"));
                if path.is_file() {
                    return read(path, name.to_owned(), Origin::User);
                }
            }
            match builtin(name) {
                Some(text) => Ok(Found {
                    name: name.to_owned(),
                    origin: Origin::Builtin,
                    path: None,
                    text: text.to_owned(),
                }),
                None => Err(format!(
                    "no workspace named {name} (kibann workspace list shows them)"
                )),
            }
        }
    }
}

fn stem(p: &Path) -> String {
    p.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A project's workspace is named after the project's directory.
fn project_name(path: &Path) -> String {
    path.parent()
        .and_then(Path::parent)
        .map(stem)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "project".into())
}

/// Read `f`'s workspace as opened from `cwd`.
pub fn parse(f: &Found, cwd: &Path) -> Result<Workspace, Problems> {
    let home = proto::paths::home_dir();
    let mut t = file::parse(&Source {
        text: &f.text,
        file: f.path.as_deref(),
        opened_from: cwd,
        home: home.as_deref(),
    })?;
    if t.name.is_none() || f.origin == Origin::Project {
        t.name = Some(f.name.clone());
    }
    Ok(t)
}

/// A workspace `kibann workspace list` shows.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(export, rename = "WorkspaceEntry")]
pub struct Listed {
    pub name: String,
    pub origin: Origin,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string")]
    pub path: Option<PathBuf>,
    /// The first comment line of the file, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub about: Option<String>,
}

fn about(text: &str) -> Option<String> {
    let mut lines = Vec::new();
    for l in text.lines() {
        match l.trim().strip_prefix('#') {
            Some(c) => lines.push(c.trim().to_owned()),
            None => break,
        }
    }
    let joined = lines.join(" ");
    // The first sentence is enough for a list.
    let first = match joined.find(". ") {
        Some(i) => &joined[..=i],
        None => &joined,
    };
    (!first.is_empty()).then(|| first.trim_end_matches(':').to_owned())
}

/// Every workspace there is for `cwd`: the project's first, then the user's,
/// then the built-in ones the user's don't replace.
pub fn list(cwd: &Path) -> Vec<Listed> {
    let mut out = Vec::new();
    if let Some(p) = project_workspace(cwd) {
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        out.push(Listed {
            name: project_name(&p),
            origin: Origin::Project,
            about: about(&text),
            path: Some(p),
        });
    }
    let mut mine = Vec::new();
    if let Some(dir) = proto::paths::workspaces_dir()
        && let Ok(rd) = std::fs::read_dir(dir)
    {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "toml") && p.is_file() {
                let text = std::fs::read_to_string(&p).unwrap_or_default();
                mine.push(Listed {
                    name: stem(&p),
                    origin: Origin::User,
                    about: about(&text),
                    path: Some(p),
                });
            }
        }
    }
    mine.sort_by(|a, b| a.name.cmp(&b.name));
    for (name, text) in BUILTINS {
        if !mine.iter().any(|m| m.name == *name) {
            mine.push(Listed {
                name: (*name).to_owned(),
                origin: Origin::Builtin,
                path: None,
                about: about(text),
            });
        }
    }
    out.extend(mine);
    out
}

// ---- Trust ----

/// What one pane of a workspace runs, for asking before running it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Runs {
    pub cwd: String,
    pub run: String,
}

/// Every command a workspace would run, in order.
pub fn commands(t: &Workspace) -> Vec<Runs> {
    let mut out = Vec::new();
    for tab in &t.tabs {
        for leaf in tab.tree.leaves() {
            let WorkspaceTree::Leaf {
                cwd,
                run: Some(run),
            } = leaf
            else {
                continue;
            };
            let what = match run {
                Run::Type { commands, .. } if commands.is_empty() => continue,
                Run::Type { commands, .. } => commands.join("  or  "),
                Run::Exec { argv } => shell_words(argv),
            };
            out.push(Runs {
                cwd: cwd.clone(),
                run: what,
            });
        }
    }
    out
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Trusted {
    /// File path to the SHA-256 of the contents the user agreed to.
    #[serde(default)]
    pub(crate) files: BTreeMap<String, String>,
    /// A part's folder to the SHA-256 of the permissions the user allowed
    /// it (see `crate::screens`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) parts: BTreeMap<String, String>,
}

pub(crate) fn digest(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub(crate) fn load_trusted(file: &Path) -> Trusted {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Whether opening `f` must ask first: it runs commands, and isn't the
/// user's own (built-in, in their workspaces folder, or agreed to before in
/// exactly this version).
pub fn needs_asking(f: &Found, t: &Workspace) -> bool {
    needs_asking_in(f, t, proto::paths::trusted_file().as_deref())
}

fn needs_asking_in(f: &Found, t: &Workspace, store: Option<&Path>) -> bool {
    if matches!(f.origin, Origin::Builtin | Origin::User) || commands(t).is_empty() {
        return false;
    }
    let (Some(path), Some(store)) = (&f.path, store) else {
        return true;
    };
    load_trusted(store).files.get(&key(path)) != Some(&digest(&f.text))
}

/// Remember that `path`, as it is now (`text`), may run its commands.
/// Write a workspace the user saved (from what is open), and trust it:
/// opening it again needn't ask about its commands.
pub fn save(path: &Path, text: &str) -> io::Result<()> {
    crate::write_whole(path, text)?;
    trust(path, text)
}

pub fn trust(path: &Path, text: &str) -> io::Result<()> {
    match proto::paths::trusted_file() {
        Some(store) => trust_in(&store, path, text),
        None => Ok(()),
    }
}

fn trust_in(store: &Path, path: &Path, text: &str) -> io::Result<()> {
    update_trusted(store, |t| {
        t.files.insert(key(path), digest(text));
    })
}

/// Change what's kept in `store`.
pub(crate) fn update_trusted(store: &Path, change: impl FnOnce(&mut Trusted)) -> io::Result<()> {
    let mut t = load_trusted(store);
    change(&mut t);
    if let Some(d) = store.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = store.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&t).map_err(io::Error::other)?,
    )?;
    std::fs::rename(tmp, store)
}

// ---- Saving ----

/// Programs not worth starting again when a workspace is opened: pagers and
/// viewers, Kibann's own, and shells (a shell in a shell is just a shell).
const NOT_SAVED: &[&str] = &[
    "less",
    "more",
    "most",
    "man",
    "kibann",
    "kibannd",
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "nu",
    "pwsh",
    "powershell",
    "cmd",
];

/// One argument as a POSIX shell reads it.
fn shell_word(a: &str) -> String {
    if !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c))
    {
        a.to_owned()
    } else {
        format!("'{}'", a.replace('\'', r"'\''"))
    }
}

/// `argv` as a POSIX shell command line.
pub fn shell_line(argv: &[String]) -> String {
    shell_words(argv)
}

fn shell_words(argv: &[String]) -> String {
    argv.iter()
        .map(|a| shell_word(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The command line that starts again what runs in `p`, if anything.
/// Programs by their name rather than their path; agents by their name
/// alone (what they were asked last isn't worth asking again).
pub fn saved_command(p: &PaneInfo) -> Option<String> {
    if let Some(agent) = &p.agent {
        return Some(agent.clone());
    }
    let argv = p.program.as_ref()?;
    let name = Path::new(argv.first()?)
        .file_name()?
        .to_string_lossy()
        .trim_start_matches('-')
        .to_owned();
    // `kibann watch` is worth starting again; its other commands (the
    // diff viewer) aren't.
    let watch =
        matches!(name.as_str(), "kibann" | "kibannd") && argv.get(1).is_some_and(|a| a == "watch");
    if watch {
        let mut words = vec!["kibann".to_owned()];
        words.extend(argv[1..].iter().cloned());
        return Some(shell_words(&words));
    }
    if name.is_empty() || NOT_SAVED.contains(&name.as_str()) {
        return None;
    }
    let mut words = vec![name];
    words.extend(argv[1..].iter().cloned());
    Some(shell_words(&words))
}

/// What to save, and how.
pub struct SaveSpec<'a> {
    pub name: Option<&'a str>,
    /// Only this tab; else every tab.
    pub tab: Option<TabId>,
    /// Directories under this one are written relative to it.
    pub relative_to: Option<&'a Path>,
}

/// The tabs on screen as a workspace file.
pub fn snapshot(
    layout: &Layout,
    panes: &[PaneInfo],
    spec: &SaveSpec<'_>,
) -> Result<String, String> {
    let tabs: Vec<_> = layout
        .tabs
        .iter()
        .filter(|t| spec.tab.is_none_or(|id| t.id == id))
        .collect();
    if tabs.is_empty() {
        return Err(match spec.tab {
            Some(t) => format!("no tab t{t}"),
            None => "there are no tabs to save".into(),
        });
    }
    let info: HashMap<PaneId, SavePane> = panes
        .iter()
        .map(|p| {
            (
                p.id,
                SavePane {
                    cwd: p.cwd.clone(),
                    command: saved_command(p),
                },
            )
        })
        .collect();
    let save: Vec<SaveTab<'_>> = tabs
        .iter()
        .map(|t| SaveTab {
            title: t.title.as_deref(),
            tree: &t.tree,
            focused: t.focused,
        })
        .collect();
    let active = tabs
        .iter()
        .position(|t| Some(t.id) == layout.active)
        .unwrap_or(0);
    let home = proto::paths::home_dir();
    Ok(file::write(
        &save,
        &info,
        &SaveOptions {
            name: spec.name,
            active,
            relative_to: spec.relative_to,
            home: home.as_deref(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_builtin_workspaces_open() {
        let here = std::env::temp_dir().canonicalize().unwrap();
        for (name, text) in BUILTINS {
            let f = Found {
                name: (*name).into(),
                origin: Origin::Builtin,
                path: None,
                text: (*text).into(),
            };
            let t = parse(&f, &here).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(t.name.as_deref(), Some(*name));
            assert!(about(text).is_some(), "{name} has no description");
            assert!(!needs_asking(&f, &t));
        }
    }

    /// The ```toml blocks of a Markdown file.
    fn toml_blocks(md: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur: Option<String> = None;
        for line in md.lines() {
            match (&mut cur, line.trim_start()) {
                (None, l) if l.starts_with("```toml") => cur = Some(String::new()),
                (Some(_), l) if l.starts_with("```") => out.push(cur.take().unwrap()),
                (Some(b), _) => {
                    b.push_str(line);
                    b.push('\n');
                }
                _ => {}
            }
        }
        out
    }

    /// Every example in the docs is a workspace that opens (but for the
    /// folders it names, which aren't here).
    #[test]
    fn the_docs_examples_are_workspaces() {
        let here = std::env::temp_dir().canonicalize().unwrap();
        let docs = [
            (
                "SKILL.md",
                include_str!("../../../skills/kibann-workspaces/SKILL.md"),
            ),
            ("README.md", include_str!("../../../README.md")),
        ];
        let mut seen = 0;
        for (file, md) in docs {
            for block in toml_blocks(md) {
                if !block.contains("[[tab]]") {
                    continue;
                }
                seen += 1;
                let r = file::parse(&Source {
                    text: &block,
                    file: None,
                    opened_from: &here,
                    home: Some(&here),
                });
                if let Err(p) = r {
                    let real: Vec<_> =
                        p.0.iter()
                            .filter(|p| !p.message.starts_with("no directory"))
                            .collect();
                    assert!(real.is_empty(), "{file}: {real:?}\n{block}");
                }
            }
        }
        assert!(seen >= 3, "found only {seen} examples");
    }

    #[test]
    fn descriptions_are_the_first_sentence() {
        assert_eq!(
            about("# Two agents. And more\n# here.\nname = \"x\"").as_deref(),
            Some("Two agents.")
        );
        assert_eq!(about("name = \"x\"\n# late"), None);
    }

    #[test]
    fn a_projects_workspace_is_found_above_but_not_past_the_repo() {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("kibann-workspaces-{}", std::process::id()));
        let repo = root.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        std::fs::create_dir_all(root.join(".kibann")).unwrap();
        std::fs::write(root.join(PROJECT_FILE), "[[tab]]\n").unwrap();
        // Above the repo: not this project's.
        assert_eq!(project_workspace(&repo.join("src/deep")), None);
        std::fs::create_dir_all(repo.join(".kibann")).unwrap();
        std::fs::write(repo.join(PROJECT_FILE), "[[tab]]\ncommand = \"make\"\n").unwrap();
        let f = find(None, &repo.join("src/deep")).unwrap();
        assert_eq!(f.path, Some(repo.join(PROJECT_FILE)));
        assert_eq!(f.name, "repo");
        assert_eq!(f.origin, Origin::Project);
        let t = parse(&f, &repo.join("src/deep")).unwrap();
        assert_eq!(t.name.as_deref(), Some("repo"));
        assert_eq!(list(&repo)[0].name, "repo");

        // Trust: asked until agreed to, and again after a change.
        let store = root.join("trusted.json");
        assert!(needs_asking_in(&f, &t, Some(&store)));
        trust_in(&store, f.path.as_ref().unwrap(), &f.text).unwrap();
        assert!(!needs_asking_in(&f, &t, Some(&store)));
        let changed = Found {
            text: "[[tab]]\ncommand = \"rm -rf ~\"\n".into(),
            ..f.clone()
        };
        let t2 = parse(&changed, &repo).unwrap();
        assert!(needs_asking_in(&changed, &t2, Some(&store)));
        // A workspace with no commands never asks.
        let quiet = Found {
            text: "[[tab]]\n".into(),
            ..f
        };
        let t3 = parse(&quiet, &repo).unwrap();
        assert!(!needs_asking_in(&quiet, &t3, Some(&store)));
        std::fs::remove_dir_all(&root).unwrap();
    }

    fn info(program: Option<&[&str]>, agent: Option<&str>) -> PaneInfo {
        PaneInfo {
            id: 1,
            tab: 1,
            title: String::new(),
            cwd: None,
            agent: agent.map(str::to_owned),
            state: None,
            git: None,
            program: program.map(|p| p.iter().map(|s| (*s).to_owned()).collect()),
        }
    }

    #[test]
    fn what_a_pane_saves_as() {
        assert_eq!(saved_command(&info(None, None)), None);
        assert_eq!(
            saved_command(&info(Some(&["/opt/homebrew/bin/htop", "-d", "10"]), None)).as_deref(),
            Some("htop -d 10")
        );
        assert_eq!(
            saved_command(&info(Some(&["npm", "run", "dev server", "it's"]), None)).as_deref(),
            Some(r"npm run 'dev server' 'it'\''s'")
        );
        assert_eq!(saved_command(&info(Some(&["less", "x"]), None)), None);
        assert_eq!(
            saved_command(&info(
                Some(&["/x/kibannd", "watch", "5", "--", "git", "status"]),
                None
            ))
            .as_deref(),
            Some("kibann watch 5 -- git status")
        );
        assert_eq!(
            saved_command(&info(
                Some(&["kibann", "pane", "diff", "p1", "--view"]),
                None
            )),
            None
        );
        assert_eq!(saved_command(&info(Some(&["-zsh"]), None)), None);
        assert_eq!(
            saved_command(&info(
                Some(&["node", "/x/claude-code/cli.js", "fix it"]),
                Some("claude")
            ))
            .as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn commands_are_listed_for_asking() {
        let here = std::env::temp_dir().canonicalize().unwrap();
        let f = Found {
            name: "x".into(),
            origin: Origin::File,
            path: None,
            text: "[[tab]]\nsplit = \"right\"\npanes = [{ command = [\"btop\", \"top\"] }, { exec = [\"tail\", \"-f\", \"a b\"] }, {}]\n".into(),
        };
        let t = parse(&f, &here).unwrap();
        let c: Vec<String> = commands(&t).into_iter().map(|r| r.run).collect();
        assert_eq!(c, ["btop  or  top", "tail -f 'a b'"]);
        assert!(needs_asking(&f, &t));
    }
}
