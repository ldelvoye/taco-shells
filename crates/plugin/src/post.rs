use std::net::Shutdown;
use std::time::Instant;

use crate::hook::Request;
use crate::http::{connect, send_request};

#[derive(Debug, PartialEq)]
pub struct PostError;

pub fn post(
    port: u16,
    request: &Request,
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
    use std::time::Duration;
    use taco_protocol::TOKEN_HEADER;
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

        let sent = Request {
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

        let sent = Request {
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
    fn a_port_with_no_listener_fails_at_once() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let sent = Request {
            route: SESSION_STATE,
            body: String::from("{}"),
        };
        let started = Instant::now();
        let outcome = post(port, &sent, None, soon(5000));

        assert!(outcome.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn a_listener_that_never_reads_costs_no_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let sent = Request {
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

        let sent = Request {
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
