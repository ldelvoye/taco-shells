use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::value::RawValue;
use serde_json::{Map, Value};
use taco_agents::{Harness, harness_of};
use taco_protocol::{HookCall, HookMessage};

pub const FOCUS_PORTS: std::ops::RangeInclusive<u16> = 23456..=23460;

const UPDATE_CHECK_ROUTE: &str = "/update/check";
// /attention only marks a terminal as waiting, while /focus-tab switches to it.
const FOCUS_TAB_ROUTE: &str = "/focus-tab";
const ATTENTION_ROUTE: &str = "/attention";
const AGENT_START_ROUTE: &str = "/agent-session/start";
const AGENT_END_ROUTE: &str = "/agent-session/end";
const AGENT_STATE_ROUTE: &str = "/agent-session/state";
const AGENT_HOOK_ROUTE: &str = "/agent-session/hook";
const AGENT_ROUTE_PREFIX: &str = "/agent-session/";

const SMALL_BODY_CAP: u64 = 64 * 1024;
const AGENT_HOOK_BODY_CAP: u64 = 16 * 1024 * 1024;
const HEAD_CAP: usize = 16 * 1024;
const READ_CHUNK: usize = 64 * 1024;
const HEAD_END: &[u8] = b"\r\n\r\n";
// The hook scripts give up after a second, so a connection that is still sending by then is dead.
const READ_DEADLINE: Duration = Duration::from_secs(1);
const WAKE_TIMEOUT: Duration = Duration::from_millis(200);

type OnEvent = Box<dyn Fn(ListenerEvent) + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub enum ListenerEvent {
    Hook(HookMessage),
    UpdateCheck,
}

struct Head {
    method: String,
    route: String,
    content_length: u64,
    chunked: bool,
    has_origin: bool,
}

pub struct HookListener {
    port: Option<u16>,
    port_file: PathBuf,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HookListener {
    pub fn start(port_file: PathBuf, on_event: OnEvent) -> HookListener {
        let ports: Vec<u16> = FOCUS_PORTS.collect();
        HookListener::start_on(&ports, port_file, READ_DEADLINE, on_event)
    }

    pub(crate) fn start_on(
        ports: &[u16],
        port_file: PathBuf,
        read_deadline: Duration,
        on_event: OnEvent,
    ) -> HookListener {
        let stopping = Arc::new(AtomicBool::new(false));
        let unbound = HookListener {
            port: None,
            port_file,
            stopping: Arc::clone(&stopping),
            thread: None,
        };
        let Some(listener) = bind_first_free(ports) else {
            return unbound;
        };
        let Ok(address) = listener.local_addr() else {
            return unbound;
        };

        let port = address.port();
        let port_text = port.to_string();
        let _ = fs::write(&unbound.port_file, port_text);

        let serving = Arc::clone(&stopping);
        let thread = thread::spawn(move || serve(&listener, &serving, read_deadline, &*on_event));
        let mut bound = unbound;
        bound.port = Some(port);
        bound.thread = Some(thread);
        bound
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        let Some(port) = self.port.take() else {
            return;
        };

        self.stopping.store(true, Ordering::SeqCst);
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let _ = TcpStream::connect_timeout(&address, WAKE_TIMEOUT);
        let thread = self.thread.take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }

        // Only the instance that bound a port wrote the port file; with every port taken, another instance owns it.
        let _ = fs::remove_file(&self.port_file);
    }
}

impl Drop for HookListener {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn bind_first_free(ports: &[u16]) -> Option<TcpListener> {
    for port in ports {
        let bound = TcpListener::bind(("127.0.0.1", *port));
        match bound {
            Ok(listener) => return Some(listener),
            Err(error) if error.kind() == ErrorKind::AddrInUse => continue,
            Err(_) => return None,
        }
    }
    None
}

fn serve(
    listener: &TcpListener,
    stopping: &AtomicBool,
    read_deadline: Duration,
    on_event: &(dyn Fn(ListenerEvent) + Send + Sync),
) {
    // One request at a time, so the app hears a session's start before its first state.
    for incoming in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = incoming else {
            continue;
        };
        let deadline = Instant::now() + read_deadline;
        handle(stream, deadline, on_event);
    }
}

fn handle(
    mut stream: TcpStream,
    deadline: Instant,
    on_event: &(dyn Fn(ListenerEvent) + Send + Sync),
) {
    let Some((head_bytes, leftover)) = read_head(&mut stream, deadline) else {
        return;
    };
    let Some(head) = parse_head(&head_bytes) else {
        respond(&mut stream, 400);
        return;
    };

    if head.method != "POST" {
        respond(&mut stream, 404);
        return;
    }

    if head.route == UPDATE_CHECK_ROUTE {
        respond(&mut stream, 200);
        on_event(ListenerEvent::UpdateCheck);
        return;
    }

    let Some(cap) = body_cap(&head.route) else {
        respond(&mut stream, 404);
        return;
    };
    if head.chunked {
        respond(&mut stream, 400);
        return;
    }

    // A browser sends Origin on every cross-origin POST and curl never does: a web page must not reach an agent's log.
    let is_agent_route = head.route.starts_with(AGENT_ROUTE_PREFIX);
    if is_agent_route && head.has_origin {
        let drained = drain_body(&mut stream, leftover.len(), head.content_length, deadline);
        if drained {
            respond(&mut stream, 403);
        }
        return;
    }

    if head.content_length > cap {
        let drained = drain_body(&mut stream, leftover.len(), head.content_length, deadline);
        if drained {
            respond(&mut stream, 413);
        }
        return;
    }

    let Some(body) = read_body(&mut stream, leftover, head.content_length, deadline) else {
        return;
    };

    // Answered before any work on the body, so a hook that blocks the agent never waits on the app.
    respond(&mut stream, 200);

    let message = parse_message(&head.route, &body);
    if let Some(message) = message {
        on_event(ListenerEvent::Hook(message));
    }
}

fn read_some(stream: &mut TcpStream, buffer: &mut [u8], deadline: Instant) -> Option<usize> {
    let now = Instant::now();
    let left = deadline.saturating_duration_since(now);
    if left.is_zero() {
        return None;
    }
    stream.set_read_timeout(Some(left)).ok()?;
    let read = stream.read(buffer);
    match read {
        Ok(0) => None,
        Ok(count) => Some(count),
        Err(_) => None,
    }
}

fn find_head_end(bytes: &[u8]) -> Option<usize> {
    let mut windows = bytes.windows(HEAD_END.len());
    windows.position(|window| window == HEAD_END)
}

fn read_head(stream: &mut TcpStream, deadline: Instant) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut received: Vec<u8> = Vec::new();
    let mut chunk = vec![0; READ_CHUNK];
    loop {
        if let Some(end) = find_head_end(&received) {
            let leftover = received.split_off(end + HEAD_END.len());
            received.truncate(end);
            return Some((received, leftover));
        }
        if received.len() > HEAD_CAP {
            return None;
        }
        let count = read_some(stream, &mut chunk, deadline)?;
        let fresh = &chunk[..count];
        received.extend_from_slice(fresh);
    }
}

fn parse_head(bytes: &[u8]) -> Option<Head> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?;
    let route = parts.next()?;

    let mut content_length = 0;
    let mut chunked = false;
    let mut has_origin = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse().ok()?;
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            let lowered = value.to_ascii_lowercase();
            chunked = lowered.contains("chunked");
        } else if name.eq_ignore_ascii_case("origin") {
            has_origin = true;
        }
    }

    Some(Head {
        method: method.to_string(),
        route: route.to_string(),
        content_length,
        chunked,
        has_origin,
    })
}

fn read_body(
    stream: &mut TcpStream,
    leftover: Vec<u8>,
    length: u64,
    deadline: Instant,
) -> Option<Vec<u8>> {
    let length = usize::try_from(length).ok()?;
    let mut body = leftover;
    let mut chunk = vec![0; READ_CHUNK];
    while body.len() < length {
        let count = read_some(stream, &mut chunk, deadline)?;
        let fresh = &chunk[..count];
        body.extend_from_slice(fresh);
    }
    body.truncate(length);
    Some(body)
}

fn drain_body(stream: &mut TcpStream, already: usize, length: u64, deadline: Instant) -> bool {
    let mut received = already as u64;
    let mut chunk = vec![0; READ_CHUNK];
    while received < length {
        let Some(count) = read_some(stream, &mut chunk, deadline) else {
            return false;
        };
        received += count as u64;
    }
    true
}

fn respond(stream: &mut TcpStream, status: u16) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Payload Too Large",
    };
    let body = match status {
        200 => "ok",
        _ => "",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.set_write_timeout(Some(READ_DEADLINE));
    let _ = stream.write_all(response.as_bytes());
}

fn body_cap(route: &str) -> Option<u64> {
    match route {
        AGENT_HOOK_ROUTE => Some(AGENT_HOOK_BODY_CAP),
        FOCUS_TAB_ROUTE | ATTENTION_ROUTE | AGENT_START_ROUTE | AGENT_END_ROUTE
        | AGENT_STATE_ROUTE => Some(SMALL_BODY_CAP),
        _ => None,
    }
}

fn parse_message(route: &str, body: &[u8]) -> Option<HookMessage> {
    if route == AGENT_HOOK_ROUTE {
        return parse_hook_message(body);
    }

    let value: Value = serde_json::from_slice(body).ok()?;
    let object = value.as_object()?;
    let pids_value = object.get("pids")?;
    let pids = read_pids(pids_value)?;
    let call = call_for(route, object)?;
    Some(HookMessage { pids, call })
}

fn parse_hook_message(body: &[u8]) -> Option<HookMessage> {
    let fields: HashMap<String, Box<RawValue>> = serde_json::from_slice(body).ok()?;

    let pids_text = fields.get("pids")?;
    let pids_value: Value = serde_json::from_str(pids_text.get()).ok()?;
    let pids = read_pids(&pids_value)?;

    let harness_text = fields.get("harness")?;
    let harness_value: Value = serde_json::from_str(harness_text.get()).ok()?;
    let harness_name = harness_value.as_str()?;
    let harness = harness_of(harness_name)?;

    let payload_raw = fields.get("payload")?;
    let payload_text = payload_raw.get();
    let payload_value: Value = serde_json::from_str(payload_text).ok()?;
    let payload = payload_value.as_object()?;
    let session_id = read_session_id(harness, payload, "session_id")?;
    let event = read_text(payload, "hook_event_name")?;

    let call = HookCall::AgentHook {
        harness: harness.id(),
        session_id,
        event: event.to_string(),
        payload: payload_text.to_string(),
    };
    Some(HookMessage { pids, call })
}

fn read_pids(value: &Value) -> Option<Vec<u32>> {
    let items = value.as_array()?;

    let mut pids = Vec::new();
    for item in items {
        let Some(number) = item.as_u64() else {
            continue;
        };
        let Ok(pid) = u32::try_from(number) else {
            continue;
        };
        pids.push(pid);
    }

    if pids.is_empty() {
        return None;
    }
    Some(pids)
}

fn call_for(route: &str, object: &Map<String, Value>) -> Option<HookCall> {
    match route {
        FOCUS_TAB_ROUTE => Some(HookCall::FocusTab),
        ATTENTION_ROUTE => Some(HookCall::Attention),
        _ => agent_call(route, object),
    }
}

fn agent_call(route: &str, object: &Map<String, Value>) -> Option<HookCall> {
    let harness_name = read_text(object, "harness")?;
    let harness = harness_of(harness_name)?;
    match route {
        AGENT_START_ROUTE => start_call(harness, object),
        AGENT_END_ROUTE => end_call(harness, object),
        AGENT_STATE_ROUTE => state_call(harness, object),
        _ => None,
    }
}

fn start_call(harness: &Harness, object: &Map<String, Value>) -> Option<HookCall> {
    let session_id = read_session_id(harness, object, "sessionId")?;
    let cwd = read_text(object, "cwd")?;
    if cwd.is_empty() {
        return None;
    }

    let conversation = object.get("hasConversation");
    let has_conversation = conversation == Some(&Value::Bool(true));
    Some(HookCall::AgentStart {
        harness: harness.id(),
        session_id,
        cwd: cwd.to_string(),
        has_conversation,
    })
}

fn end_call(harness: &Harness, object: &Map<String, Value>) -> Option<HookCall> {
    let session_id = read_session_id(harness, object, "sessionId")?;
    Some(HookCall::AgentEnd {
        harness: harness.id(),
        session_id,
    })
}

fn state_call(harness: &Harness, object: &Map<String, Value>) -> Option<HookCall> {
    let session_id = read_session_id(harness, object, "sessionId")?;
    let event = read_text(object, "event")?;
    let detail = read_detail(object);
    let signal = harness.signal_for(event, detail)?;
    Some(HookCall::AgentState {
        harness: harness.id(),
        session_id,
        signal,
    })
}

fn read_session_id(harness: &Harness, object: &Map<String, Value>, key: &str) -> Option<String> {
    let session_id = read_text(object, key)?;
    if !harness.is_session_id(session_id) {
        return None;
    }
    Some(session_id.to_string())
}

fn read_text<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    let value = object.get(key)?;
    value.as_str()
}

fn read_detail(object: &Map<String, Value>) -> Option<&str> {
    let detail = read_text(object, "detail")?;
    if detail.is_empty() {
        return None;
    }
    Some(detail)
}

#[cfg(test)]
mod tests {
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Receiver};

    use taco_protocol::{AgentSignal, HarnessId};

    use super::*;

    const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";
    const WAIT: Duration = Duration::from_secs(2);
    const TEST_DEADLINE: Duration = Duration::from_millis(200);

    fn post(port: u16, method: &str, path: &str, body: &[u8]) -> u16 {
        post_with(port, method, path, "", body)
    }

    fn post_with(port: u16, method: &str, path: &str, extra_headers: &str, body: &[u8]) -> u16 {
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
        status.parse().unwrap()
    }

    fn start_collecting() -> (HookListener, u16, Receiver<ListenerEvent>) {
        let (sender, receiver) = mpsc::channel();
        let listener = HookListener::start_on(
            &[0],
            PathBuf::from("/nonexistent/focus-port"),
            TEST_DEADLINE,
            Box::new(move |event| {
                let _ = sender.send(event);
            }),
        );
        let port = listener.port().unwrap();
        (listener, port, receiver)
    }

    fn hook(pids: Vec<u32>, call: HookCall) -> ListenerEvent {
        ListenerEvent::Hook(HookMessage { pids, call })
    }

    fn claude() -> HarnessId {
        HarnessId("claude".to_string())
    }

    fn next_event(receiver: &Receiver<ListenerEvent>) -> ListenerEvent {
        receiver.recv_timeout(WAIT).unwrap()
    }

    fn expect_dropped(port: u16, receiver: &Receiver<ListenerEvent>, path: &str, body: &str) {
        let status = post(port, "POST", path, body.as_bytes());
        assert_eq!(status, 200, "{path} {body}");

        let marker = post(port, "POST", ATTENTION_ROUTE, br#"{"pids":[9]}"#);
        assert_eq!(marker, 200);
        let next = next_event(receiver);
        assert_eq!(next, hook(vec![9], HookCall::Attention), "{path} {body}");
    }

    #[test]
    fn routes_accept_what_the_scripts_send_and_drop_the_rest() {
        let (listener, port, receiver) = start_collecting();

        let status = post(port, "POST", UPDATE_CHECK_ROUTE, b"");
        assert_eq!(status, 200);
        assert_eq!(next_event(&receiver), ListenerEvent::UpdateCheck);

        post(port, "POST", ATTENTION_ROUTE, br#"{"pids":[4,5]}"#);
        let attention = next_event(&receiver);
        assert_eq!(attention, hook(vec![4, 5], HookCall::Attention));
        post(
            port,
            "POST",
            FOCUS_TAB_ROUTE,
            br#"{"pids":[4,"x",-1,5.5,99999999999]}"#,
        );
        let focus = next_event(&receiver);
        assert_eq!(focus, hook(vec![4], HookCall::FocusTab));

        let start = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":"/tmp/work","hasConversation":true}}"#
        );
        post(port, "POST", AGENT_START_ROUTE, start.as_bytes());
        let expected = HookCall::AgentStart {
            harness: claude(),
            session_id: SESSION.to_string(),
            cwd: "/tmp/work".to_string(),
            has_conversation: true,
        };
        let started = next_event(&receiver);
        assert_eq!(started, hook(vec![1], expected));

        let fresh_start = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":"/tmp/new"}}"#
        );
        post(port, "POST", AGENT_START_ROUTE, fresh_start.as_bytes());
        let fresh = HookCall::AgentStart {
            harness: claude(),
            session_id: SESSION.to_string(),
            cwd: "/tmp/new".to_string(),
            has_conversation: false,
        };
        let started_fresh = next_event(&receiver);
        assert_eq!(started_fresh, hook(vec![1], fresh));

        let no_cwd =
            format!(r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":""}}"#);
        expect_dropped(port, &receiver, AGENT_START_ROUTE, &no_cwd);
        let bad_session = r#"{"pids":[1],"harness":"claude","sessionId":"nope","cwd":"/tmp"}"#;
        expect_dropped(port, &receiver, AGENT_START_ROUTE, bad_session);
        let unknown_harness =
            format!(r#"{{"pids":[1],"harness":"gemini","sessionId":"{SESSION}","cwd":"/tmp"}}"#);
        expect_dropped(port, &receiver, AGENT_START_ROUTE, &unknown_harness);

        let idle = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","event":"Notification","detail":"idle_prompt"}}"#
        );
        expect_dropped(port, &receiver, AGENT_STATE_ROUTE, &idle);
        let asking = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","event":"PreToolUse","detail":"AskUserQuestion"}}"#
        );
        post(port, "POST", AGENT_STATE_ROUTE, asking.as_bytes());
        let needs = HookCall::AgentState {
            harness: claude(),
            session_id: SESSION.to_string(),
            signal: AgentSignal::Needs,
        };
        let state = next_event(&receiver);
        assert_eq!(state, hook(vec![1], needs));

        expect_dropped(port, &receiver, ATTENTION_ROUTE, r#"{"pids":[]}"#);

        assert_eq!(post(port, "GET", ATTENTION_ROUTE, b""), 404);
        assert_eq!(post(port, "POST", "/nope", b"{}"), 404);
        let at_cap = vec![b' '; 64 * 1024];
        assert_eq!(post(port, "POST", ATTENTION_ROUTE, &at_cap), 200);
        let over_cap = vec![b' '; 64 * 1024 + 1];
        assert_eq!(post(port, "POST", ATTENTION_ROUTE, &over_cap), 413);

        let payload = format!(
            r#"{{ "session_id": "{SESSION}", "hook_event_name": "PreToolUse", "tool_input": {{"a": [1, 2.50]}} }}"#
        );
        let hook_body = format!(r#"{{"pids":[1],"harness":"claude","payload":{payload}}}"#);
        post(port, "POST", AGENT_HOOK_ROUTE, hook_body.as_bytes());
        let received = next_event(&receiver);
        let ListenerEvent::Hook(HookMessage {
            call:
                HookCall::AgentHook {
                    event,
                    payload: text,
                    ..
                },
            ..
        }) = received
        else {
            panic!("not an agent hook: {received:?}");
        };
        assert_eq!(event, "PreToolUse");
        assert_eq!(text, payload);

        let string_payload = r#"{"pids":[1],"harness":"claude","payload":"text"}"#;
        expect_dropped(port, &receiver, AGENT_HOOK_ROUTE, string_payload);

        let from_a_page = "Origin: https://example.com\r\n";
        let refused = post_with(
            port,
            "POST",
            AGENT_HOOK_ROUTE,
            from_a_page,
            hook_body.as_bytes(),
        );
        assert_eq!(refused, 403);
        post(port, "POST", ATTENTION_ROUTE, br#"{"pids":[8]}"#);
        let after_page = next_event(&receiver);
        assert_eq!(after_page, hook(vec![8], HookCall::Attention));

        let mut stalled = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let stalled_head =
            format!("POST {AGENT_HOOK_ROUTE} HTTP/1.1\r\nContent-Length: 5000\r\n\r\n{{");
        stalled.write_all(stalled_head.as_bytes()).unwrap();
        post(port, "POST", ATTENTION_ROUTE, br#"{"pids":[7]}"#);
        let after_stall = next_event(&receiver);
        assert_eq!(after_stall, hook(vec![7], HookCall::Attention));
        drop(stalled);

        listener.stop();
    }

    #[test]
    fn the_answer_comes_before_the_app_hears_of_it() {
        let (sender, events) = mpsc::channel();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let blocked = HookListener::start_on(
            &[0],
            PathBuf::from("/nonexistent/focus-port"),
            TEST_DEADLINE,
            Box::new(move |event| {
                let _ = sender.send(event);
                let waiting = gate.lock().unwrap();
                let _ = waiting.recv();
            }),
        );
        let port = blocked.port().unwrap();
        let payload = format!(r#"{{"session_id":"{SESSION}","hook_event_name":"SessionStart"}}"#);
        let body = format!(r#"{{"pids":[1],"harness":"claude","payload":{payload}}}"#);

        let status = post(port, "POST", AGENT_HOOK_ROUTE, body.as_bytes());

        assert_eq!(status, 200);
        let heard = events.recv_timeout(WAIT).unwrap();
        assert!(matches!(heard, ListenerEvent::Hook(_)));
        release.send(()).unwrap();
        blocked.stop();
    }

    #[test]
    fn takes_the_next_free_port_and_leaves_a_port_file_it_does_not_own() {
        let directory = tempfile::tempdir().unwrap();
        let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = blocker.local_addr().unwrap().port();
        let spare = TcpListener::bind("127.0.0.1:0").unwrap();
        let free = spare.local_addr().unwrap().port();
        drop(spare);

        let file = directory.path().join("focus-port");
        let listener = HookListener::start_on(
            &[taken, free],
            file.clone(),
            TEST_DEADLINE,
            Box::new(|_| {}),
        );
        assert_eq!(listener.port(), Some(free));
        assert_eq!(fs::read_to_string(&file).unwrap(), free.to_string());
        listener.stop();
        assert!(!file.exists());

        let other_file = directory.path().join("other-port");
        fs::write(&other_file, "999").unwrap();
        let unbound = HookListener::start_on(
            &[taken],
            other_file.clone(),
            TEST_DEADLINE,
            Box::new(|_| {}),
        );
        assert_eq!(unbound.port(), None);
        unbound.stop();
        assert_eq!(fs::read_to_string(&other_file).unwrap(), "999");
    }

    fn spawn_script(script: &str, directory: &std::path::Path, stdin_json: &str) -> Child {
        let path = format!("{}/../../hooks/{script}", env!("CARGO_MANIFEST_DIR"));
        let mut child = Command::new("/bin/sh")
            .arg(path)
            .env_clear()
            .env("TACO_SHELLS_CONFIG_DIR", directory)
            .env("PATH", "/bin:/usr/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(stdin_json.as_bytes()).unwrap();
        drop(stdin);
        child
    }

    #[test]
    fn the_shipped_scripts_are_accepted() {
        let directory = tempfile::tempdir().unwrap();
        let (sender, receiver) = mpsc::channel();
        let listener = HookListener::start_on(
            &[0],
            directory.path().join("focus-port"),
            READ_DEADLINE,
            Box::new(move |event| {
                let _ = sender.send(event);
            }),
        );

        let session_script = "taco-shells-claude-session.sh";
        let session_events = [
            r#""hook_event_name":"SessionStart","cwd":"/tmp/work","source":"resume""#,
            r#""hook_event_name":"UserPromptSubmit","cwd":"/tmp/work""#,
            r#""hook_event_name":"Notification","notification_type":"permission_prompt""#,
            r#""hook_event_name":"SessionEnd","cwd":"/tmp/work""#,
        ];
        let mut children = Vec::new();
        for fields in session_events {
            let json = format!(r#"{{"session_id":"{SESSION}",{fields}}}"#);
            let child = spawn_script(session_script, directory.path(), &json);
            children.push(child);
        }
        let attention = spawn_script("taco-shells-attention.sh", directory.path(), "{}");
        children.push(attention);
        for mut child in children {
            let status = child.wait().unwrap();
            assert!(status.success());
        }

        let session_posts = session_events.len() * 2;
        let expected_events = session_posts + 1;
        let mut calls = Vec::new();
        let mut hook_events = Vec::new();
        for _ in 0..expected_events {
            let event = next_event(&receiver);
            let ListenerEvent::Hook(message) = event else {
                panic!("not a hook: {event:?}");
            };
            assert!(message.pids.contains(&std::process::id()));
            match message.call {
                HookCall::AgentHook { event, .. } => hook_events.push(event),
                call => calls.push(call),
            }
        }
        listener.stop();

        let state = |signal| HookCall::AgentState {
            harness: claude(),
            session_id: SESSION.to_string(),
            signal,
        };
        let expected = vec![
            HookCall::AgentStart {
                harness: claude(),
                session_id: SESSION.to_string(),
                cwd: "/tmp/work".to_string(),
                has_conversation: true,
            },
            state(AgentSignal::Working),
            state(AgentSignal::Needs),
            HookCall::AgentEnd {
                harness: claude(),
                session_id: SESSION.to_string(),
            },
            HookCall::Attention,
        ];
        assert_eq!(calls.len(), expected.len(), "{calls:?}");
        for call in &expected {
            assert!(calls.contains(call), "missing {call:?} in {calls:?}");
        }
        hook_events.sort();
        let names = [
            "Notification",
            "SessionEnd",
            "SessionStart",
            "UserPromptSubmit",
        ];
        assert_eq!(hook_events, names);
    }
}
