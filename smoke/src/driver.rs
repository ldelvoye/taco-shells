use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use taco_drive::{Appearance, DriveReply, DriveRequest, Point, Rect};
use taco_protocol::{DrawnState, TerminalId};

const CONNECT_RETRY: Duration = Duration::from_millis(100);
const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Driver {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

impl Driver {
    pub fn connect(socket: &Path, deadline: Duration) -> Result<Driver, String> {
        let start = Instant::now();
        loop {
            let attempt = UnixStream::connect(socket);
            match attempt {
                Ok(stream) => return Driver::new(stream),
                Err(error) => {
                    if start.elapsed() >= deadline {
                        return Err(format!(
                            "could not connect to {} within {deadline:?}: {error}",
                            socket.display()
                        ));
                    }
                }
            }
            thread::sleep(CONNECT_RETRY);
        }
    }

    fn new(stream: UnixStream) -> Result<Driver, String> {
        stream
            .set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|error| format!("could not set the read timeout: {error}"))?;
        let reading = stream
            .try_clone()
            .map_err(|error| format!("could not clone the socket: {error}"))?;
        Ok(Driver {
            writer: stream,
            reader: BufReader::new(reading),
        })
    }

    pub fn request(&mut self, request: &DriveRequest) -> Result<DriveReply, String> {
        let json = serde_json::to_string(request)
            .map_err(|error| format!("could not encode {request:?}: {error}"))?;
        let line = format!("{json}\n");
        self.writer
            .write_all(line.as_bytes())
            .map_err(|error| format!("could not send {json}: {error}"))?;

        let mut answer = String::new();
        let read = self
            .reader
            .read_line(&mut answer)
            .map_err(|error| format!("no reply to {json}: {error}"))?;
        if read == 0 {
            return Err(format!("the app closed the socket before answering {json}"));
        }
        serde_json::from_str(&answer)
            .map_err(|error| format!("could not read reply {answer:?}: {error}"))
    }

    pub fn keys(&mut self, keys: &[&str]) -> Result<(), String> {
        let names: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
        let request = DriveRequest::Keys { keys: names };
        let reply = self.request(&request)?;
        expect_done(reply, "keys")
    }

    pub fn text(&mut self, text: &str) -> Result<(), String> {
        let request = DriveRequest::Text {
            text: text.to_string(),
        };
        let reply = self.request(&request)?;
        expect_done(reply, "text")
    }

    pub fn drawn_state(&mut self) -> Result<DrawnState, String> {
        let reply = self.request(&DriveRequest::DrawnState)?;
        match reply {
            DriveReply::DrawnState(state) => Ok(*state),
            other => Err(format!("unexpected reply to drawn_state: {other:?}")),
        }
    }

    pub fn screen_text(&mut self, terminal: &TerminalId) -> Result<Option<Vec<String>>, String> {
        let request = DriveRequest::ScreenText {
            terminal: terminal.clone(),
        };
        let reply = self.request(&request)?;
        match reply {
            DriveReply::ScreenText(lines) => Ok(Some(lines)),
            DriveReply::Failed(_) => Ok(None),
            other => Err(format!("unexpected reply to screen_text: {other:?}")),
        }
    }

    pub fn bounds(&mut self, target: &str) -> Result<Option<Rect>, String> {
        let request = DriveRequest::Bounds {
            target: target.to_string(),
        };
        let reply = self.request(&request)?;
        match reply {
            DriveReply::Bounds(rect) => Ok(rect),
            other => Err(format!("unexpected reply to bounds: {other:?}")),
        }
    }

    pub fn drag(&mut self, from: Point, to: Point) -> Result<(), String> {
        let reply = self.request(&DriveRequest::Drag { from, to })?;
        expect_done(reply, "drag")
    }

    pub fn render(&mut self, path: &Path, appearance: Appearance) -> Result<(), String> {
        let request = DriveRequest::Render {
            path: path.display().to_string(),
            appearance,
        };
        let reply = self.request(&request)?;
        expect_done(reply, "render")
    }
}

fn expect_done(reply: DriveReply, request: &str) -> Result<(), String> {
    match reply {
        DriveReply::Done => Ok(()),
        DriveReply::Failed(reason) => Err(format!("{request} failed: {reason}")),
        other => Err(format!("unexpected reply to {request}: {other:?}")),
    }
}
