use std::io::{ErrorKind, Read};
use std::net::TcpStream;
use std::time::Instant;

use serde_json::Value;

use crate::post::{connect, send_request, time_left};

const READ_CHUNK: usize = 8 * 1024;
const HEAD_END: &str = "\r\n\r\n";
const OK_STATUS_LINE: &str = "HTTP/1.1 200";

fn read_response(stream: &mut TcpStream, deadline: Instant) -> Option<String> {
    let mut received: Vec<u8> = Vec::new();
    let mut buffer = [0u8; READ_CHUNK];
    loop {
        let remaining = time_left(deadline);
        let Ok(left) = remaining else {
            return None;
        };
        let timeout_set = stream.set_read_timeout(Some(left));
        if timeout_set.is_err() {
            return None;
        }
        let read = stream.read(&mut buffer);
        match read {
            Ok(0) => break,
            Ok(count) => received.extend_from_slice(&buffer[..count]),
            Err(problem) if problem.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    let text = String::from_utf8(received);
    let Ok(text) = text else {
        return None;
    };
    Some(text)
}

fn json_body(response: &str) -> Option<Value> {
    if !response.starts_with(OK_STATUS_LINE) {
        return None;
    }
    let (_, body) = response.split_once(HEAD_END)?;
    let parsed = serde_json::from_str(body);
    let Ok(value) = parsed else {
        return None;
    };
    Some(value)
}

pub fn ask(
    port: u16,
    route: &str,
    body: &str,
    token: Option<&str>,
    deadline: Instant,
) -> Option<Value> {
    let connected = connect(port, deadline);
    let Ok(mut stream) = connected else {
        return None;
    };
    let sent = send_request(&mut stream, route, body, token, deadline);
    if sent.is_err() {
        return None;
    }
    let response = read_response(&mut stream, deadline)?;
    json_body(&response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::time::Duration;
    use taco_protocol::routes::STOP;

    fn soon(milliseconds: u64) -> Instant {
        Instant::now() + Duration::from_millis(milliseconds)
    }

    fn answering_with(reply: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            stream.write_all(reply.as_bytes()).unwrap();
        });
        port
    }

    #[test]
    fn a_200_answer_is_parsed_and_anything_else_is_none() {
        let good =
            answering_with("HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"reason\":\"go\"}");
        let answer = ask(good, STOP, "{}", Some("t"), soon(1000));
        assert_eq!(answer, Some(serde_json::json!({"reason": "go"})));

        let refused = answering_with("HTTP/1.1 403 Forbidden\r\n\r\n{}");
        assert_eq!(ask(refused, "/x", "{}", None, soon(1000)), None);

        let garbled = answering_with("HTTP/1.1 200 OK\r\n\r\nnot json");
        assert_eq!(ask(garbled, "/x", "{}", None, soon(1000)), None);
    }

    #[test]
    fn a_listener_that_never_answers_is_none_at_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = Instant::now();

        let answer = ask(port, "/x", "{}", None, soon(200));

        assert_eq!(answer, None);
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(listener);
    }
}
