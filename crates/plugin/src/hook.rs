use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::identity::{CONFIG_FOLDER, PLUGIN_NAME, Surroundings};

const ATTENTION_ROUTE: &str = "/attention";
const START_ROUTE: &str = "/agent-session/start";
const END_ROUTE: &str = "/agent-session/end";
const STATE_ROUTE: &str = "/agent-session/state";
const HOOK_ROUTE: &str = "/agent-session/hook";
const HARNESS: &str = "claude";
const PORT_FILE_NAME: &str = "focus-port";
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

pub fn posts_for(payload: &Value, raw_payload: &str, pids: &[u32]) -> Vec<Post> {
    let mut posts = Vec::new();
    let Some(event) = text_field(payload, "hook_event_name") else {
        return posts;
    };

    if event == "Notification" || event == "Stop" {
        let body = json!({ "pids": pids });
        posts.push(Post {
            route: ATTENTION_ROUTE,
            body: body.to_string(),
        });
    }

    let Some(session_id) = text_field(payload, "session_id") else {
        return posts;
    };
    let Some(post) = event_post(payload, &event, &session_id, pids) else {
        return posts;
    };
    posts.push(post);

    let pids_json = json!(pids).to_string();
    let trimmed = raw_payload.trim();
    let body = format!(r#"{{"pids":{pids_json},"harness":"{HARNESS}","payload":{trimmed}}}"#);
    posts.push(Post {
        route: HOOK_ROUTE,
        body,
    });
    posts
}

pub fn posts_at_all(surroundings: &Surroundings) -> bool {
    // A headless run gets no posts, so it never becomes the terminal's newest agent session.
    if let Some(entrypoint) = &surroundings.entrypoint
        && entrypoint.starts_with("sdk")
    {
        return false;
    }

    match &surroundings.app {
        Some(app) => app == PLUGIN_NAME,
        None => true,
    }
}

pub fn port_file(
    port_file_variable: Option<OsString>,
    config_dir_variable: Option<OsString>,
    home: Option<OsString>,
) -> Option<PathBuf> {
    if let Some(path) = port_file_variable {
        return Some(PathBuf::from(path));
    }

    if let Some(directory) = config_dir_variable {
        let directory = PathBuf::from(directory);
        return Some(directory.join(PORT_FILE_NAME));
    }

    match home {
        Some(home) => {
            let home = PathBuf::from(home);
            let folder = home.join(CONFIG_FOLDER);
            Some(folder.join(PORT_FILE_NAME))
        }
        None => None,
    }
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

    const PIDS: [u32; 2] = [42, 7];

    fn run(raw: &str) -> Vec<(&'static str, Value)> {
        let payload: Value = serde_json::from_str(raw).unwrap();
        let posts = posts_for(&payload, raw, &PIDS);
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
        assert_eq!(
            routes(&notification),
            [ATTENTION_ROUTE, STATE_ROUTE, HOOK_ROUTE]
        );
        assert_eq!(notification[0].1, json!({"pids": [42, 7]}));
        assert_eq!(notification[1].1["detail"], "idle_prompt");

        let prompt = run(r#"{"hook_event_name":"UserPromptSubmit","session_id":"s1"}"#);
        assert!(prompt[0].1.get("detail").is_none());
    }

    #[test]
    fn events_without_a_session_or_a_meaning_post_little() {
        let stop_without_session = run(r#"{"hook_event_name":"Stop"}"#);
        assert_eq!(routes(&stop_without_session), [ATTENTION_ROUTE]);

        let unknown = run(r#"{"hook_event_name":"PreCompact","session_id":"s1"}"#);
        assert!(unknown.is_empty());
    }

    fn surroundings(app: Option<&str>, entrypoint: Option<&str>) -> Surroundings {
        Surroundings {
            app: app.map(String::from),
            entrypoint: entrypoint.map(String::from),
        }
    }

    #[test]
    fn posts_unless_headless_or_in_the_other_apps_terminal() {
        let other_channel = match PLUGIN_NAME {
            "taco-shells" => "taco-shells-dev",
            _ => "taco-shells",
        };

        assert!(!posts_at_all(&surroundings(
            Some(PLUGIN_NAME),
            Some("sdk-cli")
        )));
        assert!(!posts_at_all(&surroundings(
            Some(other_channel),
            Some("cli")
        )));
        assert!(posts_at_all(&surroundings(None, Some("cli"))));
        assert!(posts_at_all(&surroundings(Some(PLUGIN_NAME), None)));
    }

    #[test]
    fn port_file_prefers_the_port_variable_then_the_config_dir_then_home() {
        let port = Some(OsString::from("/p/port"));
        let config = Some(OsString::from("/c"));
        let home = Some(OsString::from("/h"));

        let from_port = port_file(port.clone(), config.clone(), home.clone());
        assert_eq!(from_port, Some(PathBuf::from("/p/port")));

        let from_config = port_file(None, config.clone(), home.clone());
        assert_eq!(from_config, Some(PathBuf::from("/c/focus-port")));

        let from_home = port_file(None, None, home);
        let expected = PathBuf::from("/h").join(CONFIG_FOLDER).join("focus-port");
        assert_eq!(from_home, Some(expected));

        assert_eq!(port_file(None, None, None), None);
    }
}
