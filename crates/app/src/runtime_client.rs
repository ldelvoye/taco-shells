//! The app's side of the terminal runtime: starting `taco-shells-runtime`, its control connection, and its exit.

use std::collections::HashMap;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use taco_protocol::{RuntimeAnswer, RuntimeCommand, RuntimeQuery, RuntimeReport};

const PROGRAM: &str = "taco-shells-runtime";
/// Where the runtime finds its two connections, as its arguments say.
const CONTROL_FD: RawFd = 3;
const TERMINALS_FD: RawFd = 4;
/// Above both, so moving one connection into place cannot overwrite the other.
const ABOVE_BOTH: RawFd = 10;
/// How long a stop waits for the runtime to go by itself, and then again after SIGTERM.
const STOP_PATIENCE: Duration = Duration::from_secs(2);

pub enum RuntimeEvent {
    Report(RuntimeReport),
    /// Any exit the app did not ask for. `reason` is shown to the user: "The terminal runtime stopped (exit code 101)."
    Died {
        reason: String,
    },
}

#[derive(Clone)]
pub struct RuntimeClient {
    shared: Arc<Shared>,
}

pub struct RuntimeStart {
    pub client: RuntimeClient,
    /// The terminals connection, handed to `terminal-view` as it is.
    pub terminals: UnixStream,
}

struct Shared {
    control: Mutex<UnixStream>,
    pid: u32,
    next_ask: AtomicU64,
    /// Asks waiting for their answer. Taken whole, and answered with none, once the runtime is gone.
    asks: Mutex<Option<HashMap<u64, mpsc::Sender<Option<RuntimeAnswer>>>>>,
    stopping: AtomicBool,
    exit: Mutex<Option<ExitStatus>>,
    exited: Condvar,
}

impl RuntimeClient {
    /// `taco-shells-runtime` beside the running executable, in the bundle and in `target/<profile>/` alike.
    pub fn program_beside_this_one() -> io::Result<PathBuf> {
        let this_one = std::env::current_exe()?;
        let Some(directory) = this_one.parent() else {
            let reason = format!("{} has no directory", this_one.display());
            return Err(io::Error::new(io::ErrorKind::NotFound, reason));
        };
        Ok(directory.join(PROGRAM))
    }

    /// Starts it with two socket pairs at descriptors 3 and 4; `on_event` runs on the client's reader thread, in arrival order.
    pub fn start(
        program: &Path,
        on_event: impl FnMut(RuntimeEvent) + Send + 'static,
    ) -> io::Result<RuntimeStart> {
        let (control, control_end) = UnixStream::pair()?;
        let (terminals, terminals_end) = UnixStream::pair()?;
        let control_fd = control_end.as_raw_fd();
        let terminals_fd = terminals_end.as_raw_fd();

        let mut command = Command::new(program);
        command.arg(format!("--control-fd={CONTROL_FD}"));
        command.arg(format!("--terminals-fd={TERMINALS_FD}"));
        command.stdin(Stdio::null());
        // Between fork and exec only async-signal-safe calls. The copies at 3 and 4 stay open across exec; the ones above close.
        unsafe {
            command.pre_exec(move || {
                let control_high = libc::fcntl(control_fd, libc::F_DUPFD_CLOEXEC, ABOVE_BOTH);
                let terminals_high = libc::fcntl(terminals_fd, libc::F_DUPFD_CLOEXEC, ABOVE_BOTH);
                if control_high < 0 || terminals_high < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::dup2(control_high, CONTROL_FD) < 0
                    || libc::dup2(terminals_high, TERMINALS_FD) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn()?;
        // The runtime holds its own ends now; ours would keep its connections open after it died.
        drop(command);
        drop(control_end);
        drop(terminals_end);

        let reader = control.try_clone()?;
        let shared = Arc::new(Shared {
            control: Mutex::new(control),
            pid: child.id(),
            next_ask: AtomicU64::new(0),
            asks: Mutex::new(Some(HashMap::new())),
            stopping: AtomicBool::new(false),
            exit: Mutex::new(None),
            exited: Condvar::new(),
        });

        let waiter_shared = shared.clone();
        thread::Builder::new()
            .name("wait for the runtime".into())
            .spawn(move || {
                let status = child.wait();
                let mut exit = waiter_shared.exit.lock().expect("runtime exit lock");
                *exit = status.ok();
                waiter_shared.exited.notify_all();
            })?;

        let reader_shared = shared.clone();
        thread::Builder::new()
            .name("read the runtime".into())
            .spawn(move || {
                read_reports(reader, reader_shared, on_event);
            })?;

        let client = RuntimeClient { shared };
        Ok(RuntimeStart { client, terminals })
    }

    /// Dropped silently once the runtime is gone.
    pub fn send(&self, command: RuntimeCommand) {
        let mut control = self.shared.control.lock().expect("runtime connection lock");
        let _ = taco_wire::write_frame(&mut *control, &command);
    }

    /// Receives exactly one value: the answer, or `None` once the runtime is gone, including for asks still in flight when it died.
    pub fn ask(&self, query: RuntimeQuery) -> mpsc::Receiver<Option<RuntimeAnswer>> {
        let (answer, answered) = mpsc::channel();
        let ask = self.shared.next_ask.fetch_add(1, Ordering::SeqCst);
        {
            let mut asks = self.shared.asks.lock().expect("runtime asks lock");
            let Some(waiting) = asks.as_mut() else {
                let _ = answer.send(None);
                return answered;
            };
            waiting.insert(ask, answer);
        }
        self.send(RuntimeCommand::Ask { ask, query });
        answered
    }

    /// Sends `Stop`, waits up to two seconds for the exit, then SIGTERM. A stop never produces `RuntimeEvent::Died`.
    pub fn stop(&self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.send(RuntimeCommand::Stop);
        if self.wait_for_exit() {
            return;
        }
        unsafe { libc::kill(self.shared.pid as libc::pid_t, libc::SIGTERM) };
        self.wait_for_exit();
    }

    /// Whether the runtime has exited, waiting up to `STOP_PATIENCE` for it.
    fn wait_for_exit(&self) -> bool {
        let exit = self.shared.exit.lock().expect("runtime exit lock");
        let (exit, _) = self
            .shared
            .exited
            .wait_timeout_while(exit, STOP_PATIENCE, |status| status.is_none())
            .expect("runtime exit lock");
        exit.is_some()
    }
}

fn read_reports(
    mut reader: UnixStream,
    shared: Arc<Shared>,
    mut on_event: impl FnMut(RuntimeEvent),
) {
    while let Ok(Some(report)) = taco_wire::read_frame::<RuntimeReport>(&mut reader) {
        if let RuntimeReport::Answer { ask, value } = report {
            answer(&shared, ask, Some(value));
            continue;
        }
        on_event(RuntimeEvent::Report(report));
    }

    // The connection is gone, so the runtime is too, or about to be: nothing in flight will be answered.
    let in_flight = shared.asks.lock().expect("runtime asks lock").take();
    if let Some(in_flight) = in_flight {
        for (_, waiting) in in_flight {
            let _ = waiting.send(None);
        }
    }

    let exit = shared.exit.lock().expect("runtime exit lock");
    let exit = shared
        .exited
        .wait_while(exit, |status| status.is_none())
        .expect("runtime exit lock");
    let status = *exit;
    drop(exit);
    if shared.stopping.load(Ordering::SeqCst) {
        return;
    }
    let reason = format!("The terminal runtime stopped ({}).", how_it_ended(status));
    on_event(RuntimeEvent::Died { reason });
}

fn answer(shared: &Shared, ask: u64, value: Option<RuntimeAnswer>) {
    let mut asks = shared.asks.lock().expect("runtime asks lock");
    let Some(waiting) = asks.as_mut() else {
        return;
    };
    if let Some(asker) = waiting.remove(&ask) {
        let _ = asker.send(value);
    }
}

fn how_it_ended(status: Option<ExitStatus>) -> String {
    let Some(status) = status else {
        return "its exit could not be read".to_string();
    };
    if let Some(code) = status.code() {
        return format!("exit code {code}");
    }
    match status.signal() {
        Some(signal) => format!("killed by signal {signal}"),
        None => "its exit could not be read".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in runtime: it takes one byte of what it is sent, then exits with `code`.
    fn stand_in(name: &str, code: i32) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("taco-runtime-client-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        let script = format!("#!/bin/sh\nhead -c 1 <&{CONTROL_FD} >/dev/null\nexit {code}\n");
        std::fs::write(&path, script).unwrap();
        let executable = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, executable).unwrap();
        path
    }

    #[test]
    fn an_ask_in_flight_when_the_runtime_dies_is_answered_with_none() {
        let (events, received) = mpsc::channel();
        let program = stand_in("dies", 7);
        let started = RuntimeClient::start(&program, move |event| {
            let _ = events.send(event);
        })
        .unwrap();

        let pending = started
            .client
            .ask(RuntimeQuery::TerminalForPids { pids: vec![1] });
        let patience = Duration::from_secs(10);
        assert_eq!(pending.recv_timeout(patience).unwrap(), None);
        let after = started
            .client
            .ask(RuntimeQuery::TerminalForPids { pids: vec![1] });
        assert_eq!(
            after.recv_timeout(patience).unwrap(),
            None,
            "an ask after the runtime has gone"
        );

        let event = received.recv_timeout(patience).unwrap();
        let RuntimeEvent::Died { reason } = event else {
            panic!("a report from a stand-in")
        };
        assert_eq!(reason, "The terminal runtime stopped (exit code 7).");

        // A runtime that goes because it was asked to is not reported as dying.
        let (events, received) = mpsc::channel();
        let program = stand_in("stops", 0);
        let started = RuntimeClient::start(&program, move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        started.client.stop();
        // The reader drops `on_event` as it ends, so this returns once it has, with an event only if it sent one.
        assert!(received.recv().is_err());
    }
}
