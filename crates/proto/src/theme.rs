//! Color themes, in Ghostty's theme file format:
//!
//! ```text
//! # a comment
//! palette = 0=#1d1f21
//! background = #1d1f21
//! foreground = #c5c8c6
//! cursor-color = #c5c8c6
//! cursor-text = #1d1f21
//! selection-background = #373b41
//! selection-foreground = #c5c8c6
//! ```
//!
//! And the config line that picks one: `theme = NAME` or
//! `theme = light:NAME,dark:NAME`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A theme's colors (`0xRRGGBB`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Theme {
    /// The 16 ANSI colors.
    pub palette: Vec<u32>,
    pub background: u32,
    pub foreground: u32,
    /// `None`: the cursor takes the text's color.
    pub cursor: Option<u32>,
    pub cursor_text: Option<u32>,
    /// `None`: selected text is shown inverted.
    pub selection_bg: Option<u32>,
    pub selection_fg: Option<u32>,
}

/// The name of the theme used when none is chosen in dark mode.
pub const DEFAULT_DARK: &str = "Kibann Dark";
/// And in light mode.
pub const DEFAULT_LIGHT: &str = "Kibann Light";

/// Today's colors: libghostty's default palette (Tomorrow Night) on black.
pub const KIBANN_DARK: &str = "\
palette = 0=#1d1f21
palette = 1=#cc6666
palette = 2=#b5bd68
palette = 3=#f0c674
palette = 4=#81a2be
palette = 5=#b294bb
palette = 6=#8abeb7
palette = 7=#c5c8c6
palette = 8=#666666
palette = 9=#d54e53
palette = 10=#b9ca4a
palette = 11=#e7c547
palette = 12=#7aa6da
palette = 13=#c397d8
palette = 14=#70c0b1
palette = 15=#eaeaea
background = #000000
foreground = #ffffff
";

/// A light counterpart: dark text on white, colors deep enough to read on it.
pub const KIBANN_LIGHT: &str = "\
palette = 0=#1f2328
palette = 1=#cf222e
palette = 2=#116329
palette = 3=#7d4e00
palette = 4=#0969da
palette = 5=#8250df
palette = 6=#1b7c83
palette = 7=#6e7781
palette = 8=#57606a
palette = 9=#a40e26
palette = 10=#1a7f37
palette = 11=#633c01
palette = 12=#218bff
palette = 13=#a475f9
palette = 14=#3192aa
palette = 15=#8c959f
background = #ffffff
foreground = #1f2328
cursor-color = #0969da
selection-background = #b6d7ff
selection-foreground = #1f2328
";

impl Theme {
    /// The built-in dark theme.
    pub fn kibann_dark() -> Self {
        // It sets every color, so it needs no base of its own.
        let blank = Theme {
            palette: vec![0; 16],
            background: 0,
            foreground: 0,
            cursor: None,
            cursor_text: None,
            selection_bg: None,
            selection_fg: None,
        };
        Self::parse(KIBANN_DARK, Some(&blank)).0
    }

    /// The built-in light theme.
    pub fn kibann_light() -> Self {
        Self::parse(KIBANN_LIGHT, None).0
    }

    /// Parse a theme file. Keys it doesn't set keep `base`'s values (the
    /// built-in dark theme's without one). Returns the theme and a message
    /// for each line it couldn't use; unknown keys are skipped silently, as
    /// Ghostty's theme files may carry other settings.
    pub fn parse(text: &str, base: Option<&Theme>) -> (Theme, Vec<String>) {
        let mut t = base.cloned().unwrap_or_else(Theme::kibann_dark);
        let mut errors = Vec::new();
        for entry in crate::config::entries(text) {
            let (n, key, value) = match entry {
                Ok(e) => e,
                Err(n) => {
                    errors.push(format!("line {n}: expected `key = value`"));
                    continue;
                }
            };
            let bad = |what: &str| format!("line {n}: {what}: {value}");
            match key {
                "palette" => {
                    let parsed = value.split_once('=').and_then(|(i, c)| {
                        let i: usize = i.trim().parse().ok()?;
                        Some((i, color(c.trim())?))
                    });
                    match parsed {
                        Some((i, c)) if i < 16 => t.palette[i] = c,
                        // Ghostty allows 0-255; colors past 15 aren't
                        // themed here.
                        Some((i, _)) if i < 256 => {}
                        _ => errors.push(bad("bad palette entry")),
                    }
                }
                "background"
                | "foreground"
                | "cursor-color"
                | "cursor-text"
                | "selection-background"
                | "selection-foreground" => match color(value) {
                    Some(c) => match key {
                        "background" => t.background = c,
                        "foreground" => t.foreground = c,
                        "cursor-color" => t.cursor = Some(c),
                        "cursor-text" => t.cursor_text = Some(c),
                        "selection-background" => t.selection_bg = Some(c),
                        _ => t.selection_fg = Some(c),
                    },
                    None => errors.push(bad(&format!("bad color for {key}"))),
                },
                _ => {}
            }
        }
        (t, errors)
    }

    /// Whether it's a dark theme (its background is dark).
    pub fn is_dark(&self) -> bool {
        luminance(self.background) < DARK_BELOW
    }
}

/// `#rrggbb`, `rrggbb` or `#rgb`.
fn color(s: &str) -> Option<u32> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => u32::from_str_radix(hex, 16).ok(),
        3 => {
            let v = u32::from_str_radix(hex, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            Some((r * 17) << 16 | (g * 17) << 8 | (b * 17))
        }
        _ => None,
    }
}

/// A theme is dark when its background's luminance is below this.
pub const DARK_BELOW: f64 = 0.4;

/// How far each of the window's surfaces and text colors sits from the
/// theme's background toward its foreground (by [`mix`]). The page builds
/// its colors from these (`app/src/workspace/theme.ts`, generated as
/// `MIX`); the app paints the window behind the page `bar`.
pub const CHROME_MIX: &[(&str, f64)] = &[
    ("sidebar", 0.063),
    ("bar", 0.067),
    ("header", 0.078),
    ("panel", 0.1),
    ("tab", 0.114),
    ("tabActive", 0.165),
    ("kbd", 0.165),
    ("edge", 0.165),
    ("divider", 0.227),
    ("muted", 0.49),
    ("text", 0.604),
    ("label", 0.66),
    ("strong", 0.784),
    ("title", 0.847),
    ("bright", 0.9),
];

/// `theme`'s color for the chrome surface `name` (from [`CHROME_MIX`]).
pub fn chrome(theme: &Theme, name: &str) -> Option<u32> {
    let (_, t) = CHROME_MIX.iter().find(|(n, _)| *n == name)?;
    Some(mix(theme.background, theme.foreground, *t))
}

/// `a` moved toward `b` by `t` (0..1), per channel (as `mix` in the app's
/// `workspace/theme.ts`).
pub fn mix(a: u32, b: u32, t: f64) -> u32 {
    let ch = |s: u32| {
        let x = f64::from((a >> s) & 0xff);
        let y = f64::from((b >> s) & 0xff);
        ((x * (1.0 - t) + y * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// WCAG relative luminance, 0 (black) to 1 (white).
fn luminance(c: u32) -> f64 {
    let ch = |v: u32| {
        let v = f64::from(v & 0xff) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c >> 16) + 0.7152 * ch(c >> 8) + 0.0722 * ch(c)
}

/// Which theme to use: one for both appearances, or one for each.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeChoice {
    pub light: String,
    pub dark: String,
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self {
            light: DEFAULT_LIGHT.into(),
            dark: DEFAULT_DARK.into(),
        }
    }
}

impl ThemeChoice {
    /// The value of a `theme =` line: `NAME` or `light:NAME,dark:NAME` (in
    /// either order; a side left out keeps the default).
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        let has_sides = value
            .split(',')
            .any(|p| p.trim_start().starts_with("light:") || p.trim_start().starts_with("dark:"));
        if !has_sides {
            return Some(Self {
                light: value.into(),
                dark: value.into(),
            });
        }
        let mut c = Self::default();
        for part in value.split(',') {
            let part = part.trim();
            if let Some(n) = part.strip_prefix("light:") {
                c.light = n.trim().into();
            } else {
                c.dark = part.strip_prefix("dark:")?.trim().into();
            }
        }
        Some(c)
    }

    /// The value for a `theme =` line.
    pub fn to_value(&self) -> String {
        if self.light == self.dark {
            self.dark.clone()
        } else {
            format!("light:{},dark:{}", self.light, self.dark)
        }
    }

    pub fn for_appearance(&self, dark: bool) -> &str {
        if dark { &self.dark } else { &self.light }
    }
}

/// `text` with its `theme` line set to `choice`, keeping every other line
/// and comment; the line is added at the end if there is none.
pub fn set_config_theme(text: &str, choice: &ThemeChoice) -> String {
    crate::config::set_config_values(text, "theme", &[choice.to_value()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_theme_file() {
        let (t, errors) = Theme::parse(
            "# Dracula\npalette = 1=#ff5555\npalette = 200=#000000\nbackground = #282a36\n\
             foreground=f8f8f2\ncursor-color = #fff\nselection-background = #44475a # soft\n\
             font-size = 14\npalette = 3=nope\nbackground #123456\n",
            None,
        );
        assert_eq!(t.palette[1], 0xff5555);
        assert_eq!(t.palette[0], 0x1d1f21, "unset entries keep the base");
        assert_eq!(t.background, 0x282a36);
        assert_eq!(t.foreground, 0xf8f8f2);
        assert_eq!(t.cursor, Some(0xffffff));
        assert_eq!(t.selection_bg, Some(0x44475a));
        assert_eq!(t.selection_fg, None);
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(errors[0].contains("line 9"));
    }

    #[test]
    fn the_built_in_themes() {
        let d = Theme::kibann_dark();
        assert_eq!(d.palette[15], 0xeaeaea);
        assert_eq!((d.background, d.foreground), (0x000000, 0xffffff));
        assert_eq!(Theme::parse(KIBANN_DARK, None), (d.clone(), vec![]));
        assert!(d.is_dark());
        let l = Theme::kibann_light();
        assert!(!l.is_dark());
        assert_eq!(l.palette.len(), 16);
        assert_eq!(Theme::parse(KIBANN_LIGHT, None).1, Vec::<String>::new());
    }

    #[test]
    fn theme_choices() {
        let both = ThemeChoice::parse("Dracula").unwrap();
        assert_eq!(
            (both.light.as_str(), both.dark.as_str()),
            ("Dracula", "Dracula")
        );
        assert_eq!(both.to_value(), "Dracula");
        let c = ThemeChoice::parse(" dark:Nord , light:Solarized Light ").unwrap();
        assert_eq!(c.for_appearance(true), "Nord");
        assert_eq!(c.for_appearance(false), "Solarized Light");
        assert_eq!(c.to_value(), "light:Solarized Light,dark:Nord");
        let one = ThemeChoice::parse("dark:Nord").unwrap();
        assert_eq!(one.light, DEFAULT_LIGHT);
        assert!(ThemeChoice::parse("dark:Nord,blue:X").is_none());
        assert!(ThemeChoice::parse("  ").is_none());
    }

    #[test]
    fn config_lines() {
        let text = "# mine\nfont-size = 13\ntheme = Nord\n# theme = old\ntheme = dark:Dracula\n";
        let choice = ThemeChoice {
            light: "A".into(),
            dark: "B".into(),
        };
        assert_eq!(
            set_config_theme(text, &choice),
            "# mine\nfont-size = 13\ntheme = light:A,dark:B\n# theme = old\n"
        );
        assert_eq!(set_config_theme("", &choice), "theme = light:A,dark:B\n");
        assert_eq!(
            set_config_theme("x = 1", &choice),
            "x = 1\ntheme = light:A,dark:B\n"
        );
    }
}
