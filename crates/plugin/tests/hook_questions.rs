use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use taco_plugin::PLUGIN_NAME;
use taco_protocol::{APP_VARIABLE, PORT_FILE_VARIABLE};

const CONTEXT_ROUTE: &str = "/agent-session/context";
const STOP_ROUTE: &str = "/agent-session/stop";
const POST_ROUTES: [&str; 3] = ["/attention", "/agent-session/state", "/agent-session/hook"];
const HEAD_END: &[u8] = b"\r\n\r\n";
const HELD_FOR: Duration = Duration::from_secs(6);
const POLL_LIMIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy)]
enum Reply {
    Json(&'static str),
    Hold,
}

struct Output {
    stdout: String,
    status_code: Option<i32>,
    elapsed: Duration,
}

fn read_request(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut received: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 4096];
    loop {
        let head_end = received.windows(HEAD_END.len()).position(|w| w == HEAD_END);
        if let Some(position) = head_end {
            let head = String::from_utf8_lossy(&received[..position]).to_string();
            let length = content_length(&head);
            let body_start = position + HEAD_END.len();
            if received.len() >= body_start + length {
                return head.lines().next().map(String::from);
            }
        }
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        received.extend_from_slice(&buffer[..count]);
    }
}

fn content_length(head: &str) -> usize {
    for line in head.lines() {
        let lowered = line.to_ascii_lowercase();
        if let Some(value) = lowered.strip_prefix("content-length:") {
            return value.trim().parse().unwrap_or(0);
        }
    }
    0
}

fn handle(mut stream: TcpStream, reply: Reply, routes: &Mutex<Vec<String>>) {
    let Some(request_line) = read_request(&mut stream) else {
        return;
    };
    let route = request_line.split(' ').nth(1).unwrap_or("").to_string();
    routes.lock().unwrap().push(route.clone());
    let is_question = route == CONTEXT_ROUTE || route == STOP_ROUTE;
    if !is_question {
        return;
    }
    match reply {
        Reply::Json(body) => {
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
        Reply::Hold => thread::sleep(HELD_FOR),
    }
}

fn stand_in(reply: Reply) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let routes = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&routes);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let seen = Arc::clone(&seen);
            thread::spawn(move || handle(stream, reply, &seen));
        }
    });
    (port, routes)
}

fn write_port_files(directory: &Path, port: u16) {
    std::fs::write(directory.join("focus-port"), format!("{port}\n")).unwrap();
    std::fs::write(directory.join("focus-token"), "test-token\n").unwrap();
}

fn run_hook(directory: &Path, payload: &Value) -> Output {
    let path = std::env::var_os("PATH").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_taco-shells-plugin"))
        .arg("hook")
        .env_clear()
        .env("PATH", path)
        .env("HOME", directory)
        .env(PORT_FILE_VARIABLE, directory.join("focus-port"))
        .env(APP_VARIABLE, PLUGIN_NAME)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(payload.to_string().as_bytes()).unwrap();
    drop(stdin);
    let finished = child.wait_with_output().unwrap();
    Output {
        stdout: String::from_utf8(finished.stdout).unwrap(),
        status_code: finished.status.code(),
        elapsed: started.elapsed(),
    }
}

fn stop_payload() -> Value {
    json!({"hook_event_name": "Stop", "session_id": "s1"})
}

fn seen_routes(routes: &Mutex<Vec<String>>) -> Vec<String> {
    routes.lock().unwrap().clone()
}

fn wait_for_posts(routes: &Mutex<Vec<String>>) -> Vec<String> {
    let started = Instant::now();
    loop {
        let seen = seen_routes(routes);
        let has_all = POST_ROUTES
            .iter()
            .all(|route| seen.iter().any(|seen_route| seen_route == route));
        if has_all || started.elapsed() > POLL_LIMIT {
            return seen;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_nudged_stop_prints_one_block_decision_and_sends_no_posts() {
    let directory = tempfile::tempdir().unwrap();
    let (port, routes) = stand_in(Reply::Json(r#"{"reason":"write a note"}"#));
    write_port_files(directory.path(), port);

    let output = run_hook(directory.path(), &stop_payload());

    assert_eq!(output.status_code, Some(0));
    assert!(output.stdout.ends_with('\n'));
    assert_eq!(output.stdout.lines().count(), 1);
    let printed: Value = serde_json::from_str(&output.stdout).unwrap();
    assert_eq!(
        printed,
        json!({"decision": "block", "reason": "write a note"})
    );
    thread::sleep(Duration::from_millis(300));
    assert_eq!(seen_routes(&routes), vec![STOP_ROUTE.to_string()]);
}

#[test]
fn a_stop_the_app_never_answers_exits_in_time_silently_and_still_posts() {
    let directory = tempfile::tempdir().unwrap();
    let (port, routes) = stand_in(Reply::Hold);
    write_port_files(directory.path(), port);

    let output = run_hook(directory.path(), &stop_payload());

    assert_eq!(output.status_code, Some(0));
    assert!(output.elapsed < Duration::from_millis(2500));
    assert_eq!(output.stdout, "");
    let seen = wait_for_posts(&routes);
    for route in POST_ROUTES {
        assert!(seen.iter().any(|seen_route| seen_route == route), "{route}");
    }
}

#[test]
fn a_stop_already_continued_by_a_hook_asks_nothing_and_still_posts() {
    let directory = tempfile::tempdir().unwrap();
    let (port, routes) = stand_in(Reply::Json(r#"{"reason":"write a note"}"#));
    write_port_files(directory.path(), port);
    let payload = json!({"hook_event_name": "Stop", "session_id": "s1", "stop_hook_active": true});

    let output = run_hook(directory.path(), &payload);

    assert_eq!(output.status_code, Some(0));
    assert_eq!(output.stdout, "");
    let seen = wait_for_posts(&routes);
    assert!(!seen.iter().any(|route| route == STOP_ROUTE));
    for route in POST_ROUTES {
        assert!(seen.iter().any(|seen_route| seen_route == route), "{route}");
    }
}

#[test]
fn a_session_start_the_app_does_not_claim_prints_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let (port, _routes) = stand_in(Reply::Json("{}"));
    write_port_files(directory.path(), port);
    let payload = json!({
        "hook_event_name": "SessionStart",
        "session_id": "s1",
        "cwd": "/work",
        "source": "startup",
    });

    let output = run_hook(directory.path(), &payload);

    assert_eq!(output.status_code, Some(0));
    assert_eq!(output.stdout, "");
}

#[test]
fn with_no_listener_the_hook_exits_at_once_and_prints_nothing() {
    let directory = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    write_port_files(directory.path(), port);

    let output = run_hook(directory.path(), &stop_payload());

    assert_eq!(output.status_code, Some(0));
    assert!(output.elapsed < Duration::from_secs(1));
    assert_eq!(output.stdout, "");
}
