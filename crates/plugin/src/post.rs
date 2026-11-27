use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::hook::Post;

const TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, PartialEq)]
pub enum PostError {
    Refused,
    Other,
}

pub fn post(port: u16, post: &Post) -> Result<(), PostError> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let connected = TcpStream::connect_timeout(&address, TIMEOUT);
    let mut stream = match connected {
        Ok(stream) => stream,
        Err(problem) if problem.kind() == ErrorKind::ConnectionRefused => {
            return Err(PostError::Refused);
        }
        Err(_) => return Err(PostError::Other),
    };

    stream
        .set_read_timeout(Some(TIMEOUT))
        .map_err(|_| PostError::Other)?;
    stream
        .set_write_timeout(Some(TIMEOUT))
        .map_err(|_| PostError::Other)?;

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        post.route,
        post.body.len(),
        post.body,
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|_| PostError::Other)?;

    let mut ignored = Vec::new();
    let _ = stream.read_to_end(&mut ignored);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn sends_the_route_and_body_as_one_http_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut received = String::new();
            let mut buffer = [0u8; 1024];
            while !received.ends_with("{\"a\":1}") {
                let count = stream.read(&mut buffer).unwrap();
                received.push_str(&String::from_utf8_lossy(&buffer[..count]));
            }
            received
        });

        let sent = Post {
            route: "/attention",
            body: String::from("{\"a\":1}"),
        };
        post(port, &sent).unwrap();

        let received = server.join().unwrap();
        let first_line = received.lines().next().unwrap();
        assert_eq!(first_line, "POST /attention HTTP/1.1");
        assert!(received.contains("Content-Length: 7\r\n"));
        assert!(received.ends_with("\r\n\r\n{\"a\":1}"));
    }

    #[test]
    fn a_port_with_no_listener_is_refused() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let sent = Post {
            route: "/attention",
            body: String::from("{}"),
        };
        assert_eq!(post(port, &sent), Err(PostError::Refused));
    }
}
