use serde_json::value::RawValue;
use taco_agents::{Harness, harness_of};
use taco_protocol::routes::{AGENT_HOOK, SESSION_END, SESSION_START, SESSION_STATE};
use taco_protocol::{
    AgentHookBody, HookCall, HookMessage, SessionEndBody, SessionStartBody, SessionStateBody,
};

use super::is_any_channels_activity_call;

pub(super) struct ParsedHook {
    pub(super) message: HookMessage,
    pub(super) source: String,
}

pub(super) fn parse_message(
    route: &str,
    body: &[u8],
    sent_token: Option<&str>,
    token: &str,
) -> Option<HookMessage> {
    match route {
        AGENT_HOOK => {
            let parsed = parse_hook_message(body, sent_token, token)?;
            Some(parsed.message)
        }
        SESSION_START => parse_start(body),
        SESSION_END => parse_end(body),
        SESSION_STATE => parse_state(body),
        _ => None,
    }
}

fn parse_start(body: &[u8]) -> Option<HookMessage> {
    let parsed: SessionStartBody = serde_json::from_slice(body).ok()?;
    let harness = harness_of(&parsed.harness.0)?;
    let pids = non_empty(parsed.pids)?;
    let session_id = checked_session_id(harness, parsed.session_id)?;
    if parsed.cwd.is_empty() {
        return None;
    }
    let call = HookCall::AgentStart {
        harness: harness.id(),
        session_id,
        cwd: parsed.cwd,
        has_conversation: parsed.has_conversation,
    };
    Some(HookMessage { pids, call })
}

fn parse_end(body: &[u8]) -> Option<HookMessage> {
    let parsed: SessionEndBody = serde_json::from_slice(body).ok()?;
    let harness = harness_of(&parsed.harness.0)?;
    let pids = non_empty(parsed.pids)?;
    let session_id = checked_session_id(harness, parsed.session_id)?;
    let call = HookCall::AgentEnd {
        harness: harness.id(),
        session_id,
    };
    Some(HookMessage { pids, call })
}

fn parse_state(body: &[u8]) -> Option<HookMessage> {
    let parsed: SessionStateBody = serde_json::from_slice(body).ok()?;
    let harness = harness_of(&parsed.harness.0)?;
    let pids = non_empty(parsed.pids)?;
    let session_id = checked_session_id(harness, parsed.session_id)?;
    let mut detail = None;
    if let Some(text) = &parsed.detail
        && !text.is_empty()
    {
        detail = Some(text.as_str());
    }
    let signal = harness.signal_for(&parsed.event, detail)?;
    let call = HookCall::AgentState {
        harness: harness.id(),
        session_id,
        signal,
    };
    Some(HookMessage { pids, call })
}

pub(super) fn parse_hook_message(
    body: &[u8],
    sent_token: Option<&str>,
    token: &str,
) -> Option<ParsedHook> {
    let parsed: AgentHookBody<Box<RawValue>> = serde_json::from_slice(body).ok()?;
    let harness = harness_of(&parsed.harness.0)?;
    let pids = non_empty(parsed.pids)?;

    let payload_text = parsed.payload.get();
    let fields = harness.hook_fields(payload_text)?;

    let tool_name = fields.tool_name.as_deref();
    let is_activity_call = tool_name.is_some_and(is_any_channels_activity_call);
    // Any local process can post here, so an activity call counts only with the token the app wrote for its own hook.
    if is_activity_call && sent_token != Some(token) {
        return None;
    }

    let session_id = checked_session_id(harness, fields.session_id)?;
    let mut source = String::new();
    if let Some(text) = fields.source {
        source = text;
    }

    let call = HookCall::AgentHook {
        harness: harness.id(),
        session_id,
        event: fields.event,
        payload: payload_text.to_string(),
    };
    let message = HookMessage { pids, call };
    Some(ParsedHook { message, source })
}

fn non_empty(pids: Vec<u32>) -> Option<Vec<u32>> {
    if pids.is_empty() {
        return None;
    }
    Some(pids)
}

fn checked_session_id(harness: &Harness, session_id: String) -> Option<String> {
    if !harness.is_session_id(&session_id) {
        return None;
    }
    Some(session_id)
}
