//! Themes: the bundled collection (iTerm2-Color-Schemes, Ghostty format),
//! the user's own files, and the config file that says which one to use.
//!
//! - Config: `~/.config/kibann/config` (or `$XDG_CONFIG_HOME/kibann`, or
//!   `$KIBANN_CONFIG_DIR`), Ghostty syntax; only `theme` is read for now.
//! - The user's themes: `themes/` beside it, one file per theme, named as
//!   the theme. They win over a bundled theme of the same name.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

use proto::theme::{
    DEFAULT_DARK, DEFAULT_LIGHT, KIBANN_DARK, KIBANN_LIGHT, Theme, ThemeChoice, config_theme,
    set_config_theme,
};
use serde::Serialize;

/// `### NAME` then the theme's lines, for every bundled theme.
const BUNDLE: &str = include_str!("../themes/themes.txt");

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dark: bool,
}

/// The bundled themes: name, text, and whether it's dark.
fn bundled() -> &'static [(&'static str, &'static str, bool)] {
    static ALL: OnceLock<Vec<(&'static str, &'static str, bool)>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut out = Vec::new();
        let mut rest = BUNDLE;
        // Each entry starts at a line "### NAME".
        while let Some(at) = rest.find("### ") {
            let after = &rest[at + 4..];
            let (name, body) = after.split_once('\n').unwrap_or((after, ""));
            let end = body.find("\n### ").map_or(body.len(), |e| e + 1);
            let text = &body[..end];
            out.push((name.trim(), text, Theme::parse(text, None).0.is_dark()));
            rest = &body[end..];
        }
        out
    })
}

/// Where the config file and the user's themes live.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("KIBANN_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join("kibann"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".config").join("kibann"))
}

fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config"))
}

fn user_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("themes"))
}

/// The user's theme files: name and path.
fn user_themes() -> Vec<(String, PathBuf)> {
    let Some(dir) = user_dir() else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_owned();
            (!name.starts_with('.')).then_some((name, e.path()))
        })
        .collect()
}

/// Every theme, the built-in two first, then by name.
pub fn list() -> Vec<Entry> {
    let mut rest: Vec<Entry> = bundled()
        .iter()
        .map(|(n, _, dark)| Entry {
            name: (*n).to_owned(),
            dark: *dark,
        })
        .collect();
    for (name, path) in user_themes() {
        let dark = std::fs::read_to_string(&path)
            .map(|t| Theme::parse(&t, None).0.is_dark())
            .unwrap_or(true);
        match rest.iter_mut().find(|e| e.name == name) {
            Some(e) => e.dark = dark,
            None => rest.push(Entry { name, dark }),
        }
    }
    rest.retain(|e| e.name != DEFAULT_DARK && e.name != DEFAULT_LIGHT);
    rest.sort_by_key(|e| e.name.to_lowercase());
    let mut out = vec![
        Entry {
            name: DEFAULT_DARK.into(),
            dark: true,
        },
        Entry {
            name: DEFAULT_LIGHT.into(),
            dark: false,
        },
    ];
    out.extend(rest);
    out
}

/// The theme named `name`, and a message for each line that couldn't be
/// used. A user file wins, then the built-ins, then the bundle (an exact
/// name first, then ignoring case).
pub fn load(name: &str) -> Result<(Theme, Vec<String>), String> {
    if let Some((_, path)) = user_themes().into_iter().find(|(n, _)| n == name) {
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        return Ok(Theme::parse(&text, None));
    }
    if name == DEFAULT_DARK {
        return Ok(Theme::parse(KIBANN_DARK, None));
    }
    if name == DEFAULT_LIGHT {
        return Ok(Theme::parse(KIBANN_LIGHT, None));
    }
    let all = bundled();
    let found = all
        .iter()
        .find(|(n, ..)| *n == name)
        .or_else(|| all.iter().find(|(n, ..)| n.eq_ignore_ascii_case(name)));
    match found {
        Some((_, text, _)) => Ok(Theme::parse(text, None)),
        None => Err(format!("no theme named \"{name}\"")),
    }
}

/// The config file's theme choice (the defaults without a file or a
/// `theme` line), and its problems.
pub fn read_choice() -> (ThemeChoice, Vec<String>) {
    let Some(text) = config_file().and_then(|f| std::fs::read_to_string(f).ok()) else {
        return (ThemeChoice::default(), Vec::new());
    };
    let (choice, errors) = config_theme(&text);
    (choice.unwrap_or_default(), errors)
}

/// Save `choice` to the config file, keeping its other lines.
pub fn save_choice(choice: &ThemeChoice) -> Result<(), String> {
    let file = config_file().ok_or("no home directory")?;
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    let new = set_config_theme(&old, choice);
    let dir = file.parent().ok_or("bad config path")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Write beside it and rename, so a reader never sees half a file.
    let tmp = dir.join(".config.tmp");
    std::fs::write(&tmp, new).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
}

/// Changes when the config file or a user theme changes (polled).
pub fn stamp() -> Vec<(String, Option<SystemTime>)> {
    let mtime = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut s: Vec<_> = user_themes()
        .into_iter()
        .map(|(n, p)| {
            let t = mtime(&p);
            (n, t)
        })
        .collect();
    s.sort();
    if let Some(f) = config_file() {
        s.push((String::new(), mtime(&f)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundle_parses_cleanly() {
        let all = bundled();
        assert!(all.len() > 800, "{} themes", all.len());
        for (name, text, _) in all {
            assert!(!name.is_empty());
            let (t, errors) = Theme::parse(text, None);
            assert!(errors.is_empty(), "{name}: {errors:?}");
            assert!(text.contains("background"), "{name} sets a background");
            assert_eq!(t.palette.len(), 16);
        }
        let dracula = load("Dracula").unwrap().0;
        assert_eq!(dracula.background, 0x282a36);
        assert!(load("dracula").is_ok(), "case doesn't matter");
        assert!(load("No Such Theme").is_err());
        assert!(!load("Catppuccin Latte").unwrap().0.is_dark());
    }

    #[test]
    fn the_list_starts_with_the_built_ins() {
        let l = list();
        assert_eq!(l[0].name, DEFAULT_DARK);
        assert_eq!(l[1].name, DEFAULT_LIGHT);
        assert!(l.iter().any(|e| e.name == "Nord" && e.dark));
        let names: Vec<_> = l[2..].iter().map(|e| e.name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
