use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use taco_protocol::TerminalId;

use super::UNKNOWN_EXIT_CODE;
use crate::budget::Budget;
use crate::core::Input;

/// The most read from a pty at once. One read is what another terminal's echo can wait behind.
const READ_BYTES: usize = 64 * 1024;
/// The most read after a shell has exited, before its exit is reported anyway.
const DRAIN_BYTES: usize = 1024 * 1024;

/// One shell's output, read in order and then its exit, so the exit always follows the last output.
pub(super) struct Reader {
    pub(super) id: TerminalId,
    pub(super) master: File,
    /// A copy of the pty's slave, held until the last output is read: macOS throws away unread output once the slave's last copy closes.
    pub(super) shell_side: OwnedFd,
    /// Readable once the waiter has seen the shell exit.
    pub(super) wake: OwnedFd,
    pub(super) exit_code: Arc<Mutex<Option<i32>>>,
    pub(super) budget: Arc<Budget>,
    pub(super) inputs: Sender<Input>,
}

enum Polled {
    Readable,
    Exited,
    Interrupted,
    Failed,
}

enum Drained {
    Data(usize),
    /// Nothing is left to read, or the pty cannot be read.
    Closed,
    /// The core has gone, so nothing more is wanted.
    Unwanted,
}

impl Reader {
    pub(super) fn run(mut self) {
        let mut buffer = vec![0u8; READ_BYTES];
        let mut pty_open = true;
        loop {
            self.budget.wait_for_room();
            match self.poll(pty_open) {
                Polled::Exited => break,
                Polled::Interrupted => {}
                Polled::Readable => match self.read_once(&mut buffer) {
                    Drained::Data(_) => {}
                    // The exit is still to come.
                    Drained::Closed => pty_open = false,
                    Drained::Unwanted => return,
                },
                // Polling again would only spin: the exit is waited for on its own.
                Polled::Failed => {
                    self.wait_for_exit();
                    break;
                }
            }
        }

        // The shell has exited: what it printed last is read before its exit is reported,
        // up to a limit, so a process that outlives it and keeps printing cannot hold the exit back.
        self.budget.close();
        set_nonblocking(self.master.as_raw_fd());
        let mut drained = 0;
        while drained < DRAIN_BYTES {
            match self.read_once(&mut buffer) {
                Drained::Data(read) => drained += read,
                Drained::Closed => break,
                Drained::Unwanted => return,
            }
        }
        drop(self.shell_side);

        let recorded = *self.exit_code.lock().expect("exit code lock");
        let mut code = UNKNOWN_EXIT_CODE;
        if let Some(recorded) = recorded {
            code = recorded;
        }
        let _ = self.inputs.send(Input::Exited { id: self.id, code });
    }

    fn poll(&self, pty_open: bool) -> Polled {
        let mut fds = [
            libc::pollfd {
                fd: self.wake.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.master.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // The pty stays in the list only while some process still holds it.
        let mut watched = 1;
        if pty_open {
            watched = 2;
        }
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), watched, -1) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                return Polled::Interrupted;
            }
            return Polled::Failed;
        }
        if fds[0].revents != 0 {
            return Polled::Exited;
        }
        if pty_open && fds[1].revents != 0 {
            return Polled::Readable;
        }
        Polled::Interrupted
    }

    fn wait_for_exit(&self) {
        let mut byte = 0u8;
        loop {
            let pointer = &mut byte as *mut u8 as *mut libc::c_void;
            let read = unsafe { libc::read(self.wake.as_raw_fd(), pointer, 1) };
            let interrupted =
                read < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted;
            if !interrupted {
                return;
            }
        }
    }

    fn read_once(&mut self, buffer: &mut [u8]) -> Drained {
        let read = match self.master.read(buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Drained::Data(0),
            Err(_) => return Drained::Closed,
        };
        if read == 0 {
            return Drained::Closed;
        }
        self.budget.spend(read);
        let data = buffer[..read].to_vec();
        let output = Input::Output {
            id: self.id.clone(),
            data,
        };
        if self.inputs.send(output).is_err() {
            return Drained::Unwanted;
        }
        Drained::Data(read)
    }
}

fn set_nonblocking(fd: RawFd) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
}
