//! What a part (a web page in a screen's block) sees of the window, through
//! `window.kibann`. `parts/kibann.d.ts` documents the same shapes for part
//! authors; `npm run typecheck` checks that it matches these.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::layout::{PaneId, TabId};
use crate::status::AgentState;

/// The parts API's version, in the `init` message (`kibann.api`). Bump it
/// when a part written for the old one would break.
pub const PART_API: u32 = 1;

/// The window (`kibann.state`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PartState {
    pub tabs: Vec<PartTab>,
    /// Every pane, in tab order.
    pub panes: Vec<PartPane>,
    /// The focused pane of the shown tab.
    pub focused: Option<PaneId>,
    /// The screen's name.
    pub screen: String,
    /// Panes whose agent waits for the user.
    pub blocked: u32,
    /// Whether the window has the user's attention.
    pub window_focused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PartTab {
    pub id: TabId,
    /// Its name from a workspace, else the focused pane's title.
    pub title: String,
    /// Shown now.
    pub active: bool,
    /// Its panes' ids, in order.
    pub panes: Vec<PaneId>,
    pub focused: PaneId,
    /// The most urgent agent state in it: blocked, done, working, idle.
    pub state: Option<AgentState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PartPane {
    pub id: PaneId,
    /// Its tab's id.
    pub tab: TabId,
    /// The program's title, else the program's name.
    pub title: String,
    /// Focused in its tab.
    pub focused: bool,
    /// On screen now (its tab is shown, and it isn't hidden by a zoom).
    pub shown: bool,
    /// Its directory, `~` for home; "" until known.
    pub cwd: String,
    /// The repository's name, when the directory is in one.
    pub repo: Option<String>,
    pub branch: Option<String>,
    /// Files changed, staged or not.
    pub changed: u32,
    pub ahead: u32,
    pub behind: u32,
    /// The coding agent in it (`claude`, `opencode`), if any.
    pub agent: Option<String>,
    /// That agent's state; `None` without one.
    pub state: Option<AgentState>,
    /// When `state` began (ms since 1970).
    #[ts(type = "number | null")]
    pub since: Option<u64>,
    /// The terminal's colors, `#rrggbb`.
    pub bg: String,
    pub fg: String,
}

/// The window's look (`kibann.on("theme")`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PartTheme {
    /// The window's colors by name (also CSS variables `--k-NAME`).
    pub colors: std::collections::BTreeMap<String, String>,
    /// Kibann's UI font (`--k-font`) and the terminals' (`--k-mono`).
    pub font: String,
    pub mono: String,
    pub dark: bool,
}
