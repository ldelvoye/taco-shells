use serde_json::value::RawValue;
use serde_json::{Value, json};
use taco_plugin::{posts_for, question_for};
use taco_protocol::routes::{AGENT_HOOK, CONTEXT, SESSION_END, SESSION_START, SESSION_STATE, STOP};
use taco_protocol::{
    AgentHookBody, ContextAnswer, HarnessId, SessionEndBody, SessionStartBody, SessionStateBody,
    StopAnswer,
};

const PIDS: [u32; 2] = [42, 7];

const START: &str =
    r#"{"pids":[42,7],"harness":"claude","sessionId":"s1","cwd":"/work","hasConversation":true}"#;
const END: &str = r#"{"pids":[42,7],"harness":"claude","sessionId":"s1"}"#;
const STATE_WITH_DETAIL: &str =
    r#"{"pids":[42,7],"harness":"claude","sessionId":"s1","event":"PreToolUse","detail":"Bash"}"#;
const STATE_WITHOUT_DETAIL: &str =
    r#"{"pids":[42,7],"harness":"claude","sessionId":"s1","event":"UserPromptSubmit"}"#;
const HOOK: &str = r#"{"pids":[42,7],"harness":"claude","payload":{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash"}}"#;
const CONTEXT_QUESTION: &str = r#"{"pids":[42,7],"harness":"claude","payload":{"hook_event_name":"SessionStart","session_id":"s1","cwd":"/work","source":"resume"}}"#;
const STOP_QUESTION: &str =
    r#"{"pids":[42,7],"harness":"claude","payload":{"hook_event_name":"Stop","session_id":"s1"}}"#;

fn claude() -> HarnessId {
    HarnessId(String::from("claude"))
}

fn as_value(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn built_post(payload: Value, route: &str) -> Value {
    let posts = posts_for(&payload, &PIDS);
    let found = posts.iter().find(|post| post.route == route).unwrap();
    as_value(&found.body)
}

fn built_question(payload: Value, route: &str) -> Value {
    let question = question_for(&payload, &PIDS).unwrap();
    assert_eq!(question.route, route);
    as_value(&question.body)
}

macro_rules! round_trips {
    ($kind:ty, $literal:expr, $expected:expr) => {{
        let parsed: $kind = serde_json::from_str($literal).unwrap();
        assert_eq!(parsed, $expected, "{}", $literal);
        let written = serde_json::to_string(&parsed).unwrap();
        assert_eq!(written, $literal);
    }};
}

#[test]
fn the_wire_bodies_and_answers_are_pinned() {
    round_trips!(
        SessionStartBody,
        START,
        SessionStartBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            session_id: String::from("s1"),
            cwd: String::from("/work"),
            has_conversation: true,
        }
    );
    round_trips!(
        SessionEndBody,
        END,
        SessionEndBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            session_id: String::from("s1"),
        }
    );
    round_trips!(
        SessionStateBody,
        STATE_WITH_DETAIL,
        SessionStateBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            session_id: String::from("s1"),
            event: String::from("PreToolUse"),
            detail: Some(String::from("Bash")),
        }
    );
    round_trips!(
        SessionStateBody,
        STATE_WITHOUT_DETAIL,
        SessionStateBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            session_id: String::from("s1"),
            event: String::from("UserPromptSubmit"),
            detail: None,
        }
    );
    let hook_payload =
        json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash"});
    round_trips!(
        AgentHookBody<Value>,
        HOOK,
        AgentHookBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            payload: hook_payload,
        }
    );

    let raw: AgentHookBody<Box<RawValue>> = serde_json::from_str(HOOK).unwrap();
    let raw_payload = r#"{"hook_event_name":"PreToolUse","session_id":"s1","tool_name":"Bash"}"#;
    assert_eq!(raw.payload.get(), raw_payload);

    let start_payload = json!({"hook_event_name": "SessionStart", "session_id": "s1", "cwd": "/work", "source": "resume"});
    let end_payload = json!({"hook_event_name": "SessionEnd", "session_id": "s1"});
    let tool_payload =
        json!({"hook_event_name": "PreToolUse", "session_id": "s1", "tool_name": "Bash"});
    let prompt_payload = json!({"hook_event_name": "UserPromptSubmit", "session_id": "s1"});
    let stop_payload = json!({"hook_event_name": "Stop", "session_id": "s1"});
    assert_eq!(
        built_post(start_payload.clone(), SESSION_START),
        as_value(START)
    );
    assert_eq!(built_post(end_payload, SESSION_END), as_value(END));
    let with_detail = as_value(STATE_WITH_DETAIL);
    assert_eq!(built_post(tool_payload.clone(), SESSION_STATE), with_detail);
    let without_detail = as_value(STATE_WITHOUT_DETAIL);
    assert_eq!(built_post(prompt_payload, SESSION_STATE), without_detail);
    assert_eq!(built_post(tool_payload, AGENT_HOOK), as_value(HOOK));
    assert_eq!(
        built_question(start_payload, CONTEXT),
        as_value(CONTEXT_QUESTION)
    );
    assert_eq!(built_question(stop_payload, STOP), as_value(STOP_QUESTION));
    round_trips!(
        AgentHookBody<Value>,
        CONTEXT_QUESTION,
        AgentHookBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            payload: json!({"hook_event_name": "SessionStart", "session_id": "s1", "cwd": "/work", "source": "resume"}),
        }
    );
    round_trips!(
        AgentHookBody<Value>,
        STOP_QUESTION,
        AgentHookBody {
            pids: PIDS.to_vec(),
            harness: claude(),
            payload: json!({"hook_event_name": "Stop", "session_id": "s1"}),
        }
    );

    round_trips!(
        ContextAnswer,
        r#"{"yours":true,"context":"x"}"#,
        ContextAnswer {
            yours: true,
            context: Some(String::from("x")),
        }
    );
    round_trips!(
        ContextAnswer,
        r#"{"yours":true}"#,
        ContextAnswer {
            yours: true,
            context: None,
        }
    );
    round_trips!(ContextAnswer, "{}", ContextAnswer::default());
    round_trips!(
        StopAnswer,
        r#"{"reason":"write a note"}"#,
        StopAnswer {
            reason: Some(String::from("write a note")),
        }
    );
    round_trips!(StopAnswer, "{}", StopAnswer::default());

    let extra = r#"{"pids":[42,7],"harness":"claude","sessionId":"s1","cwd":"/work","hasConversation":true,"later":{"added":1}}"#;
    let tolerated: SessionStartBody = serde_json::from_str(extra).unwrap();
    let expected: SessionStartBody = serde_json::from_str(START).unwrap();
    assert_eq!(tolerated, expected);

    for pids in [r#"[4,"x"]"#, "[-1]", "[5.5]", "[99999999999]"] {
        let odd = format!(r#"{{"pids":{pids},"harness":"claude","sessionId":"s1"}}"#);
        let refused = serde_json::from_str::<SessionEndBody>(&odd);
        assert!(refused.is_err(), "{odd}");
    }
}
