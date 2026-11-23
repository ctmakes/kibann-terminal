//! What the user can ask the window to do: a shortcut, a menu item, a
//! `keybind` line in the config. The app's menu ids are these as JSON, and
//! the page's keymap uses them (`Action` in `workspace/keymap.ts`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::layout::Dir;

/// A way to move across panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Command {
    NewTab,
    Close,
    Split {
        dir: Dir,
    },
    /// The next (1) or previous (-1) pane in the tab.
    FocusCycle {
        #[ts(type = "1 | -1")]
        step: i8,
    },
    Focus {
        toward: Direction,
    },
    Resize {
        toward: Direction,
    },
    Equalize,
    Zoom,
    /// The next (1) or previous (-1) tab.
    TabCycle {
        #[ts(type = "1 | -1")]
        step: i8,
    },
    /// 0-based; -1 is the last tab.
    GoToTab {
        index: i32,
    },
    Sidebar,
    Shortcuts,
    /// Bigger (1), smaller (-1), or back to the config's size (0).
    FontSize {
        #[ts(type = "1 | -1 | 0")]
        step: i8,
    },
    /// Settings (Cmd+,).
    Settings,
    /// The theme picker.
    Themes,
    /// The workspace picker.
    Workspaces,
    /// Save the window as a workspace.
    SaveWorkspace,
    /// The screen picker.
    Screens,
    /// The next (1) or previous (-1) screen.
    ScreenCycle {
        #[ts(type = "1 | -1")]
        step: i8,
    },
    /// Back to the default screen, for one that hides everything.
    ScreenReset,
    /// The find bar on the focused pane.
    Find,
    /// The next match up the scrollback (older), or back down.
    FindStep {
        older: bool,
    },
}

impl Command {
    /// As a menu item's id (the page parses it back).
    pub fn id(&self) -> String {
        serde_json::to_string(self).expect("a command serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_what_the_page_reads() {
        assert_eq!(Command::NewTab.id(), r#"{"kind":"newTab"}"#);
        assert_eq!(
            Command::Split { dir: Dir::Row }.id(),
            r#"{"kind":"split","dir":"row"}"#
        );
        assert_eq!(
            Command::FindStep { older: true }.id(),
            r#"{"kind":"findStep","older":true}"#
        );
        assert_eq!(
            Command::GoToTab { index: -1 }.id(),
            r#"{"kind":"goToTab","index":-1}"#
        );
    }
}
