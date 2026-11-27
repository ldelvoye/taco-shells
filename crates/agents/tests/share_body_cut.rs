use serde_json::json;
use taco_agents::{CLAUDE, CUT_MARK, EntryRead, SHARE_TOOL, activity_tool_prefix, reduced_payload};

fn shared_body_after_the_trip(body: &str) -> Option<String> {
    let tool_name = format!("{}{SHARE_TOOL}", activity_tool_prefix());
    let payload = json!({
        "hook_event_name": "PostToolUse",
        "session_id": "s1",
        "cwd": "/work",
        "tool_name": tool_name,
        "tool_use_id": "t1",
        "tool_input": {"kind": "document", "title": "Plan", "body": body},
        "tool_response": {}
    });
    let reduced = reduced_payload(&payload);
    let sent = reduced.to_string();
    let entries = CLAUDE.entries_of("PostToolUse", &sent);
    for entry in entries {
        if let EntryRead::Share { body, .. } = entry {
            return body;
        }
    }
    None
}

#[test]
fn a_shared_body_is_kept_up_to_the_readers_limit_and_marked_when_cut() {
    let fits = "x".repeat(10_000);
    let kept = shared_body_after_the_trip(&fits);
    assert_eq!(kept.as_deref(), Some(fits.as_str()));

    let too_long = "y".repeat(25_000);
    let cut = shared_body_after_the_trip(&too_long).unwrap();
    assert!(cut.ends_with(CUT_MARK), "{}", &cut[cut.len() - 20..]);
    let kept_characters = cut.chars().count() - CUT_MARK.chars().count();
    assert_eq!(kept_characters, 20_000);
}
