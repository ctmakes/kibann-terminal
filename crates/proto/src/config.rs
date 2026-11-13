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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font_family: Vec::new(),
            font_size: None,
            agent_notifications: true,
            keybind: Vec::new(),
            restore_layout: true,
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
                _ => {}
            }
        }
        (s, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fonts_and_switches() {
        let (s, e) = Settings::parse(
            "# mine\ntheme = Nord\nfont-family = \"Berkeley Mono\"\nfont-family = Symbols Nerd Font # icons\nfont-size = 13.5\nagent-notifications = false\nkeybind = clear\nkeybind = super+d=new_split:down # mine\nrestore-layout = false\n",
        );
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(s.font_family, ["Berkeley Mono", "Symbols Nerd Font"]);
        assert_eq!(s.font_size, Some(13.5));
        assert!(!s.agent_notifications);
        assert_eq!(s.keybind, ["clear", "super+d=new_split:down"]);
        assert!(!s.restore_layout);
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
