use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::value::RawValue;
use serde_json::{Map, Value};
use taco_agents::{Harness, harness_of};
use taco_plugin::{SERVER_NAME, TOOL_NAMES};
use taco_protocol::routes::{AGENT_HOOK, CONTEXT, SESSION_END, SESSION_START, SESSION_STATE, STOP};
use taco_protocol::{
    AgentHookBody, ContextAnswer, HookCall, HookMessage, SessionEndBody, SessionStartBody,
    SessionStateBody, StopAnswer, TOKEN_HEADER, deadlines,
};

use taco_protocol::TOKEN_FILE_NAME;

pub const FOCUS_PORTS: std::ops::RangeInclusive<u16> = 23456..=23460;

const UPDATE_CHECK_ROUTE: &str = "/update/check";
const NO_ANSWER: &str = "{}";

const SMALL_BODY_CAP: u64 = 64 * 1024;
const AGENT_HOOK_BODY_CAP: u64 = 16 * 1024 * 1024;
const HEAD_CAP: usize = 16 * 1024;
const READ_CHUNK: usize = 64 * 1024;
const HEAD_END: &[u8] = b"\r\n\r\n";
const PLUGIN_TOOL_PREFIX: &str = "mcp__plugin_";
// The plugin's hook gives up after 250 ms, so a connection still sending after a second is dead.
const READ_DEADLINE: Duration = Duration::from_secs(1);
const WAKE_TIMEOUT: Duration = Duration::from_millis(200);

type OnEvent = Box<dyn Fn(ListenerEvent) + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub enum ListenerEvent {
    Hook(HookMessage),
    Question(Question),
    UpdateCheck,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QuestionKind {
    Context { source: String },
    Stop,
}

pub enum AnswerPart {
    Placed(bool),
    Context(Option<String>),
    Nudge(Option<String>, Option<OnDelivered>),
}

pub type OnDelivered = Box<dyn FnOnce() + Send>;

#[derive(Clone, Debug)]
pub struct Question {
    pub kind: QuestionKind,
    pub message: HookMessage,
    pub reply: Sender<AnswerPart>,
}

impl PartialEq for Question {
    fn eq(&self, other: &Question) -> bool {
        self.kind == other.kind && self.message == other.message
    }
}

struct Answer {
    body: String,
    on_delivered: Option<OnDelivered>,
}

struct ParsedHook {
    message: HookMessage,
    source: String,
}

struct Head {
    method: String,
    route: String,
    content_length: u64,
    chunked: bool,
    has_origin: bool,
    token: Option<String>,
}

pub struct HookListener {
    port: Option<u16>,
    port_file: PathBuf,
    token_file: PathBuf,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl HookListener {
    pub fn start(port_file: PathBuf, on_event: OnEvent) -> HookListener {
        let ports: Vec<u16> = FOCUS_PORTS.collect();
        let answer_window = deadlines::ANSWER_WINDOW;
        HookListener::start_on(&ports, port_file, READ_DEADLINE, answer_window, on_event)
    }

    pub(crate) fn start_on(
        ports: &[u16],
        port_file: PathBuf,
        read_deadline: Duration,
        answer_window: Duration,
        on_event: OnEvent,
    ) -> HookListener {
        let stopping = Arc::new(AtomicBool::new(false));
        let token_file = port_file.with_file_name(TOKEN_FILE_NAME);
        let unbound = HookListener {
            port: None,
            port_file,
            token_file,
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
        let identifier = uuid::Uuid::new_v4();
        let token = identifier.simple().to_string();
        write_token_file(&unbound.token_file, &token);
        let port_text = port.to_string();
        let _ = fs::write(&unbound.port_file, port_text);

        let serving = Arc::clone(&stopping);
        let thread = thread::spawn(move || {
            serve(
                &listener,
                &serving,
                read_deadline,
                answer_window,
                &token,
                &*on_event,
            );
        });
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
        let _ = fs::remove_file(&self.token_file);
    }
}

impl Drop for HookListener {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn write_token_file(path: &Path, token: &str) {
    let temporary_name = format!(".{TOKEN_FILE_NAME}.{}.tmp", std::process::id());
    let temporary = path.with_file_name(temporary_name);
    let _ = fs::remove_file(&temporary);
    let opened = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary);
    let written = match opened {
        Ok(mut file) => file.write_all(token.as_bytes()),
        Err(problem) => Err(problem),
    };
    let replaced = match written {
        Ok(()) => fs::rename(&temporary, path),
        Err(problem) => Err(problem),
    };
    if replaced.is_err() {
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_file(path);
    }
}

fn is_any_channels_activity_call(tool_name: &str) -> bool {
    if !tool_name.starts_with(PLUGIN_TOOL_PREFIX) {
        return false;
    }
    TOOL_NAMES.iter().any(|name| {
        let suffix = format!("_{SERVER_NAME}__{name}");
        tool_name.ends_with(&suffix)
    })
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
    answer_window: Duration,
    token: &str,
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
        handle(stream, deadline, answer_window, token, on_event);
    }
}

fn handle(
    mut stream: TcpStream,
    deadline: Instant,
    answer_window: Duration,
    token: &str,
    on_event: &(dyn Fn(ListenerEvent) + Send + Sync),
) {
    let Some((head_bytes, leftover)) = read_head(&mut stream, deadline) else {
        return;
    };
    let answer_by = Instant::now() + answer_window;
    let Some(head) = parse_head(&head_bytes) else {
        respond(&mut stream, 400);
        return;
    };

    if head.method != "POST" {
        respond(&mut stream, 404);
        return;
    }

    // A browser sends Origin on every cross-origin POST and curl never does, and only our scripts and promote call this port.
    if head.has_origin {
        let drained = drain_body(&mut stream, leftover.len(), head.content_length, deadline);
        if drained {
            respond(&mut stream, 403);
        }
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

    let sent_token = head.token.as_deref();
    let is_question = head.route == CONTEXT || head.route == STOP;
    if is_question {
        ask_question(
            stream,
            &head.route,
            sent_token,
            &body,
            answer_by,
            token,
            on_event,
        );
        return;
    }

    // Answered before any work on the body, so a hook that blocks the agent never waits on the app.
    respond(&mut stream, 200);

    // Every post comes from the plugin and carries the token; one without it is another process.
    if sent_token != Some(token) {
        return;
    }
    let message = parse_message(&head.route, &body, sent_token, token);
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
    let mut token = None;
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
        } else if name.eq_ignore_ascii_case(TOKEN_HEADER) {
            token = Some(value.to_string());
        }
    }

    Some(Head {
        method: method.to_string(),
        route: route.to_string(),
        content_length,
        chunked,
        has_origin,
        token,
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

fn respond_with_body(stream: &mut TcpStream, body: &str) -> bool {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.set_write_timeout(Some(READ_DEADLINE));
    let written = stream.write_all(response.as_bytes());
    written.is_ok()
}

fn ask_question(
    mut stream: TcpStream,
    route: &str,
    sent_token: Option<&str>,
    body: &[u8],
    answer_by: Instant,
    token: &str,
    on_event: &(dyn Fn(ListenerEvent) + Send + Sync),
) {
    if sent_token != Some(token) {
        respond_with_body(&mut stream, NO_ANSWER);
        return;
    }
    let Some(parsed) = parse_hook_message(body, sent_token, token) else {
        respond_with_body(&mut stream, NO_ANSWER);
        return;
    };

    let kind = question_kind(route, parsed.source);
    let (reply, replies) = mpsc::channel();
    let question = Question {
        kind: kind.clone(),
        message: parsed.message,
        reply,
    };
    on_event(ListenerEvent::Question(question));

    // Answered off the serial loop, so a question waiting on the workspace holds up no other post.
    let spare = stream.try_clone();
    let builder = thread::Builder::new().name("hook-question".into());
    let spawned = builder.spawn(move || answer_question(stream, &kind, &replies, answer_by));
    if spawned.is_err()
        && let Ok(mut spare) = spare
    {
        respond_with_body(&mut spare, NO_ANSWER);
    }
}

fn question_kind(route: &str, source: String) -> QuestionKind {
    if route == STOP {
        return QuestionKind::Stop;
    }
    QuestionKind::Context { source }
}

fn answer_question(
    mut stream: TcpStream,
    kind: &QuestionKind,
    replies: &Receiver<AnswerPart>,
    answer_by: Instant,
) {
    let mut placed: Option<bool> = None;
    let mut content: Option<Answer> = None;
    loop {
        if placed == Some(false) {
            break;
        }
        if placed == Some(true) && content.is_some() {
            break;
        }
        let left = answer_by.saturating_duration_since(Instant::now());
        let Ok(part) = replies.recv_timeout(left) else {
            break;
        };
        if let AnswerPart::Placed(is_placed) = part {
            placed = Some(is_placed);
            continue;
        }
        let held = complete_answer(kind, part);
        if held.is_some() {
            content = held;
        }
    }

    let answer = final_answer(kind, placed, content);
    let written = respond_with_body(&mut stream, &answer.body);
    if written && let Some(on_delivered) = answer.on_delivered {
        on_delivered();
    }
}

fn context_body(context: Option<String>) -> String {
    let answer = ContextAnswer {
        yours: true,
        context,
    };
    match serde_json::to_string(&answer) {
        Ok(text) => text,
        Err(_) => String::from(NO_ANSWER),
    }
}

fn stop_body(reason: Option<String>) -> String {
    let answer = StopAnswer { reason };
    match serde_json::to_string(&answer) {
        Ok(text) => text,
        Err(_) => String::from(NO_ANSWER),
    }
}

fn complete_answer(kind: &QuestionKind, part: AnswerPart) -> Option<Answer> {
    match (kind, part) {
        (QuestionKind::Context { .. }, AnswerPart::Context(context)) => Some(Answer {
            body: context_body(context),
            on_delivered: None,
        }),
        (QuestionKind::Stop, AnswerPart::Nudge(Some(reason), on_delivered)) => Some(Answer {
            body: stop_body(Some(reason)),
            on_delivered,
        }),
        (QuestionKind::Stop, AnswerPart::Nudge(None, _)) => Some(Answer {
            body: stop_body(None),
            on_delivered: None,
        }),
        _ => None,
    }
}

fn final_answer(kind: &QuestionKind, placed: Option<bool>, content: Option<Answer>) -> Answer {
    let nothing = Answer {
        body: String::from(NO_ANSWER),
        on_delivered: None,
    };
    if placed != Some(true) {
        return nothing;
    }
    if let Some(answer) = content {
        return answer;
    }
    if matches!(kind, QuestionKind::Context { .. }) {
        return Answer {
            body: context_body(None),
            on_delivered: None,
        };
    }
    nothing
}

fn body_cap(route: &str) -> Option<u64> {
    match route {
        AGENT_HOOK | CONTEXT | STOP => Some(AGENT_HOOK_BODY_CAP),
        SESSION_START | SESSION_END | SESSION_STATE => Some(SMALL_BODY_CAP),
        _ => None,
    }
}

fn parse_message(
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

fn parse_hook_message(body: &[u8], sent_token: Option<&str>, token: &str) -> Option<ParsedHook> {
    let parsed: AgentHookBody<Box<RawValue>> = serde_json::from_slice(body).ok()?;
    let harness = harness_of(&parsed.harness.0)?;
    let pids = non_empty(parsed.pids)?;

    let payload_text = parsed.payload.get();
    let payload_value: Value = serde_json::from_str(payload_text).ok()?;
    let payload = payload_value.as_object()?;

    let tool_name = text_field(payload, "tool_name");
    let is_activity_call = tool_name.is_some_and(is_any_channels_activity_call);
    // Any local process can post here, so an activity call counts only with the token the app wrote for its own hook.
    if is_activity_call && sent_token != Some(token) {
        return None;
    }

    let session_text = text_field(payload, "session_id")?;
    let session_id = checked_session_id(harness, String::from(session_text))?;
    let event = text_field(payload, "hook_event_name")?;
    let mut source = String::new();
    if let Some(text) = text_field(payload, "source") {
        source = String::from(text);
    }

    let call = HookCall::AgentHook {
        harness: harness.id(),
        session_id,
        event: event.to_string(),
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

fn text_field<'a>(payload: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    let value = payload.get(key)?;
    value.as_str()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc::{self, Receiver};

    use taco_protocol::{AgentSignal, HarnessId, PORT_FILE_NAME};

    use super::*;

    const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";
    const WAIT: Duration = Duration::from_secs(2);
    const TEST_DEADLINE: Duration = Duration::from_millis(200);
    const QUESTION_WINDOW: Duration = Duration::from_millis(300);

    struct Questioned {
        _listener: HookListener,
        port: u16,
        token_header: String,
        _directory: tempfile::TempDir,
    }

    struct Collecting {
        listener: HookListener,
        port: u16,
        receiver: Receiver<ListenerEvent>,
        token_header: String,
        _directory: tempfile::TempDir,
    }

    fn post(port: u16, method: &str, path: &str, body: &[u8]) -> u16 {
        post_with(port, method, path, "", body)
    }

    fn post_with(port: u16, method: &str, path: &str, extra_headers: &str, body: &[u8]) -> u16 {
        let (status, _) = post_for_reply(port, method, path, extra_headers, body);
        status
    }

    fn post_for_reply(
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

    fn start_collecting() -> Collecting {
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

    fn hook(pids: Vec<u32>, call: HookCall) -> ListenerEvent {
        ListenerEvent::Hook(HookMessage { pids, call })
    }

    fn claude() -> HarnessId {
        HarnessId("claude".to_string())
    }

    fn end_body(pids: &str) -> String {
        format!(r#"{{"pids":{pids},"harness":"claude","sessionId":"{SESSION}"}}"#)
    }

    fn ended(pids: Vec<u32>) -> ListenerEvent {
        let call = HookCall::AgentEnd {
            harness: claude(),
            session_id: SESSION.to_string(),
        };
        hook(pids, call)
    }

    fn next_event(receiver: &Receiver<ListenerEvent>) -> ListenerEvent {
        receiver.recv_timeout(WAIT).unwrap()
    }

    fn expect_dropped(collecting: &Collecting, path: &str, body: &str) {
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

    #[test]
    fn routes_accept_what_the_scripts_send_and_drop_the_rest() {
        let collecting = start_collecting();
        let port = collecting.port;
        let receiver = &collecting.receiver;
        let token_header = &collecting.token_header;
        let post = |port: u16, method: &str, path: &str, body: &[u8]| {
            post_with(port, method, path, token_header, body)
        };

        let status = post(port, "POST", UPDATE_CHECK_ROUTE, b"");
        assert_eq!(status, 200);
        assert_eq!(next_event(receiver), ListenerEvent::UpdateCheck);

        let odd_pids = end_body(r#"[4,"x",-1,5.5,99999999999,5]"#);
        expect_dropped(&collecting, SESSION_END, &odd_pids);
        assert_eq!(post(port, "POST", "/focus-tab", br#"{"pids":[4]}"#), 404);

        let start = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":"/tmp/work","hasConversation":true}}"#
        );
        post(port, "POST", SESSION_START, start.as_bytes());
        let expected = HookCall::AgentStart {
            harness: claude(),
            session_id: SESSION.to_string(),
            cwd: "/tmp/work".to_string(),
            has_conversation: true,
        };
        let started = next_event(receiver);
        assert_eq!(started, hook(vec![1], expected));

        let fresh_start = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":"/tmp/new"}}"#
        );
        post(port, "POST", SESSION_START, fresh_start.as_bytes());
        let fresh = HookCall::AgentStart {
            harness: claude(),
            session_id: SESSION.to_string(),
            cwd: "/tmp/new".to_string(),
            has_conversation: false,
        };
        let started_fresh = next_event(receiver);
        assert_eq!(started_fresh, hook(vec![1], fresh));

        let no_cwd =
            format!(r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":""}}"#);
        expect_dropped(&collecting, SESSION_START, &no_cwd);
        let bad_session = r#"{"pids":[1],"harness":"claude","sessionId":"nope","cwd":"/tmp"}"#;
        expect_dropped(&collecting, SESSION_START, bad_session);
        let unknown_harness =
            format!(r#"{{"pids":[1],"harness":"gemini","sessionId":"{SESSION}","cwd":"/tmp"}}"#);
        expect_dropped(&collecting, SESSION_START, &unknown_harness);

        let idle = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","event":"Notification","detail":"idle_prompt"}}"#
        );
        expect_dropped(&collecting, SESSION_STATE, &idle);
        let asking = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","event":"PreToolUse","detail":"AskUserQuestion"}}"#
        );
        post(port, "POST", SESSION_STATE, asking.as_bytes());
        let needs = HookCall::AgentState {
            harness: claude(),
            session_id: SESSION.to_string(),
            signal: AgentSignal::Needs,
        };
        let state = next_event(receiver);
        assert_eq!(state, hook(vec![1], needs));

        expect_dropped(&collecting, SESSION_END, &end_body("[]"));

        assert_eq!(post(port, "GET", SESSION_END, b""), 404);
        assert_eq!(post(port, "POST", "/nope", b"{}"), 404);
        let at_cap = vec![b' '; 64 * 1024];
        assert_eq!(post(port, "POST", SESSION_END, &at_cap), 200);
        let over_cap = vec![b' '; 64 * 1024 + 1];
        assert_eq!(post(port, "POST", SESSION_END, &over_cap), 413);

        let payload = format!(
            r#"{{ "session_id": "{SESSION}", "hook_event_name": "PreToolUse", "tool_input": {{"a": [1, 2.50]}} }}"#
        );
        let hook_body = format!(r#"{{"pids":[1],"harness":"claude","payload":{payload}}}"#);
        post(port, "POST", AGENT_HOOK, hook_body.as_bytes());
        let received = next_event(receiver);
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
        expect_dropped(&collecting, AGENT_HOOK, string_payload);

        let from_a_page = "Origin: https://example.com\r\n";
        let refused = post_with(port, "POST", AGENT_HOOK, from_a_page, hook_body.as_bytes());
        assert_eq!(refused, 403);
        let page_end_body = end_body("[6]");
        let page_end = post_with(
            port,
            "POST",
            SESSION_END,
            from_a_page,
            page_end_body.as_bytes(),
        );
        assert_eq!(page_end, 403);
        let page_check = post_with(port, "POST", UPDATE_CHECK_ROUTE, from_a_page, b"");
        assert_eq!(page_check, 403);
        let after_page_body = end_body("[8]");
        post(port, "POST", SESSION_END, after_page_body.as_bytes());
        let after_page = next_event(receiver);
        assert_eq!(after_page, ended(vec![8]));

        let mut stalled = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let stalled_head = format!("POST {AGENT_HOOK} HTTP/1.1\r\nContent-Length: 5000\r\n\r\n{{");
        stalled.write_all(stalled_head.as_bytes()).unwrap();
        let after_stall_body = end_body("[7]");
        post(port, "POST", SESSION_END, after_stall_body.as_bytes());
        let after_stall = next_event(receiver);
        assert_eq!(after_stall, ended(vec![7]));
        drop(stalled);

        collecting.listener.stop();
    }

    fn activity_post(tool_name: &str) -> String {
        let payload = serde_json::json!({
            "session_id": SESSION,
            "hook_event_name": "PostToolUse",
            "tool_name": tool_name,
            "tool_input": {"text": "did a thing"},
        });
        let body = serde_json::json!({"pids": [1], "harness": "claude", "payload": payload});
        body.to_string()
    }

    #[test]
    fn activity_calls_need_the_apps_token() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join(TOKEN_FILE_NAME);
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

        let mode = fs::metadata(&token_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let token = fs::read_to_string(&token_file).unwrap();

        let note = activity_post("mcp__plugin_taco-shells-dev_activity__note");
        let with_token = format!("{TOKEN_HEADER}: {token}\r\n");
        let lowercase_header = TOKEN_HEADER.to_ascii_lowercase();
        let lowercase_token = format!("{lowercase_header}: {token}\r\n");
        let wrong_token = format!("{TOKEN_HEADER}: nope\r\n");

        let status = post_with(port, "POST", AGENT_HOOK, &with_token, note.as_bytes());
        assert_eq!(status, 200);
        let delivered = next_event(&receiver);
        assert!(matches!(
            delivered,
            ListenerEvent::Hook(HookMessage {
                call: HookCall::AgentHook { .. },
                ..
            })
        ));
        let status = post_with(port, "POST", AGENT_HOOK, &lowercase_token, note.as_bytes());
        assert_eq!(status, 200);
        next_event(&receiver);

        let status = post(port, "POST", AGENT_HOOK, note.as_bytes());
        assert_eq!(status, 200);
        let status = post_with(port, "POST", AGENT_HOOK, &wrong_token, note.as_bytes());
        assert_eq!(status, 200);
        let share = activity_post("mcp__plugin_other_activity__share");
        let status = post(port, "POST", AGENT_HOOK, share.as_bytes());
        assert_eq!(status, 200);
        let marker = end_body("[9]");
        let marked = post_with(port, "POST", SESSION_END, &with_token, marker.as_bytes());
        assert_eq!(marked, 200);
        let next = next_event(&receiver);
        assert_eq!(next, ended(vec![9]));

        let ordinary = activity_post("Bash");
        post_with(port, "POST", AGENT_HOOK, &with_token, ordinary.as_bytes());
        let event = next_event(&receiver);
        assert!(matches!(event, ListenerEvent::Hook(_)));

        listener.stop();
        assert!(!token_file.exists());
    }

    fn every_hook_route_body() -> Vec<(&'static str, String)> {
        let start = format!(
            r#"{{"pids":[1],"harness":"claude","sessionId":"{SESSION}","cwd":"/tmp/work"}}"#
        );
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

    #[test]
    fn every_hook_post_needs_the_token() {
        let directory = tempfile::tempdir().unwrap();
        let token_file = directory.path().join(TOKEN_FILE_NAME);
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
        let token = fs::read_to_string(&token_file).unwrap();
        let with_token = format!("{TOKEN_HEADER}: {token}\r\n");
        let routes = every_hook_route_body();

        for (route, body) in &routes {
            let status = post(port, "POST", route, body.as_bytes());
            assert_eq!(status, 200, "{route}");
        }
        let marker = end_body("[9]");
        post_with(port, "POST", SESSION_END, &with_token, marker.as_bytes());
        let first = next_event(&receiver);
        assert_eq!(first, ended(vec![9]));

        for (route, body) in &routes {
            let status = post_with(port, "POST", route, &with_token, body.as_bytes());
            assert_eq!(status, 200, "{route}");
            let delivered = next_event(&receiver);
            assert!(matches!(delivered, ListenerEvent::Hook(_)), "{route}");
        }

        let check = post(port, "POST", UPDATE_CHECK_ROUTE, b"");
        assert_eq!(check, 200);
        assert_eq!(next_event(&receiver), ListenerEvent::UpdateCheck);
        listener.stop();
    }

    #[test]
    fn the_answer_comes_before_the_app_hears_of_it() {
        let (sender, events) = mpsc::channel();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let directory = tempfile::tempdir().unwrap();
        let blocked = HookListener::start_on(
            &[0],
            directory.path().join(PORT_FILE_NAME),
            TEST_DEADLINE,
            deadlines::ANSWER_WINDOW,
            Box::new(move |event| {
                let _ = sender.send(event);
                let waiting = gate.lock().unwrap();
                let _ = waiting.recv();
            }),
        );
        let port = blocked.port().unwrap();
        let payload = format!(r#"{{"session_id":"{SESSION}","hook_event_name":"SessionStart"}}"#);
        let body = format!(r#"{{"pids":[1],"harness":"claude","payload":{payload}}}"#);

        let token = fs::read_to_string(directory.path().join(TOKEN_FILE_NAME)).unwrap();
        let with_token = format!("{TOKEN_HEADER}: {token}\r\n");

        let status = post_with(port, "POST", AGENT_HOOK, &with_token, body.as_bytes());

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

        let file = directory.path().join(PORT_FILE_NAME);
        let listener = HookListener::start_on(
            &[taken, free],
            file.clone(),
            TEST_DEADLINE,
            deadlines::ANSWER_WINDOW,
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
            deadlines::ANSWER_WINDOW,
            Box::new(|_| {}),
        );
        assert_eq!(unbound.port(), None);
        unbound.stop();
        assert_eq!(fs::read_to_string(&other_file).unwrap(), "999");
    }

    #[test]
    fn the_plugin_hook_is_accepted() {
        let collecting = start_collecting();
        let port = collecting.port;
        let receiver = &collecting.receiver;
        let token_header = &collecting.token_header;

        let payloads = [
            serde_json::json!({
                "session_id": SESSION,
                "hook_event_name": "SessionStart",
                "cwd": "/tmp/work",
                "source": "resume",
            }),
            serde_json::json!({
                "session_id": SESSION,
                "hook_event_name": "UserPromptSubmit",
                "cwd": "/tmp/work",
            }),
            serde_json::json!({
                "session_id": SESSION,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt",
            }),
            serde_json::json!({
                "session_id": SESSION,
                "hook_event_name": "SessionEnd",
                "cwd": "/tmp/work",
            }),
        ];
        let pids = [std::process::id()];
        let mut sent = 0;
        for payload in &payloads {
            let posts = taco_plugin::posts_for(payload, &pids);
            for next in &posts {
                let body = next.body.as_bytes();
                let status = post_with(port, "POST", next.route, token_header, body);
                assert_eq!(status, 200, "{}", next.route);
                sent += 1;
            }
        }

        let mut calls = Vec::new();
        let mut hook_events = Vec::new();
        for _ in 0..sent {
            let event = next_event(receiver);
            let ListenerEvent::Hook(message) = event else {
                panic!("not a hook: {event:?}");
            };
            assert!(message.pids.contains(&std::process::id()));
            match message.call {
                HookCall::AgentHook { event, .. } => hook_events.push(event),
                call => calls.push(call),
            }
        }
        collecting.listener.stop();

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

    fn start_questioned(on_event: impl Fn(ListenerEvent) + Send + Sync + 'static) -> Questioned {
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

    fn question_body() -> String {
        let payload = serde_json::json!({
            "session_id": SESSION,
            "hook_event_name": "SessionStart",
            "source": "resume",
        });
        let body = serde_json::json!({"pids": [1], "harness": "claude", "payload": payload});
        body.to_string()
    }

    fn ask(questioned: &Questioned, route: &str) -> (String, Duration) {
        let started = Instant::now();
        let body = question_body();
        let headers = &questioned.token_header;
        let (status, reply) =
            post_for_reply(questioned.port, "POST", route, headers, body.as_bytes());
        assert_eq!(status, 200);
        (reply, started.elapsed())
    }

    fn answering(parts: fn() -> Vec<AnswerPart>) -> impl Fn(ListenerEvent) + Send + Sync + 'static {
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

    fn wait_until(flag: &AtomicBool) -> bool {
        let give_up = Instant::now() + WAIT;
        while Instant::now() < give_up {
            if flag.load(Ordering::SeqCst) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn a_waiting_question_holds_up_no_other_post() {
        let held = Arc::new(Mutex::new(Vec::new()));
        let keeper = Arc::clone(&held);
        let (sender, receiver) = mpsc::channel();
        let questioned = start_questioned(move |event| match event {
            ListenerEvent::Question(question) => keeper.lock().unwrap().push(question.reply),
            other => {
                let _ = sender.send(other);
            }
        });

        let port = questioned.port;
        let headers = questioned.token_header.clone();
        let asker = thread::spawn(move || {
            let started = Instant::now();
            let body = question_body();
            let (_, reply) = post_for_reply(port, "POST", CONTEXT, &headers, body.as_bytes());
            (reply, started.elapsed())
        });
        thread::sleep(Duration::from_millis(100));

        let sent = Instant::now();
        let end = end_body("[3]");
        let status = post_with(
            port,
            "POST",
            SESSION_END,
            &questioned.token_header,
            end.as_bytes(),
        );
        let post_took = sent.elapsed();
        assert_eq!(status, 200);
        assert!(post_took < Duration::from_millis(150), "{post_took:?}");
        assert_eq!(next_event(&receiver), ended(vec![3]));

        let (reply, waited) = asker.join().unwrap();
        assert_eq!(reply, "{}");
        let window = Duration::from_millis(200)..Duration::from_millis(800);
        assert!(window.contains(&waited), "{waited:?}");
    }

    #[test]
    fn a_context_question_is_answered_as_soon_as_it_is_complete() {
        let with_context = start_questioned(answering(|| {
            vec![
                AnswerPart::Placed(true),
                AnswerPart::Context(Some("hello".to_string())),
            ]
        }));
        let (reply, waited) = ask(&with_context, CONTEXT);
        assert_eq!(reply, r#"{"yours":true,"context":"hello"}"#);
        assert!(waited < Duration::from_millis(200), "{waited:?}");

        let placed_only = start_questioned(answering(|| vec![AnswerPart::Placed(true)]));
        let (reply, waited) = ask(&placed_only, CONTEXT);
        assert_eq!(reply, r#"{"yours":true}"#);
        assert!(waited > Duration::from_millis(250), "{waited:?}");

        let elsewhere = start_questioned(answering(|| vec![AnswerPart::Placed(false)]));
        let (reply, waited) = ask(&elsewhere, CONTEXT);
        assert_eq!(reply, "{}");
        assert!(waited < Duration::from_millis(200), "{waited:?}");
    }

    #[test]
    fn a_question_is_answered_at_once_whatever_order_its_parts_arrive_in() {
        let context_first = start_questioned(answering(|| {
            vec![
                AnswerPart::Context(Some("hello".to_string())),
                AnswerPart::Placed(true),
            ]
        }));
        let (reply, waited) = ask(&context_first, CONTEXT);
        assert_eq!(reply, r#"{"yours":true,"context":"hello"}"#);
        assert!(waited < Duration::from_millis(200), "{waited:?}");

        let nothing_to_say = start_questioned(answering(|| {
            vec![AnswerPart::Placed(true), AnswerPart::Nudge(None, None)]
        }));
        let (reply, waited) = ask(&nothing_to_say, STOP);
        assert_eq!(reply, "{}");
        assert!(waited < Duration::from_millis(200), "{waited:?}");
    }

    #[test]
    fn a_stop_question_runs_on_delivered_only_when_the_reason_was_sent() {
        let delivered = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&delivered);
        let in_time = start_questioned(move |event| {
            let ListenerEvent::Question(question) = event else {
                return;
            };
            let flag = Arc::clone(&flag);
            let on_delivered: OnDelivered = Box::new(move || flag.store(true, Ordering::SeqCst));
            let _ = question.reply.send(AnswerPart::Placed(true));
            let nudge = AnswerPart::Nudge(Some("write a note".to_string()), Some(on_delivered));
            let _ = question.reply.send(nudge);
        });
        let (reply, _) = ask(&in_time, STOP);
        assert_eq!(reply, r#"{"reason":"write a note"}"#);
        assert!(wait_until(&delivered));

        let late_delivered = Arc::new(AtomicBool::new(false));
        let late_flag = Arc::clone(&late_delivered);
        let late_sender: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));
        let late_slot = Arc::clone(&late_sender);
        let late = start_questioned(move |event| {
            let ListenerEvent::Question(question) = event else {
                return;
            };
            let flag = Arc::clone(&late_flag);
            let sender = thread::spawn(move || {
                let on_delivered: OnDelivered =
                    Box::new(move || flag.store(true, Ordering::SeqCst));
                let _ = question.reply.send(AnswerPart::Placed(true));
                thread::sleep(Duration::from_millis(600));
                let nudge = AnswerPart::Nudge(Some("too late".to_string()), Some(on_delivered));
                let _ = question.reply.send(nudge);
            });
            *late_slot.lock().unwrap() = Some(sender);
        });
        let (reply, _) = ask(&late, STOP);
        assert_eq!(reply, "{}");
        let sender = late_sender.lock().unwrap().take().unwrap();
        sender.join().unwrap();
        assert!(!late_delivered.load(Ordering::SeqCst));
    }

    #[test]
    fn a_question_without_the_token_gets_nothing_and_raises_no_event() {
        let (sender, receiver) = mpsc::channel();
        let questioned = start_questioned(move |event| {
            let _ = sender.send(event);
        });
        let port = questioned.port;
        let body = question_body();

        for route in [CONTEXT, STOP] {
            let started = Instant::now();
            let (status, reply) = post_for_reply(port, "POST", route, "", body.as_bytes());
            assert_eq!(status, 200, "{route}");
            assert_eq!(reply, "{}", "{route}");
            assert!(started.elapsed() < Duration::from_millis(500), "{route}");

            let wrong = format!("{TOKEN_HEADER}: nope\r\n");
            let (_, reply) = post_for_reply(port, "POST", route, &wrong, body.as_bytes());
            assert_eq!(reply, "{}", "{route}");
        }

        let marker = end_body("[9]");
        let headers = &questioned.token_header;
        post_with(port, "POST", SESSION_END, headers, marker.as_bytes());
        assert_eq!(next_event(&receiver), ended(vec![9]));
    }
}
