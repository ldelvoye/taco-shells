use serde::{Deserialize, Serialize};

use crate::ids::{HarnessId, TerminalId};
use crate::rows::AgentSignal;

/// A hook script's call, with the process ids from the hook's process up to the terminal's shell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookMessage {
    pub pids: Vec<u32>,
    pub call: HookCall,
}

/// A hook script's call once the terminal it came from is known.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HookEvent {
    pub terminal: TerminalId,
    pub call: HookCall,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HookCall {
    FocusTab,
    Attention,
    AgentStart {
        harness: HarnessId,
        session_id: String,
        cwd: String,
        has_conversation: bool,
    },
    AgentEnd {
        harness: HarnessId,
        session_id: String,
    },
    AgentState {
        harness: HarnessId,
        session_id: String,
        signal: AgentSignal,
    },
    /// `payload` is the hook's whole JSON object as the agent's tool sent it, as text.
    AgentHook {
        harness: HarnessId,
        session_id: String,
        event: String,
        payload: String,
    },
}
