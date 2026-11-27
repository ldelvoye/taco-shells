use serde_json::{Map, Value, json};

use super::entries::{
    MAX_FAILURE_CHARACTERS, MAX_NOW_CHARACTERS, MAX_WRITTEN_CHARACTERS, activity_tool_of,
};
use super::shared::{MAX_BODY_CHARACTERS, MAX_TARGET_CHARACTERS, MAX_TITLE_CHARACTERS};
use super::text::MAX_ENTRY_CHARACTERS;
use super::{MAX_NAME_CHARACTERS, MAX_QUESTION_CHARACTERS};

const FAILURE_EVENT: &str = "PostToolUseFailure";
const LONGEST_READ: usize = largest(&[
    MAX_BODY_CHARACTERS,
    MAX_TARGET_CHARACTERS,
    MAX_TITLE_CHARACTERS,
    MAX_WRITTEN_CHARACTERS,
    MAX_NOW_CHARACTERS,
    MAX_FAILURE_CHARACTERS,
    MAX_QUESTION_CHARACTERS,
    MAX_NAME_CHARACTERS,
    MAX_ENTRY_CHARACTERS,
]);
// One character over the longest text any reader keeps, so a text cut here still shows as cut.
const STRING_CAP: usize = LONGEST_READ + 1;
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

const fn largest(limits: &[usize]) -> usize {
    let mut longest = 0;
    let mut index = 0;
    while index < limits.len() {
        if limits[index] > longest {
            longest = limits[index];
        }
        index += 1;
    }
    longest
}

fn text_field(payload: &Value, key: &str) -> Option<String> {
    let value = payload.get(key)?;
    let text = value.as_str()?;
    if text.is_empty() {
        return None;
    }
    Some(String::from(text))
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
            name if activity_tool_of(name).is_some() => {
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

#[cfg(test)]
mod tests {
    use super::super::entries::{NOTE_TOOL, activity_tool_prefix};
    use super::*;

    #[test]
    fn the_reduced_payload_keeps_what_the_app_reads() {
        let activity_tool = format!("{}{NOTE_TOOL}", activity_tool_prefix());

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
}
