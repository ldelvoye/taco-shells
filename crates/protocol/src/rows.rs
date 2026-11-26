use serde::{Deserialize, Serialize};

use crate::ids::HarnessId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowState {
    Working,
    Needs,
    Done,
    Idle,
}

/// What an agent's hook reported. `Resume` is a start whose session already holds a conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSignal {
    Start,
    Resume,
    Working,
    Needs,
    Stopped,
    End,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentRow {
    pub harness: HarnessId,
    pub session_id: String,
    pub state: RowState,
    /// When `state` began, in milliseconds since the Unix epoch.
    pub state_since_ms: u64,
    pub has_conversation: bool,
    /// The agent's question while the row needs you.
    pub question: Option<String>,
}

/// What the window needs to know about one agent: its name in a menu, and whether its sessions can be forked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentLabel {
    pub name: String,
    pub forks: bool,
}
