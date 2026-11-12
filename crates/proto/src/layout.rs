//! The workspace layout: tabs, each a split tree of panes. The daemon owns
//! it; the app draws it and sends [`Action`]s, and so does the `kibann` CLI.
//!
//! A pane is a session: its id is the session id. Ids are never reused.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type PaneId = u32;
pub type TabId = u32;

/// "row" puts the two sides of a split next to each other (split right);
/// "column" stacks them (split down).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Dir {
    Row,
    Column,
}

/// A split divides its box between `a` (left or top) and `b` (right or
/// bottom); `ratio` is `a`'s share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
#[ts(export)]
pub enum Tree {
    Leaf {
        pane: PaneId,
    },
    Split {
        dir: Dir,
        ratio: f64,
        a: Box<Tree>,
        b: Box<Tree>,
    },
}

/// One step from a split to one of its sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Step {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, rename = "LayoutTab")]
pub struct Tab {
    pub id: TabId,
    pub tree: Tree,
    pub focused: PaneId,
    /// The pane shown alone, filling the tab.
    pub zoomed: Option<PaneId>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Layout {
    pub tabs: Vec<Tab>,
    /// The tab shown; `None` only when there are no tabs.
    pub active: Option<TabId>,
}

/// A change to the layout, from the app or the CLI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
#[ts(export, rename = "LayoutAction")]
pub enum Action {
    /// A new tab with one new shell, in `cwd`, else in `near`'s directory.
    /// With `focus`, it becomes the active tab.
    NewTab {
        #[ts(optional)]
        near: Option<PaneId>,
        #[ts(optional)]
        cwd: Option<String>,
        /// A program instead of the login shell.
        #[ts(optional)]
        argv: Option<Vec<String>>,
        focus: bool,
    },
    /// A new shell right of `pane` ("row") or below it ("column"), in `cwd`,
    /// else in `pane`'s directory. With `focus`, it becomes the focused pane
    /// (and its tab the active one).
    Split {
        pane: PaneId,
        dir: Dir,
        #[ts(optional)]
        cwd: Option<String>,
        #[ts(optional)]
        argv: Option<Vec<String>>,
        focus: bool,
    },
    /// End the pane's shell; its sibling takes the space.
    Close {
        pane: PaneId,
    },
    /// Focus the pane and show its tab.
    Focus {
        pane: PaneId,
    },
    Activate {
        tab: TabId,
    },
    SetRatio {
        tab: TabId,
        path: Vec<Step>,
        ratio: f64,
    },
    Equalize {
        tab: TabId,
    },
    /// Show `pane` alone in its tab, or every pane again with `None`.
    Zoom {
        tab: TabId,
        #[ts(optional)]
        pane: Option<PaneId>,
    },
}

impl Tree {
    pub fn leaf(pane: PaneId) -> Self {
        Tree::Leaf { pane }
    }

    /// Panes in reading order: left before right, top before bottom.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<PaneId>) {
        match self {
            Tree::Leaf { pane } => out.push(*pane),
            Tree::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        match self {
            Tree::Leaf { pane: p } => *p == pane,
            Tree::Split { a, b, .. } => a.contains(pane) || b.contains(pane),
        }
    }
}

impl Layout {
    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// The tab holding `pane`.
    pub fn tab_of(&self, pane: PaneId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.tree.contains(pane))
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tabs.iter().flat_map(|t| t.tree.panes()).collect()
    }
}
