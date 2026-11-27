use std::path::Path;

use serde_json::{Map, Value, json};

use crate::identity::tool_name_prefix;
use crate::panel_text::PANEL_TEXT;
use crate::post::PostError;
use crate::tools::TOOL_NAMES;

const START_ROUTE: &str = "/agent-session/start";
const END_ROUTE: &str = "/agent-session/end";
const STATE_ROUTE: &str = "/agent-session/state";
const HOOK_ROUTE: &str = "/agent-session/hook";
pub const CONTEXT_ROUTE: &str = "/agent-session/context";
const STOP_ROUTE: &str = "/agent-session/stop";
const HARNESS: &str = "claude";
const TOKEN_FILE_NAME: &str = "focus-token";
const FAILURE_EVENT: &str = "PostToolUseFailure";
// One character over the largest limit the readers keep (a shared body), so a text cut here still shows as cut.
const STRING_CAP: usize = 20_001;
const COMMON_KEYS: [&str; 6] = [
    "hook_event_name",
    "session_id",
    "prompt_id",
    "agent_id",
    "cwd",
    "source",
];
const TOOL_SUMMARY_KEYS: [&str; 7] = [
    "file_path",
    "notebook_path",
    "command",
    "pattern",
    "url",
    "query",
    "description",
];
const POST_TOOL_KEYS: [&str; 3] = ["tool_name", "tool_use_id", "scratchpad_dir"];
const ACTIVITY_INPUT_KEYS: [&str; 5] = ["text", "kind", "title", "target", "body"];
const STATE_EVENTS: [&str; 6] = [
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Notification",
    "Stop",
    "StopFailure",
];

#[derive(Debug, PartialEq)]
pub struct Post {
    pub route: &'static str,
    pub body: String,
}

#[derive(Debug, PartialEq)]
pub struct Question {
    pub route: &'static str,
    pub body: String,
}

fn text_field(payload: &Value, key: &str) -> Option<String> {
    let value = payload.get(key)?;
    let text = value.as_str()?;
    if text.is_empty() {
        return None;
    }
    Some(String::from(text))
}

fn event_post(payload: &Value, event: &str, session_id: &str, pids: &[u32]) -> Option<Post> {
    if event == "SessionStart" {
        let cwd = text_field(payload, "cwd")?;
        let source = text_field(payload, "source");
        let has_conversation =
            source.as_deref() == Some("resume") || source.as_deref() == Some("compact");
        let body = json!({
            "pids": pids,
            "harness": HARNESS,
            "sessionId": session_id,
            "cwd": cwd,
            "hasConversation": has_conversation,
        });
        return Some(Post {
            route: START_ROUTE,
            body: body.to_string(),
        });
    }

    if event == "SessionEnd" {
        let body = json!({
            "pids": pids,
            "harness": HARNESS,
            "sessionId": session_id,
        });
        return Some(Post {
            route: END_ROUTE,
            body: body.to_string(),
        });
    }

    if !STATE_EVENTS.contains(&event) {
        return None;
    }

    let detail_key = match event {
        "PreToolUse" | "PostToolUse" => Some("tool_name"),
        "Notification" => Some("notification_type"),
        _ => None,
    };
    let mut detail = None;
    if let Some(key) = detail_key {
        detail = text_field(payload, key);
    }

    let mut body = json!({
        "pids": pids,
        "harness": HARNESS,
        "sessionId": session_id,
        "event": event,
    });
    if let Some(detail) = detail {
        body["detail"] = Value::String(detail);
    }
    Some(Post {
        route: STATE_ROUTE,
        body: body.to_string(),
    })
}

fn copy_keys(source: Option<&Value>, keys: &[&str], into: &mut Map<String, Value>) {
    let Some(source) = source else {
        return;
    };
    for key in keys {
        if let Some(value) = source.get(*key) {
            into.insert(key.to_string(), value.clone());
        }
    }
}

fn attach(kept: &mut Map<String, Value>, key: &str, fields: Map<String, Value>) {
    if fields.is_empty() {
        return;
    }
    kept.insert(key.to_string(), Value::Object(fields));
}

fn is_activity_tool(tool_name: &str) -> bool {
    let prefix = tool_name_prefix();
    TOOL_NAMES.iter().any(|name| {
        let activity_tool = format!("{prefix}{name}");
        activity_tool == tool_name
    })
}

fn first_question(payload: &Value) -> Option<Value> {
    let input = payload.get("tool_input")?;
    let questions = input.get("questions")?;
    let first = questions.as_array()?.first()?;
    let question = first.get("question")?;
    Some(json!({ "question": question }))
}

fn keep_pre_tool(payload: &Value, kept: &mut Map<String, Value>) {
    copy_keys(Some(payload), &["tool_name"], kept);
    let Some(tool_name) = text_field(payload, "tool_name") else {
        return;
    };
    if tool_name != "AskUserQuestion" {
        return;
    }
    if let Some(question) = first_question(payload) {
        let kept_input = json!({ "questions": [question] });
        kept.insert(String::from("tool_input"), kept_input);
    }
}

fn keep_post_tool(payload: &Value, kept: &mut Map<String, Value>) {
    copy_keys(Some(payload), &POST_TOOL_KEYS, kept);

    let mut input = Map::new();
    let mut response = Map::new();
    let tool_input = payload.get("tool_input");
    let tool_response = payload.get("tool_response");
    copy_keys(tool_input, &TOOL_SUMMARY_KEYS, &mut input);

    let tool_name = text_field(payload, "tool_name");
    if let Some(tool_name) = tool_name {
        match tool_name.as_str() {
            "SendMessage" => {
                copy_keys(tool_input, &["to", "message"], &mut input);
                copy_keys(tool_response, &["success", "display"], &mut response);
            }
            "Bash" => copy_keys(tool_response, &["gitOperation"], &mut response),
            "Write" => copy_keys(tool_response, &["type", "filePath"], &mut response),
            "Artifact" => {
                copy_keys(tool_input, &["action", "asset"], &mut input);
                copy_keys(tool_response, &["url", "title", "updated"], &mut response);
            }
            name if is_activity_tool(name) => {
                copy_keys(tool_input, &ACTIVITY_INPUT_KEYS, &mut input);
                copy_keys(tool_response, &["isError"], &mut response);
            }
            _ => {}
        }
    }

    attach(kept, "tool_input", input);
    attach(kept, "tool_response", response);
}

fn cap_strings(value: Value) -> Value {
    match value {
        Value::String(text) => {
            let cut: String = text.chars().take(STRING_CAP).collect();
            Value::String(cut)
        }
        Value::Array(items) => {
            let capped: Vec<Value> = items.into_iter().map(cap_strings).collect();
            Value::Array(capped)
        }
        Value::Object(fields) => {
            let pairs = fields.into_iter();
            let capped_pairs = pairs.map(|(key, field)| (key, cap_strings(field)));
            let capped: Map<String, Value> = capped_pairs.collect();
            Value::Object(capped)
        }
        other => other,
    }
}

pub fn reduced_payload(payload: &Value) -> Value {
    let mut kept = Map::new();
    copy_keys(Some(payload), &COMMON_KEYS, &mut kept);

    let event = text_field(payload, "hook_event_name");
    if let Some(event) = event {
        match event.as_str() {
            "UserPromptSubmit" => copy_keys(Some(payload), &["prompt"], &mut kept),
            "Notification" => {
                copy_keys(Some(payload), &["notification_type", "message"], &mut kept);
            }
            "PreToolUse" => keep_pre_tool(payload, &mut kept),
            "PostToolUse" => keep_post_tool(payload, &mut kept),
            FAILURE_EVENT => {
                keep_post_tool(payload, &mut kept);
                copy_keys(Some(payload), &["error", "is_interrupt"], &mut kept);
            }
            _ => {}
        }
    }

    cap_strings(Value::Object(kept))
}

pub fn posts_for(payload: &Value, pids: &[u32]) -> Vec<Post> {
    let mut posts = Vec::new();
    let Some(event) = text_field(payload, "hook_event_name") else {
        return posts;
    };

    let Some(session_id) = text_field(payload, "session_id") else {
        return posts;
    };
    let state_post = event_post(payload, &event, &session_id, pids);
    match state_post {
        Some(post) => posts.push(post),
        None if event == FAILURE_EVENT => {}
        None => return posts,
    }

    posts.push(Post {
        route: HOOK_ROUTE,
        body: hook_body(payload, pids),
    });
    posts
}

fn hook_body(payload: &Value, pids: &[u32]) -> String {
    let reduced = reduced_payload(payload);
    let body = json!({
        "pids": pids,
        "harness": HARNESS,
        "payload": reduced,
    });
    body.to_string()
}

pub fn question_for(payload: &Value, pids: &[u32]) -> Option<Question> {
    let event = text_field(payload, "hook_event_name")?;
    let route = match event.as_str() {
        "SessionStart" => CONTEXT_ROUTE,
        "Stop" => STOP_ROUTE,
        _ => return None,
    };
    let already_continued = payload.get("stop_hook_active") == Some(&Value::Bool(true));
    if route == STOP_ROUTE && already_continued {
        return None;
    }
    Some(Question {
        route,
        body: hook_body(payload, pids),
    })
}

pub fn session_start_output(answer: &Value) -> Option<String> {
    if answer["yours"] != Value::Bool(true) {
        return None;
    }
    let mut text = String::from(PANEL_TEXT);
    let context = text_field(answer, "context");
    if let Some(context) = context {
        text.push_str("\n\n");
        text.push_str(&context);
    }
    let output = json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    });
    Some(output.to_string())
}

pub fn stop_output(answer: &Value) -> Option<String> {
    let reason = text_field(answer, "reason")?;
    let output = json!({ "decision": "block", "reason": reason });
    Some(output.to_string())
}

pub fn send_posts(posts: &[Post], mut send: impl FnMut(&Post) -> Result<(), PostError>) -> usize {
    let mut attempted = 0;
    for next in posts {
        attempted += 1;
        if send(next).is_err() {
            break;
        }
    }
    attempted
}

pub fn read_token(port_path: &Path) -> Option<String> {
    let token_path = port_path.with_file_name(TOKEN_FILE_NAME);
    let content = std::fs::read_to_string(token_path).ok()?;
    let first_line = content.lines().next()?;
    let trimmed = first_line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(String::from(trimmed))
}

pub fn read_port(path: &Path) -> Option<u16> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    let first_line = lines.next()?;
    let trimmed = first_line.trim();
    trimmed.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::PLUGIN_NAME;

    const PIDS: [u32; 2] = [42, 7];

    fn run(raw: &str) -> Vec<(&'static str, Value)> {
        let payload: Value = serde_json::from_str(raw).unwrap();
        let posts = posts_for(&payload, &PIDS);
        posts
            .into_iter()
            .map(|post| (post.route, serde_json::from_str(&post.body).unwrap()))
            .collect()
    }

    fn routes(posts: &[(&'static str, Value)]) -> Vec<&'static str> {
        posts.iter().map(|(route, _)| *route).collect()
    }

    #[test]
    fn session_start_carries_cwd_and_whether_a_conversation_exists() {
        let resumed = run(
            r#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/work","source":"resume"}"#,
        );
        assert_eq!(routes(&resumed), [START_ROUTE, HOOK_ROUTE]);
        let expected = json!({
            "pids": [42, 7],
            "harness": "claude",
            "sessionId": "s1",
            "cwd": "/work",
            "hasConversation": true,
        });
        assert_eq!(resumed[0].1, expected);
        assert_eq!(resumed[1].1["payload"]["source"], "resume");
        assert_eq!(resumed[1].1["pids"], json!([42, 7]));

        let compacted = run(
            r#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/work","source":"compact"}"#,
        );
        assert_eq!(compacted[0].1["hasConversation"], true);

        let fresh = run(
            r#"{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/work","source":"startup"}"#,
        );
        assert_eq!(fresh[0].1["hasConversation"], false);

        let no_cwd = run(r#"{"hook_event_name":"SessionStart","session_id":"s1"}"#);
        assert!(no_cwd.is_empty());
    }

    #[test]
    fn a_cwd_with_quotes_and_backslashes_survives() {
        let cwd = r#"/tmp/a "b"\c"#;
        let raw = json!({"hook_event_name": "SessionStart", "session_id": "s1", "cwd": cwd});
        let posts = run(&raw.to_string());
        assert_eq!(posts[0].1["cwd"], cwd);
    }

    #[test]
    fn state_events_carry_their_detail() {
        let tool = run(r#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash"}"#);
        assert_eq!(routes(&tool), [STATE_ROUTE, HOOK_ROUTE]);
        assert_eq!(tool[0].1["detail"], "Bash");

        let notification = run(
            r#"{"hook_event_name":"Notification","session_id":"s1","notification_type":"idle_prompt"}"#,
        );
        assert_eq!(routes(&notification), [STATE_ROUTE, HOOK_ROUTE]);
        assert_eq!(notification[0].1["detail"], "idle_prompt");

        let prompt = run(r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1"}"#);
        assert!(prompt[0].1.get("detail").is_none());
    }

    #[test]
    fn events_without_a_session_or_a_meaning_post_little() {
        let stop_without_session = run(r#"{"hook_event_name":"Stop"}"#);
        assert!(stop_without_session.is_empty());

        let unknown = run(r#"{"hook_event_name":"PreCompact","session_id":"s1"}"#);
        assert!(unknown.is_empty());
    }

    #[test]
    fn a_failed_tool_posts_only_the_hook_route() {
        let failed = run(
            r#"{"hook_event_name":"PostToolUseFailure","session_id":"s1","tool_name":"Bash","error":"boom"}"#,
        );
        assert_eq!(routes(&failed), [HOOK_ROUTE]);
        assert_eq!(failed[0].1["payload"]["error"], "boom");
    }

    #[test]
    fn the_reduced_payload_keeps_what_the_app_reads() {
        let activity_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__note");

        let write = reduced_payload(&json!({
            "hook_event_name": "PostToolUse",
            "session_id": "s1",
            "tool_name": "Write",
            "tool_input": {"file_path": "/a.rs", "content": "secret body"},
            "tool_response": {"type": "create", "filePath": "/a.rs", "content": "secret body"},
        }));
        let expected_write = json!({
            "hook_event_name": "PostToolUse",
            "session_id": "s1",
            "tool_name": "Write",
            "tool_input": {"file_path": "/a.rs"},
            "tool_response": {"type": "create", "filePath": "/a.rs"},
        });
        assert_eq!(write, expected_write);

        let bash = reduced_payload(&json!({
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "git push"},
            "tool_response": {"stdout": "lots", "gitOperation": {"kind": "push"}},
        }));
        assert_eq!(bash["tool_input"]["command"], "git push");
        assert_eq!(
            bash["tool_response"],
            json!({"gitOperation": {"kind": "push"}})
        );

        let note = reduced_payload(&json!({
            "hook_event_name": "PostToolUse",
            "tool_name": activity_tool,
            "tool_input": {"text": "did a thing", "other": 1},
            "tool_response": {"isError": false, "content": "x"},
        }));
        assert_eq!(note["tool_input"], json!({"text": "did a thing"}));
        assert_eq!(note["tool_response"], json!({"isError": false}));

        let question = reduced_payload(&json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "AskUserQuestion",
            "tool_input": {"questions": [
                {"question": "First?", "options": ["a"]},
                {"question": "Second?"},
            ]},
        }));
        assert_eq!(
            question["tool_input"],
            json!({"questions": [{"question": "First?"}]})
        );

        let long_prompt = "é".repeat(STRING_CAP + 500);
        let prompt = reduced_payload(&json!({
            "hook_event_name": "UserPromptSubmit",
            "prompt": long_prompt,
        }));
        assert_eq!(
            prompt["prompt"].as_str().unwrap().chars().count(),
            STRING_CAP
        );

        let failure = reduced_payload(&json!({
            "hook_event_name": "PostToolUseFailure",
            "tool_name": "Bash",
            "error": "boom",
            "is_interrupt": true,
            "tool_input": {"command": "ls", "timeout": 5},
        }));
        assert_eq!(failure["error"], "boom");
        assert_eq!(failure["is_interrupt"], true);
        assert!(failure["tool_input"].get("timeout").is_none());
    }

    #[test]
    fn the_token_is_the_first_line_of_the_file_beside_the_port_file() {
        let directory = tempfile::tempdir().unwrap();
        let port_path = directory.path().join("focus-port");
        assert_eq!(read_token(&port_path), None);

        let token_path = directory.path().join(TOKEN_FILE_NAME);
        std::fs::write(token_path, "abc123\nignored\n").unwrap();
        assert_eq!(read_token(&port_path), Some(String::from("abc123")));
    }

    #[test]
    fn sending_stops_at_the_first_failure() {
        let posts: Vec<Post> = (0..3)
            .map(|index| Post {
                route: STATE_ROUTE,
                body: index.to_string(),
            })
            .collect();

        let mut sent = Vec::new();
        let attempted = send_posts(&posts, |post| {
            sent.push(post.body.clone());
            Ok(())
        });
        assert_eq!(attempted, 3);
        assert_eq!(sent, ["0", "1", "2"]);

        let mut sent = Vec::new();
        let attempted = send_posts(&posts, |post| {
            sent.push(post.body.clone());
            if sent.len() == 2 {
                return Err(PostError::Other);
            }
            Ok(())
        });
        assert_eq!(attempted, 2);
        assert_eq!(sent, ["0", "1"]);
    }

    #[test]
    fn only_a_session_start_and_a_first_stop_ask_the_app_and_they_send_the_hook_posts_body() {
        let start = json!({"hook_event_name": "SessionStart", "session_id": "s1", "cwd": "/w", "source": "compact"});
        let stop = json!({"hook_event_name": "Stop", "session_id": "s1"});
        let looping =
            json!({"hook_event_name": "Stop", "session_id": "s1", "stop_hook_active": true});
        let tool =
            json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash"});

        let started = question_for(&start, &PIDS).unwrap();
        assert_eq!(started.route, CONTEXT_ROUTE);
        let stopped = question_for(&stop, &PIDS).unwrap();
        assert_eq!(stopped.route, STOP_ROUTE);
        assert_eq!(question_for(&looping, &PIDS), None);
        assert_eq!(question_for(&tool, &PIDS), None);

        let posts = posts_for(&start, &PIDS);
        let hook_post = posts.last().unwrap();
        assert_eq!(hook_post.route, HOOK_ROUTE);
        assert_eq!(started.body, hook_post.body);
    }

    #[test]
    fn the_session_start_output_hands_over_the_panel_text_and_the_apps_context() {
        assert_eq!(session_start_output(&json!({})), None);

        let bare = session_start_output(&json!({"yours": true})).unwrap();
        let parsed: Value = serde_json::from_str(&bare).unwrap();
        let expected = json!({"hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": PANEL_TEXT,
        }});
        assert_eq!(parsed, expected);

        let with_context = session_start_output(&json!({"yours": true, "context": "Earlier: x"}));
        let parsed: Value = serde_json::from_str(&with_context.unwrap()).unwrap();
        let text = format!("{PANEL_TEXT}\n\nEarlier: x");
        assert_eq!(parsed["hookSpecificOutput"]["additionalContext"], text);
    }

    #[test]
    fn a_stop_is_blocked_only_with_a_reason() {
        assert_eq!(stop_output(&json!({})), None);
        assert_eq!(stop_output(&json!({"reason": ""})), None);

        let blocked = stop_output(&json!({"reason": "write a note"})).unwrap();
        let parsed: Value = serde_json::from_str(&blocked).unwrap();
        assert_eq!(
            parsed,
            json!({"decision": "block", "reason": "write a note"})
        );
    }
}
