use std::io::{ErrorKind, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use taco_protocol::TOKEN_HEADER;

use crate::hook::Post;

const WRITE_CHUNK: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub enum PostError {
    Refused,
    Other,
}

pub(crate) fn time_left(deadline: Instant) -> Result<Duration, PostError> {
    let left = deadline.checked_duration_since(Instant::now());
    match left {
        Some(left) if !left.is_zero() => Ok(left),
        _ => Err(PostError::Other),
    }
}

pub(crate) fn connect(port: u16, deadline: Instant) -> Result<TcpStream, PostError> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let connect_budget = time_left(deadline)?;
    let connected = TcpStream::connect_timeout(&address, connect_budget);
    match connected {
        Ok(stream) => Ok(stream),
        Err(problem) if problem.kind() == ErrorKind::ConnectionRefused => Err(PostError::Refused),
        Err(_) => Err(PostError::Other),
    }
}

pub(crate) fn send_request(
    stream: &mut TcpStream,
    route: &str,
    body: &str,
    token: Option<&str>,
    deadline: Instant,
) -> Result<(), PostError> {
    let mut token_header = String::new();
    if let Some(token) = token {
        token_header = format!("{TOKEN_HEADER}: {token}\r\n");
    }
    let text = format!(
        "POST {route} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n{token_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    for chunk in text.as_bytes().chunks(WRITE_CHUNK) {
        let write_budget = time_left(deadline)?;
        stream
            .set_write_timeout(Some(write_budget))
            .map_err(|_| PostError::Other)?;
        stream.write_all(chunk).map_err(|_| PostError::Other)?;
    }
    Ok(())
}

pub fn post(
    port: u16,
    request: &Post,
    token: Option<&str>,
    deadline: Instant,
) -> Result<(), PostError> {
    let mut stream = connect(port, deadline)?;
    // The listener reads the whole body before it answers, so waiting for the answer only adds time.
    send_request(&mut stream, request.route, &request.body, token, deadline)?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;
    use taco_protocol::routes::SESSION_STATE;

    fn soon(milliseconds: u64) -> Instant {
        Instant::now() + Duration::from_millis(milliseconds)
    }

    #[test]
    fn sends_the_route_and_body_as_one_http_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut received = String::new();
            let mut buffer = [0u8; 1024];
            while !received.ends_with("{\"a\":1}") {
                let count = stream.read(&mut buffer).unwrap();
                received.push_str(&String::from_utf8_lossy(&buffer[..count]));
            }
            received
        });

        let sent = Post {
            route: SESSION_STATE,
            body: String::from("{\"a\":1}"),
        };
        post(port, &sent, Some("abc123"), soon(1000)).unwrap();

        let received = server.join().unwrap();
        let first_line = received.lines().next().unwrap();
        assert_eq!(first_line, format!("POST {SESSION_STATE} HTTP/1.1"));
        assert!(received.contains(&format!("\r\n{TOKEN_HEADER}: abc123\r\n")));
        assert!(received.ends_with("\r\n\r\n{\"a\":1}"));
    }

    #[test]
    fn no_token_means_no_header() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = String::new();
            stream.read_to_string(&mut received).unwrap();
            received
        });

        let sent = Post {
            route: SESSION_STATE,
            body: String::from("{}"),
        };
        post(port, &sent, None, soon(1000)).unwrap();

        let received = server.join().unwrap();
        assert!(
            !received
                .to_ascii_lowercase()
                .contains("x-taco-shells-token")
        );
    }

    #[test]
    fn a_port_with_no_listener_is_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let sent = Post {
            route: SESSION_STATE,
            body: String::from("{}"),
        };
        assert_eq!(post(port, &sent, None, soon(1000)), Err(PostError::Refused));
    }

    #[test]
    fn a_listener_that_never_reads_costs_no_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let sent = Post {
            route: SESSION_STATE,
            body: String::from("{}"),
        };
        let started = Instant::now();
        let outcome = post(port, &sent, None, soon(250));

        assert_eq!(outcome, Ok(()));
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(listener);
    }

    #[test]
    fn a_large_body_to_a_listener_that_never_reads_gives_up_at_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepter = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_secs(3));
            drop(stream);
        });

        let sent = Post {
            route: SESSION_STATE,
            body: "x".repeat(2 * 1024 * 1024),
        };
        let started = Instant::now();
        let outcome = post(port, &sent, None, soon(250));

        assert!(outcome.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        accepter.join().unwrap();
    }
}
