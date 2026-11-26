//! The terminal channel: frames in from the runtime on one thread, requests out to it on another.

use std::io;
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;

use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use taco_protocol::{TerminalEvent, TerminalRequest};

/// The sending half. Requests queue on a thread of their own, so a large paste never holds up a frame.
#[derive(Clone)]
pub struct Requests {
    sender: mpsc::Sender<TerminalRequest>,
}

impl Requests {
    /// Queued, or dropped when the runtime is gone: its death reaches the app on the control connection.
    pub fn send(&self, request: TerminalRequest) {
        let _ = self.sender.send(request);
    }
}

/// Starts the two threads. The events stream ends when the runtime closes the connection.
pub fn connect(stream: UnixStream) -> io::Result<(Requests, UnboundedReceiver<TerminalEvent>)> {
    let mut reader = stream.try_clone()?;
    let mut writer = stream;
    let (event_sender, events) = unbounded();
    let (sender, requests) = mpsc::channel::<TerminalRequest>();

    thread::Builder::new()
        .name("terminal-channel-read".to_string())
        .spawn(move || {
            while let Ok(Some(event)) = taco_wire::read_frame::<TerminalEvent>(&mut reader) {
                if event_sender.unbounded_send(event).is_err() {
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
        })?;
    Ok((Requests { sender }, events))
}
