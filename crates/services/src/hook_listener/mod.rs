mod http;
mod messages;
mod questions;
#[cfg(test)]
mod test_support;

use std::fs;
use std::io::{ErrorKind, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use taco_protocol::routes::{CONTEXT, STOP};
use taco_protocol::{HookMessage, deadlines};

use taco_protocol::TOKEN_FILE_NAME;

use http::{body_cap, drain_body, parse_head, read_body, read_head, write_response};
use messages::parse_message;
use questions::ask_question;
pub use questions::{AnswerPart, OnDelivered, Question, QuestionKind};

pub const FOCUS_PORTS: std::ops::RangeInclusive<u16> = 23456..=23460;

const UPDATE_CHECK_ROUTE: &str = "/update/check";

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

pub struct HookListener {
    port: Option<u16>,
    port_file: PathBuf,
    token_file: PathBuf,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Serving {
    read_deadline: Duration,
    answer_window: Duration,
    token: String,
    on_event: OnEvent,
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

        let serving = Serving {
            read_deadline,
            answer_window,
            token,
            on_event,
        };
        let thread_stopping = Arc::clone(&stopping);
        let thread = thread::spawn(move || {
            serve(&listener, &thread_stopping, &serving);
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

fn replace_token_file(temporary: &Path, path: &Path, token: &str) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options.open(temporary)?;
    file.write_all(token.as_bytes())?;
    fs::rename(temporary, path)
}

fn write_token_file(path: &Path, token: &str) {
    let temporary_name = format!(".{TOKEN_FILE_NAME}.{}.tmp", std::process::id());
    let temporary = path.with_file_name(temporary_name);
    let _ = fs::remove_file(&temporary);
    let replaced = replace_token_file(&temporary, path, token);
    if replaced.is_err() {
        let _ = fs::remove_file(&temporary);
        let _ = fs::remove_file(path);
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

fn serve(listener: &TcpListener, stopping: &AtomicBool, serving: &Serving) {
    // One request at a time, so the app hears a session's start before its first state.
    for incoming in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = incoming else {
            continue;
        };
        let deadline = Instant::now() + serving.read_deadline;
        handle(stream, deadline, serving);
    }
}

fn handle(mut stream: TcpStream, deadline: Instant, serving: &Serving) {
    let token = serving.token.as_str();
    let on_event = &serving.on_event;
    let Some((head_bytes, leftover)) = read_head(&mut stream, deadline) else {
        return;
    };
    let answer_by = Instant::now() + serving.answer_window;
    let Some(head) = parse_head(&head_bytes) else {
        write_response(&mut stream, 400, None);
        return;
    };

    if head.method != "POST" {
        write_response(&mut stream, 404, None);
        return;
    }

    // A browser sends Origin on every cross-origin POST and curl never does, and only our scripts and promote call this port.
    if head.has_origin {
        let drained = drain_body(&mut stream, leftover.len(), head.content_length, deadline);
        if drained {
            write_response(&mut stream, 403, None);
        }
        return;
    }

    if head.route == UPDATE_CHECK_ROUTE {
        write_response(&mut stream, 200, None);
        on_event(ListenerEvent::UpdateCheck);
        return;
    }

    let Some(cap) = body_cap(&head.route) else {
        write_response(&mut stream, 404, None);
        return;
    };
    if head.chunked {
        write_response(&mut stream, 400, None);
        return;
    }

    if head.content_length > cap {
        let drained = drain_body(&mut stream, leftover.len(), head.content_length, deadline);
        if drained {
            write_response(&mut stream, 413, None);
        }
        return;
    }

    let Some(body) = read_body(&mut stream, leftover, head.content_length, deadline) else {
        return;
    };

    let sent_token = head.token.as_deref();
    let is_question = head.route == CONTEXT || head.route == STOP;
    if is_question {
        ask_question(stream, &head.route, sent_token, &body, answer_by, serving);
        return;
    }

    // Answered before any work on the body, so a hook that blocks the agent never waits on the app.
    write_response(&mut stream, 200, None);

    // Every post comes from the plugin and carries the token; one without it is another process.
    if sent_token != Some(token) {
        return;
    }
    let message = parse_message(&head.route, &body);
    if let Some(message) = message {
        on_event(ListenerEvent::Hook(message));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::mpsc;

    use taco_protocol::routes::{AGENT_HOOK, SESSION_END, SESSION_START, SESSION_STATE};
    use taco_protocol::{AgentSignal, HookCall, PORT_FILE_NAME, TOKEN_HEADER};

    use super::test_support::*;
    use super::*;

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
