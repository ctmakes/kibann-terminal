//! Screens: what the whole window shows and where, read from
//! `screens/NAME/screen.json`. A tree of rows, columns and stacks whose
//! leaves are blocks; a block shows the terminals or a part (Kibann's own,
//! like the sidebar, or a web page from `parts/NAME/`).
//!
//! The `files` crate reads and checks them; the app
//! draws them (`app/src/screen/`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A screen file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Screen {
    /// One line on what it's for, shown in the picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub about: Option<String>,
    /// Colors and fonts for Kibann's own surfaces on this screen.
    #[serde(default)]
    pub chrome: Chrome,
    pub root: Node,
    /// Blocks over the window, at an edge.
    #[serde(default)]
    pub floating: Vec<Floating>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Chrome {
    /// Behind and between the blocks (the gaps show it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub background: Option<String>,
    /// The window's colors by name (`sidebar`, `bar`, `text`, `blocked`…),
    /// over the theme's. Parts get them too, as `--k-NAME`.
    #[serde(default)]
    pub colors: BTreeMap<String, String>,
    /// The font for Kibann's own text (not the terminals'), a CSS family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub font: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ScreenNodeKind")]
pub enum NodeKind {
    /// Children side by side.
    Row,
    /// Children stacked top to bottom.
    Column,
    /// One child at a time: `active`, switched by `node.show`.
    Stack,
    /// Shows `content`.
    Block,
}

/// A place in the window. Which fields apply depends on `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "ScreenNode")]
pub struct Node {
    #[serde(rename = "type")]
    pub kind: NodeKind,
    /// For `node.toggle`, a stack's `active`, and remembering a size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    /// Along its parent's direction; none is `{"weight": 1}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub size: Option<Size>,
    /// Pixels, along its parent's direction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max: Option<f64>,
    /// Its edge toward the next sibling can be dragged (a `px` size, with
    /// an `id` to remember it by).
    #[serde(default)]
    pub resizable: bool,
    /// Starts hidden; `node.toggle` shows it.
    #[serde(default)]
    pub hidden: bool,
    /// Row, column and stack.
    #[serde(default)]
    pub children: Vec<Node>,
    /// A stack's child shown first, by id; none is the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub active: Option<String>,
    /// A block's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub content: Option<Content>,
    /// CSS for the node's box, from a short list of properties
    /// ([`STYLE_PROPERTIES`]).
    #[serde(default)]
    pub style: BTreeMap<String, String>,
}

/// How much room a node takes along its parent's direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
#[ts(export, rename = "ScreenSize")]
pub enum Size {
    /// Fixed pixels.
    Px { px: f64 },
    /// A share of what the fixed ones leave.
    Weight { weight: f64 },
    /// As big as its content (Kibann's sidebar sizes itself).
    Auto(Auto),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ScreenAuto")]
pub enum Auto {
    Auto,
}

/// What a block shows: exactly one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "ScreenContent")]
pub struct Content {
    /// `"active"`: the shown tab's panes and splits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tab: Option<String>,
    /// `"focused"`: only the shown tab's focused pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pane: Option<String>,
    /// A part by name: Kibann's (`sidebar`, `tabs`) or a web part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub part: Option<String>,
    /// Given to the part as `kibann.props`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "unknown")]
    pub props: Option<serde_json::Value>,
}

/// A block over the window at one edge. With `peek`, that much of it shows
/// and the rest slides in on hover; `node.toggle` opens and closes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, rename = "ScreenFloating")]
pub struct Floating {
    pub id: String,
    pub content: Content,
    pub edge: Edge,
    #[serde(default)]
    pub align: Align,
    /// Pixels; along the edge it's `length`, out from it `depth`.
    pub depth: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub length: Option<f64>,
    /// Pixels showing while closed; 0 hides it until toggled.
    #[serde(default)]
    pub peek: f64,
    /// Starts open.
    #[serde(default)]
    pub open: bool,
    #[serde(default)]
    pub style: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ScreenEdge")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ScreenAlign")]
pub enum Align {
    Start,
    #[default]
    Center,
    End,
    /// The whole edge.
    Stretch,
}

/// The CSS properties a node's `style` may set.
pub const STYLE_PROPERTIES: &[&str] = &[
    "background",
    "border",
    "border-top",
    "border-right",
    "border-bottom",
    "border-left",
    "border-radius",
    "box-shadow",
    "gap",
    "margin",
    "opacity",
    "outline",
    "padding",
];

/// A part's `part.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PartManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub about: Option<String>,
    #[serde(default)]
    pub permissions: Permissions,
    /// Takes the keyboard when clicked (a text field). Otherwise a click
    /// leaves the keys with the terminal.
    #[serde(default)]
    pub focusable: bool,
}

/// What a part may do beyond reading the window's state (tabs, panes,
/// titles, directories, branches, agents). Nothing else is allowed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct Permissions {
    /// See what the terminals show (`kibann.thumbnail`).
    #[serde(default)]
    pub read: bool,
    /// Type into terminals, close them and open workspaces (which run
    /// commands).
    #[serde(default, rename = "type")]
    #[ts(rename = "type")]
    pub type_: bool,
    /// Programs it may run (`kibann.exec`), by name.
    #[serde(default)]
    pub exec: Vec<String>,
    /// Hosts it may connect to (`api.github.com`, `*.example.com`).
    #[serde(default)]
    pub net: Vec<String>,
}

impl Permissions {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// What they allow, one line each, for asking the user.
    pub fn describe(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.read {
            out.push("see what your terminals show".to_owned());
        }
        if self.type_ {
            out.push("type into your terminals, close them and open workspaces".to_owned());
        }
        if !self.exec.is_empty() {
            out.push(format!("run {}", self.exec.join(", ")));
        }
        if !self.net.is_empty() {
            out.push(format!("connect to {}", self.net.join(", ")));
        }
        out
    }
}

/// A screen `screen_list` shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScreenEntry {
    pub name: String,
    pub about: Option<String>,
    /// Comes with Kibann (read-only; copy it to change it).
    pub builtin: bool,
    /// Where it is, for a user's.
    pub path: Option<String>,
}

/// The screen to show, as the app gets it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScreenNow {
    pub name: String,
    pub screen: Screen,
    /// Every web part it uses, with what the user allowed it.
    pub parts: BTreeMap<String, PartNow>,
    /// Why the chosen screen isn't the one shown (it has a mistake: the
    /// last good one stays), or "" when it is.
    pub problem: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PartNow {
    /// Where its `index.html` is served.
    pub url: String,
    pub manifest: PartManifest,
    /// What it may do: its manifest's permissions once the user agreed,
    /// else none.
    pub granted: Permissions,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_all_three_ways() {
        let s: Vec<Size> = serde_json::from_str(r#"[{"px":240},{"weight":2},"auto"]"#).unwrap();
        assert_eq!(
            s,
            vec![
                Size::Px { px: 240.0 },
                Size::Weight { weight: 2.0 },
                Size::Auto(Auto::Auto)
            ]
        );
    }

    #[test]
    fn permissions_name_type_as_type() {
        let p: Permissions = serde_json::from_str(r#"{"type":true,"exec":["git"]}"#).unwrap();
        assert!(p.type_ && !p.read);
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"read":false,"type":true,"exec":["git"],"net":[]}"#
        );
    }
}
