use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Instant;

use taco_protocol::TOKEN_HEADER;
use taco_protocol::routes::{AGENT_HOOK, CONTEXT, SESSION_END, SESSION_START, SESSION_STATE, STOP};

use super::READ_DEADLINE;

const SMALL_BODY_CAP: u64 = 64 * 1024;
const AGENT_HOOK_BODY_CAP: u64 = 16 * 1024 * 1024;
const HEAD_CAP: usize = 16 * 1024;
const READ_CHUNK: usize = 64 * 1024;
const HEAD_END: &[u8] = b"\r\n\r\n";

pub(super) struct Head {
    pub(super) method: String,
    pub(super) route: String,
    pub(super) content_length: u64,
    pub(super) chunked: bool,
    pub(super) has_origin: bool,
    pub(super) token: Option<String>,
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

pub(super) fn read_head(stream: &mut TcpStream, deadline: Instant) -> Option<(Vec<u8>, Vec<u8>)> {
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

pub(super) fn parse_head(bytes: &[u8]) -> Option<Head> {
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

pub(super) fn read_body(
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

pub(super) fn drain_body(
    stream: &mut TcpStream,
    already: usize,
    length: u64,
    deadline: Instant,
) -> bool {
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

pub(super) fn respond(stream: &mut TcpStream, status: u16) {
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

pub(super) fn respond_with_body(stream: &mut TcpStream, body: &str) -> bool {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.set_write_timeout(Some(READ_DEADLINE));
    let written = stream.write_all(response.as_bytes());
    written.is_ok()
}

pub(super) fn body_cap(route: &str) -> Option<u64> {
    match route {
        AGENT_HOOK | CONTEXT | STOP => Some(AGENT_HOOK_BODY_CAP),
        SESSION_START | SESSION_END | SESSION_STATE => Some(SMALL_BODY_CAP),
        _ => None,
    }
}
