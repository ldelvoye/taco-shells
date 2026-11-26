//! The terminal channel: frames in from the runtime on one thread, requests out to it on another.

use std::io;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;

use futures::SinkExt as _;
use futures::channel::mpsc::{Receiver, channel};
use futures::executor::block_on;
use taco_protocol::{TerminalEvent, TerminalRequest};

/// Events read but not yet applied. When the window falls behind, reading stops here and the runtime is held back, rather than its output piling up in memory.
const EVENTS_IN_FLIGHT: usize = 256;

/// The sending half. Requests queue on a thread of their own, so a large paste never holds up a frame.
pub struct Requests {
    sender: mpsc::Sender<TerminalRequest>,
}

impl Requests {
    /// Queued, or dropped once the runtime is gone.
    pub fn send(&self, request: TerminalRequest) {
        let _ = self.sender.send(request);
    }
}

/// Starts the two threads. The events end when the runtime closes the connection; dropping `Requests` closes it from this side.
pub fn connect(stream: UnixStream) -> io::Result<(Requests, Receiver<TerminalEvent>)> {
    let mut reader = stream.try_clone()?;
    let mut writer = stream;
    let (mut event_sender, events) = channel(EVENTS_IN_FLIGHT);
    let (sender, requests) = mpsc::channel::<TerminalRequest>();

    thread::Builder::new()
        .name("terminal-channel-read".to_string())
        .spawn(move || {
            while let Ok(Some(event)) = taco_wire::read_frame::<TerminalEvent>(&mut reader) {
                let delivered = block_on(event_sender.send(event));
                if delivered.is_err() {
                    break;
                }
            }
        })?;
    thread::Builder::new()
        .name("terminal-channel-write".to_string())
        .spawn(move || {
            for request in requests {
                if taco_wire::write_frame(&mut writer, &request).is_err() {
                    break;
                }
            }
            // Both directions, so the reader thread wakes and the runtime sees the connection close.
            let _ = writer.shutdown(Shutdown::Both);
        })?;
    Ok((Requests { sender }, events))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::Duration;
    use taco_protocol::TerminalId;

    #[test]
    fn letting_go_of_the_requests_closes_the_connection() {
        let (ours, mut runtime) = UnixStream::pair().unwrap();
        let (requests, events) = connect(ours).unwrap();
        let clear = TerminalRequest::Clear {
            id: TerminalId("t1".to_string()),
        };
        requests.send(clear.clone());
        let received: TerminalRequest = taco_wire::read_frame(&mut runtime).unwrap().unwrap();
        assert_eq!(received, clear);

        drop(requests);
        drop(events);
        // Only a hang would trip the timeout; a closed connection reads end of file at once.
        runtime
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut rest = Vec::new();
        let read = runtime.read_to_end(&mut rest).unwrap();
        assert_eq!(read, 0);
    }
}
