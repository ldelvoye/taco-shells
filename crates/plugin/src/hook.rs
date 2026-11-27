use std::path::Path;

use serde_json::{Value, json};
use taco_agents::reduced_payload;
use taco_protocol::routes::{AGENT_HOOK, CONTEXT, SESSION_END, SESSION_START, SESSION_STATE, STOP};
use taco_protocol::{
    AgentHookBody, ContextAnswer, HarnessId, SessionEndBody, SessionStartBody, SessionStateBody,
    StopAnswer, TOKEN_FILE_NAME,
};

use crate::panel_text::PANEL_TEXT;
use crate::post::PostError;

const HARNESS: &str = "claude";
const FAILURE_EVENT: &str = "PostToolUseFailure";
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

fn harness_id() -> HarnessId {
    HarnessId(String::from(HARNESS))
}

fn event_post(payload: &Value, event: &str, session_id: &str, pids: &[u32]) -> Option<Post> {
    if event == "SessionStart" {
        let cwd = text_field(payload, "cwd")?;
        let source = text_field(payload, "source");
        let has_conversation =
            source.as_deref() == Some("resume") || source.as_deref() == Some("compact");
        let body = SessionStartBody {
            pids: pids.to_vec(),
            harness: harness_id(),
            session_id: String::from(session_id),
            cwd,
            has_conversation,
        };
        let text = serde_json::to_string(&body).ok()?;
        return Some(Post {
            route: SESSION_START,
            body: text,
        });
    }

    if event == "SessionEnd" {
        let body = SessionEndBody {
            pids: pids.to_vec(),
            harness: harness_id(),
            session_id: String::from(session_id),
        };
        let text = serde_json::to_string(&body).ok()?;
        return Some(Post {
            route: SESSION_END,
            body: text,
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

    let body = SessionStateBody {
        pids: pids.to_vec(),
        harness: harness_id(),
        session_id: String::from(session_id),
        event: String::from(event),
        detail,
    };
    let text = serde_json::to_string(&body).ok()?;
    Some(Post {
        route: SESSION_STATE,
        body: text,
    })
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

    if let Some(body) = hook_body(payload, pids) {
        posts.push(Post {
            route: AGENT_HOOK,
            body,
        });
    }
    posts
}

fn hook_body(payload: &Value, pids: &[u32]) -> Option<String> {
    let reduced = reduced_payload(payload);
    let body = AgentHookBody {
        pids: pids.to_vec(),
        harness: harness_id(),
        payload: reduced,
    };
    serde_json::to_string(&body).ok()
}

pub fn question_for(payload: &Value, pids: &[u32]) -> Option<Question> {
    let event = text_field(payload, "hook_event_name")?;
    let route = match event.as_str() {
        "SessionStart" => CONTEXT,
        "Stop" => STOP,
        _ => return None,
    };
    let already_continued = payload.get("stop_hook_active") == Some(&Value::Bool(true));
    if route == STOP && already_continued {
        return None;
    }
    let body = hook_body(payload, pids)?;
    Some(Question { route, body })
}

pub fn session_start_output(answer: &ContextAnswer) -> Option<String> {
    if !answer.yours {
        return None;
    }
    let mut text = String::from(PANEL_TEXT);
    if let Some(context) = &answer.context
        && !context.is_empty()
    {
        text.push_str("\n\n");
        text.push_str(context);
    }
    let output = json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    });
    Some(output.to_string())
}

pub fn stop_output(answer: &StopAnswer) -> Option<String> {
    let reason = answer.reason.as_ref()?;
    if reason.is_empty() {
        return None;
    }
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
    use taco_protocol::PORT_FILE_NAME;

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
        assert_eq!(routes(&resumed), [SESSION_START, AGENT_HOOK]);
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
        assert_eq!(routes(&tool), [SESSION_STATE, AGENT_HOOK]);
        assert_eq!(tool[0].1["detail"], "Bash");

        let notification = run(
            r#"{"hook_event_name":"Notification","session_id":"s1","notification_type":"idle_prompt"}"#,
        );
        assert_eq!(routes(&notification), [SESSION_STATE, AGENT_HOOK]);
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
        assert_eq!(routes(&failed), [AGENT_HOOK]);
        assert_eq!(failed[0].1["payload"]["error"], "boom");
    }

    #[test]
    fn the_token_is_the_first_line_of_the_file_beside_the_port_file() {
        let directory = tempfile::tempdir().unwrap();
        let port_path = directory.path().join(PORT_FILE_NAME);
        assert_eq!(read_token(&port_path), None);

        let token_path = directory.path().join(TOKEN_FILE_NAME);
        std::fs::write(token_path, "abc123\nignored\n").unwrap();
        assert_eq!(read_token(&port_path), Some(String::from("abc123")));
    }

    #[test]
    fn sending_stops_at_the_first_failure() {
        let posts: Vec<Post> = (0..3)
            .map(|index| Post {
                route: SESSION_STATE,
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
        assert_eq!(started.route, CONTEXT);
        let stopped = question_for(&stop, &PIDS).unwrap();
        assert_eq!(stopped.route, STOP);
        assert_eq!(question_for(&looping, &PIDS), None);
        assert_eq!(question_for(&tool, &PIDS), None);

        let posts = posts_for(&start, &PIDS);
        let hook_post = posts.last().unwrap();
        assert_eq!(hook_post.route, AGENT_HOOK);
        assert_eq!(started.body, hook_post.body);
    }

    #[test]
    fn the_session_start_output_hands_over_the_panel_text_and_the_apps_context() {
        assert_eq!(session_start_output(&ContextAnswer::default()), None);

        let bare_answer = ContextAnswer {
            yours: true,
            context: None,
        };
        let bare = session_start_output(&bare_answer).unwrap();
        let parsed: Value = serde_json::from_str(&bare).unwrap();
        let expected = json!({"hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": PANEL_TEXT,
        }});
        assert_eq!(parsed, expected);

        let context_answer = ContextAnswer {
            yours: true,
            context: Some(String::from("Earlier: x")),
        };
        let with_context = session_start_output(&context_answer);
        let parsed: Value = serde_json::from_str(&with_context.unwrap()).unwrap();
        let text = format!("{PANEL_TEXT}\n\nEarlier: x");
        assert_eq!(parsed["hookSpecificOutput"]["additionalContext"], text);
    }

    #[test]
    fn a_stop_is_blocked_only_with_a_reason() {
        assert_eq!(stop_output(&StopAnswer::default()), None);
        let empty = StopAnswer {
            reason: Some(String::new()),
        };
        assert_eq!(stop_output(&empty), None);

        let note = StopAnswer {
            reason: Some(String::from("write a note")),
        };
        let blocked = stop_output(&note).unwrap();
        let parsed: Value = serde_json::from_str(&blocked).unwrap();
        assert_eq!(
            parsed,
            json!({"decision": "block", "reason": "write a note"})
        );
    }
}
