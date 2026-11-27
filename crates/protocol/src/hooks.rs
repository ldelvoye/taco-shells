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

// A shell opened before an update keeps running its old plugin, so any field added below must be optional with a default.

pub mod routes {
    pub const SESSION_START: &str = "/agent-session/start";
    pub const SESSION_END: &str = "/agent-session/end";
    pub const SESSION_STATE: &str = "/agent-session/state";
    pub const AGENT_HOOK: &str = "/agent-session/hook";
    pub const CONTEXT: &str = "/agent-session/context";
    pub const STOP: &str = "/agent-session/stop";
}

pub const TOKEN_FILE_NAME: &str = "focus-token";
pub const PORT_FILE_NAME: &str = "focus-port";
pub const TOKEN_HEADER: &str = "X-Taco-Shells-Token";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartBody {
    pub pids: Vec<u32>,
    pub harness: HarnessId,
    pub session_id: String,
    pub cwd: String,
    #[serde(default)]
    pub has_conversation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEndBody {
    pub pids: Vec<u32>,
    pub harness: HarnessId,
    pub session_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateBody {
    pub pids: Vec<u32>,
    pub harness: HarnessId,
    pub session_id: String,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// The hook route and both question routes carry this body; the payload is the harness's own, passed through unparsed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentHookBody<P> {
    pub pids: Vec<u32>,
    pub harness: HarnessId,
    pub payload: P,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAnswer {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub yours: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopAnswer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub mod deadlines {
    use std::time::Duration;

    // How long the plugin's posts may take in one hook run.
    pub const POSTS: Duration = Duration::from_millis(250);
    // How long the app's router waits to place a question by its pids.
    pub const QUESTION_PLACE: Duration = Duration::from_millis(600);
    // How long the listener's responder waits for an answer before it replies.
    pub const ANSWER_WINDOW: Duration = Duration::from_secs(1);
    // How long the plugin waits for an answer to a question.
    pub const QUESTION: Duration = Duration::from_millis(1500);

    const _: () = assert!(
        QUESTION_PLACE.as_millis() < ANSWER_WINDOW.as_millis()
            && ANSWER_WINDOW.as_millis() < QUESTION.as_millis()
    );
}
