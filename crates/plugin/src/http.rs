use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use taco_protocol::TOKEN_HEADER;

use crate::post::PostError;

const WRITE_CHUNK: usize = 64 * 1024;

pub(crate) fn time_left(deadline: Instant) -> Result<Duration, PostError> {
    let left = deadline.checked_duration_since(Instant::now());
    match left {
        Some(left) if !left.is_zero() => Ok(left),
        _ => Err(PostError),
    }
}

pub(crate) fn connect(port: u16, deadline: Instant) -> Result<TcpStream, PostError> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let connect_budget = time_left(deadline)?;
    let connected = TcpStream::connect_timeout(&address, connect_budget);
    match connected {
        Ok(stream) => Ok(stream),
        Err(_) => Err(PostError),
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
            .map_err(|_| PostError)?;
        stream.write_all(chunk).map_err(|_| PostError)?;
    }
    Ok(())
}
