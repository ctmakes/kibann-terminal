//! Agent status: which panes hold a coding agent, and whether it is working,
//! waiting for the user, idle, or finished while nobody looked.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AgentState {
    Idle,
    Working,
    /// Waiting for the user to answer a prompt.
    Blocked,
    /// Went from working to idle while its pane wasn't seen. Turns idle
    /// once the pane is focused.
    Done,
}

impl AgentState {
    pub fn name(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Working => "working",
            AgentState::Blocked => "blocked",
            AgentState::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => AgentState::Idle,
            "working" => AgentState::Working,
            "blocked" => AgentState::Blocked,
            "done" => AgentState::Done,
            _ => return None,
        })
    }
}

/// The agent in a pane. Panes without one have no status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PaneStatus {
    /// Which agent (`claude`), detected or reported.
    pub agent: String,
    pub state: AgentState,
}
