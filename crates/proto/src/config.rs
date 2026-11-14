//! The config file's settings besides the theme (see [`crate::theme`] for
//! `theme`), in Ghostty's syntax:
//!
//! ```text
//! font-family = Berkeley Mono
//! font-family = Symbols Nerd Font   # repeated: fallbacks, in order
//! font-size = 13
//! agent-notifications = false
//! keybind = super+shift+d=new_split:down   # repeated; see app/src/workspace/keybinds.ts
//! restore-layout = false
//! ```

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The smallest and largest font size (CSS px), for the file and for zoom.
pub const FONT_SIZE_MIN: f64 = 6.0;
pub const FONT_SIZE_MAX: f64 = 72.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Settings {
    /// Fonts to try first, in order; the bundled font and system fallbacks
    /// come after them. Empty: the bundled font.
    pub font_family: Vec<String>,
    /// CSS px; `None`: the default.
    pub font_size: Option<f64>,
    /// Tell the user (a system notification) when an agent in a pane they
    /// aren't looking at waits for them or finishes.
    pub agent_notifications: bool,
    /// `keybind` lines' values in order (`trigger=action`, or `clear`),
    /// read by the app, which owns the shortcuts.
    pub keybind: Vec<String>,
    /// Reopen the tabs and splits (with fresh shells) after the sessions
    /// ended, as after a restart.
    pub restore_layout: bool,
    /// Keep running with the window closed, behind an icon in the menu
    /// bar, so agents can still notify. Off: closing the window quits the
    /// app (the shells keep running in the daemon either way).
    pub menu_bar_icon: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: Vec::new(),
            font_size: None,
            agent_notifications: true,
            keybind: Vec::new(),
            restore_layout: true,
            menu_bar_icon: true,
        }
    }
}

impl Settings {
    /// The settings in a config file's text, and a message for each line
    /// that couldn't be used (the rest still apply). Other keys are left to
    /// their readers.
    pub fn parse(text: &str) -> (Self, Vec<String>) {
        let mut s = Self::default();
        let mut errors = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            // A comment after the value, as in Ghostty.
            let v = v.split(" #").next().unwrap_or("").trim();
            let v = v
                .strip_prefix('"')
                .and_then(|x| x.strip_suffix('"'))
                .unwrap_or(v);
            let bad = |what: &str| format!("line {}: bad {what}: {v}", n + 1);
            match k.trim() {
                // An empty value clears the list, as in Ghostty.
                "font-family" if v.is_empty() => s.font_family.clear(),
                "font-family" => {
                    if v.contains([';', '{', '}']) {
                        errors.push(bad("font-family"));
                    } else {
                        s.font_family.push(v.to_owned());
                    }
                }
                "font-size" => match v.parse::<f64>() {
                    Ok(x) if (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&x) => s.font_size = Some(x),
                    _ => errors.push(bad("font-size")),
                },
                "agent-notifications" => match v {
                    "true" => s.agent_notifications = true,
                    "false" => s.agent_notifications = false,
                    _ => errors.push(bad("agent-notifications (true or false)")),
                },
                "keybind" if !v.is_empty() => s.keybind.push(v.to_owned()),
                "restore-layout" => match v {
                    "true" => s.restore_layout = true,
                    "false" => s.restore_layout = false,
                    _ => errors.push(bad("restore-layout (true or false)")),
                },
                "menu-bar-icon" => match v {
                    "true" => s.menu_bar_icon = true,
                    "false" => s.menu_bar_icon = false,
                    _ => errors.push(bad("menu-bar-icon (true or false)")),
                },
                _ => {}
            }
        }
        (s, errors)
    }
}

/// The keys Settings (the app's modal) may write. `theme` has its own
/// writer, and `keybind` is only edited by hand.
pub const SETTABLE: &[&str] = &[
    "agent-notifications",
    "menu-bar-icon",
    "restore-layout",
    "font-family",
    "font-size",
];

/// Whether `values` can be written for `key` (a key Settings may write,
/// values the parser takes as they are).
pub fn check_values(key: &str, values: &[String]) -> Result<(), String> {
    if !SETTABLE.contains(&key) {
        return Err(format!("{key} can't be set here"));
    }
    if values.len() > 1 && key != "font-family" {
        return Err(format!("{key} takes one value"));
    }
    for v in values {
        if v.trim().is_empty() || v.trim() != v || v.contains(['\n', '\r', '"']) || v.contains(" #")
        {
            return Err(format!("bad {key}: {v:?}"));
        }
    }
    let text: String = values.iter().map(|v| format!("{key} = {v}\n")).collect();
    match Settings::parse(&text).1.into_iter().next() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// `text` (a config file) with `key`'s lines replaced by `values`, one line
/// each, where the first of them was (or at the end). Everything else, and
/// comments, stay as they were. No values removes the key: its default.
pub fn set_config_values(text: &str, key: &str, values: &[String]) -> String {
    let mut out = Vec::new();
    let mut done = false;
    let new = || values.iter().map(|v| format!("{key} = {v}"));
    for l in text.lines() {
        let t = l.trim();
        let ours = !t.starts_with('#') && t.split_once('=').is_some_and(|(k, _)| k.trim() == key);
        if !ours {
            out.push(l.to_owned());
        } else if !done {
            out.extend(new());
            done = true;
        }
    }
    if !done {
        out.extend(new());
    }
    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn setting_values_keeps_the_rest() {
        let file = "# mine\nfont-family = A\ntheme = Nord\n# font-family = old\nfont-family = B # icons\nfont-size = 12\n";
        assert_eq!(
            set_config_values(file, "font-family", &v(&["X", "Y Mono"])),
            "# mine\nfont-family = X\nfont-family = Y Mono\ntheme = Nord\n# font-family = old\nfont-size = 12\n"
        );
        assert_eq!(
            set_config_values(file, "font-size", &v(&["14"])),
            "# mine\nfont-family = A\ntheme = Nord\n# font-family = old\nfont-family = B # icons\nfont-size = 14\n"
        );
        // A key not there yet goes at the end; none removes it.
        assert_eq!(
            set_config_values("theme = Nord", "menu-bar-icon", &v(&["false"])),
            "theme = Nord\nmenu-bar-icon = false\n"
        );
        assert_eq!(
            set_config_values(file, "font-family", &[]),
            "# mine\ntheme = Nord\n# font-family = old\nfont-size = 12\n"
        );
        assert_eq!(
            set_config_values("", "font-size", &v(&["13"])),
            "font-size = 13\n"
        );
        assert_eq!(set_config_values("", "font-size", &[]), "");
        // The result reads back as what was set.
        let (s, e) = Settings::parse(&set_config_values(
            file,
            "font-family",
            &v(&["X", "Y Mono"]),
        ));
        assert!(e.is_empty());
        assert_eq!(s.font_family, ["X", "Y Mono"]);
    }

    #[test]
    fn only_settable_keys_and_good_values() {
        assert!(check_values("menu-bar-icon", &v(&["false"])).is_ok());
        assert!(check_values("font-family", &v(&["JetBrains Mono", "Symbols Nerd Font"])).is_ok());
        assert!(check_values("font-size", &v(&["13.5"])).is_ok());
        assert!(check_values("font-size", &[]).is_ok());
        assert!(check_values("keybind", &v(&["clear"])).is_err());
        assert!(check_values("theme", &v(&["Nord"])).is_err());
        assert!(check_values("menu-bar-icon", &v(&["yes"])).is_err());
        assert!(check_values("font-size", &v(&["500"])).is_err());
        assert!(check_values("font-size", &v(&["12", "13"])).is_err());
        assert!(check_values("font-family", &v(&["A\nkeybind = clear"])).is_err());
        assert!(check_values("font-family", &v(&["A # b"])).is_err());
        assert!(check_values("font-family", &v(&[" "])).is_err());
    }

    #[test]
    fn parses_fonts_and_switches() {
        let (s, e) = Settings::parse(
            "# mine\ntheme = Nord\nfont-family = \"Berkeley Mono\"\nfont-family = Symbols Nerd Font # icons\nfont-size = 13.5\nagent-notifications = false\nkeybind = clear\nkeybind = super+d=new_split:down # mine\nrestore-layout = false\nmenu-bar-icon = false\n",
        );
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(s.font_family, ["Berkeley Mono", "Symbols Nerd Font"]);
        assert_eq!(s.font_size, Some(13.5));
        assert!(!s.agent_notifications);
        assert_eq!(s.keybind, ["clear", "super+d=new_split:down"]);
        assert!(!s.restore_layout);
        assert!(!s.menu_bar_icon);
    }

    #[test]
    fn defaults_and_bad_lines() {
        assert_eq!(Settings::parse("").0, Settings::default());
        let (s, e) = Settings::parse(
            "font-size = huge\nfont-size = 200\nfont-family = A\nfont-family =\nfont-family = x{y}\nagent-notifications = yes\n",
        );
        assert_eq!(
            s,
            Settings::default(),
            "bad lines change nothing; empty clears"
        );
        assert_eq!(e.len(), 4, "{e:?}");
        assert!(e[0].starts_with("line 1: bad font-size"));
    }
}
