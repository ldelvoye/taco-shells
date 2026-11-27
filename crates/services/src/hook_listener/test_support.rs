use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use taco_protocol::routes::{AGENT_HOOK, SESSION_END, SESSION_START, SESSION_STATE};
use taco_protocol::{
    HarnessId, HookCall, HookMessage, PORT_FILE_NAME, TOKEN_FILE_NAME, TOKEN_HEADER, deadlines,
};

use super::{AnswerPart, HookListener, ListenerEvent};

pub(super) const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";
pub(super) const WAIT: Duration = Duration::from_secs(2);
pub(super) const TEST_DEADLINE: Duration = Duration::from_millis(200);
pub(super) const QUESTION_WINDOW: Duration = Duration::from_millis(300);

pub(super) struct Questioned {
    pub(super) _listener: HookListener,
    pub(super) port: u16,
    pub(super) token_header: String,
    pub(super) _directory: tempfile::TempDir,
}

pub(super) struct Collecting {
    pub(super) listener: HookListener,
    pub(super) port: u16,
    pub(super) receiver: Receiver<ListenerEvent>,
    pub(super) token_header: String,
    pub(super) _directory: tempfile::TempDir,
}

pub(super) fn post(port: u16, method: &str, path: &str, body: &[u8]) -> u16 {
    post_with(port, method, path, "", body)
}

pub(super) fn post_with(
    port: u16,
    method: &str,
    path: &str,
    extra_headers: &str,
    body: &[u8],
) -> u16 {
    let (status, _) = post_for_reply(port, method, path, extra_headers, body);
    status
}

pub(super) fn post_for_reply(
    port: u16,
    method: &str,
    path: &str,
    extra_headers: &str,
    body: &[u8],
) -> (u16, String) {
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);

    let mut reply = Vec::new();
    let _ = stream.read_to_end(&mut reply);
    let text = String::from_utf8_lossy(&reply);
    let status = text.split_whitespace().nth(1).unwrap();
    let reply_body = text.split("\r\n\r\n").nth(1).unwrap_or_default();
    (status.parse().unwrap(), reply_body.to_string())
}

pub(super) fn start_collecting() -> Collecting {
    let directory = tempfile::tempdir().unwrap();
    let (sender, receiver) = mpsc::channel();
    let listener = HookListener::start_on(
        &[0],
        directory.path().join(PORT_FILE_NAME),
        TEST_DEADLINE,
        deadlines::ANSWER_WINDOW,
        Box::new(move |event| {
            let _ = sender.send(event);
        }),
    );
    let port = listener.port().unwrap();
    let token = fs::read_to_string(directory.path().join(TOKEN_FILE_NAME)).unwrap();
    Collecting {
        listener,
        port,
        receiver,
        token_header: format!("{TOKEN_HEADER}: {token}\r\n"),
        _directory: directory,
    }
}

pub(super) fn hook(pids: Vec<u32>, call: HookCall) -> ListenerEvent {
    ListenerEvent::Hook(HookMessage { pids, call })
}

pub(super) fn claude() -> HarnessId {
    HarnessId("claude".to_string())
}

pub(super) fn end_body(pids: &str) -> String {
    format!(r#"{{"pids":{pids},"harness":"claude","sessionId":"{SESSION}"}}"#)
}

pub(super) fn ended(pids: Vec<u32>) -> ListenerEvent {
    let call = HookCall::AgentEnd {
        harness: claude(),
        session_id: SESSION.to_string(),
    };
    hook(pids, call)
}

pub(super) fn next_event(receiver: &Receiver<ListenerEvent>) -> ListenerEvent {
    receiver.recv_timeout(WAIT).unwrap()
}

pub(super) fn expect_dropped(collecting: &Collecting, path: &str, body: &str) {
    let port = collecting.port;
    let headers = &collecting.token_header;
    let status = post_with(port, "POST", path, headers, body.as_bytes());
    assert_eq!(status, 200, "{path} {body}");

    let marker = end_body("[9]");
    let marked = post_with(port, "POST", SESSION_END, headers, marker.as_bytes());
    assert_eq!(marked, 200);
    let next = next_event(&collecting.receiver);
    assert_eq!(next, ended(vec![9]), "{path} {body}");
}

pub(super) fn activity_post(tool_name: &str) -> String {
    let payload = serde_json::json!({
        "session_id": SESSION,
        "hook_event_name": "PostToolUse",
        "tool_name": tool_name,
        "tool_input": {"text": "did a thing"},
    });
    let body = serde_json::json!({"pids": [1], "harness": "claude", "payload": payload});
    body.to_string()
}

pub(super) fn every_hook_route_body() -> Vec<(&'static str, String)> {
    let start =
        format!(r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":"/tmp/work"}}"#);
    let end = format!(r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}"}}"#);
    let state = format!(
        r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","event":"UserPromptSubmit"}}"#
    );
    let payload = format!(r#"{{"session_id":"{SESSION}","hook_event_name":"SessionStart"}}"#);
    let hook_body = format!(r#"{{"pids":[1],"harness":"claude","payload":{payload}}}"#);
    vec![
        (SESSION_START, start),
        (SESSION_END, end),
        (SESSION_STATE, state),
        (AGENT_HOOK, hook_body),
    ]
}

pub(super) fn start_questioned(
    on_event: impl Fn(ListenerEvent) + Send + Sync + 'static,
) -> Questioned {
    let directory = tempfile::tempdir().unwrap();
    let listener = HookListener::start_on(
        &[0],
        directory.path().join(PORT_FILE_NAME),
        TEST_DEADLINE,
        QUESTION_WINDOW,
        Box::new(on_event),
    );
    let port = listener.port().unwrap();
    let token = fs::read_to_string(directory.path().join(TOKEN_FILE_NAME)).unwrap();
    Questioned {
        _listener: listener,
        port,
        token_header: format!("{TOKEN_HEADER}: {token}\r\n"),
        _directory: directory,
    }
}

pub(super) fn question_body() -> String {
    let payload = serde_json::json!({
        "session_id": SESSION,
        "hook_event_name": "SessionStart",
        "source": "resume",
    });
    let body = serde_json::json!({"pids": [1], "harness": "claude", "payload": payload});
    body.to_string()
}

pub(super) fn ask(questioned: &Questioned, route: &str) -> (String, Duration) {
    let started = Instant::now();
    let body = question_body();
    let headers = &questioned.token_header;
    let (status, reply) = post_for_reply(questioned.port, "POST", route, headers, body.as_bytes());
    assert_eq!(status, 200);
    (reply, started.elapsed())
}

pub(super) fn answering(
    parts: fn() -> Vec<AnswerPart>,
) -> impl Fn(ListenerEvent) + Send + Sync + 'static {
    let kept = Mutex::new(Vec::new());
    move |event| {
        let ListenerEvent::Question(question) = event else {
            return;
        };
        for part in parts() {
            let _ = question.reply.send(part);
        }
        kept.lock().unwrap().push(question.reply);
    }
}

pub(super) fn wait_until(flag: &AtomicBool) -> bool {
    let give_up = Instant::now() + WAIT;
    while Instant::now() < give_up {
        if flag.load(Ordering::SeqCst) {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    false
}
