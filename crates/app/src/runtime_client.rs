//! The app's side of the terminal runtime: starting `taco-shells-runtime`, its control connection, and its exit.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use taco_protocol::{RuntimeAnswer, RuntimeCommand, RuntimeQuery, RuntimeReport};

const PROGRAM: &str = "taco-shells-runtime";
/// Where the runtime finds its two connections, as its arguments say.
const CONTROL_FD: RawFd = 3;
const TERMINALS_FD: RawFd = 4;
/// Above both, so moving one connection into place cannot overwrite the other.
const ABOVE_BOTH: RawFd = 10;
/// Four seconds in all, which the app's quit allows for after its save. A runtime that is not wedged goes within milliseconds of `Stop`.
const STOP_PATIENCE: StopPatience = StopPatience {
    send: Duration::from_millis(500),
    exit: Duration::from_millis(1500),
    after_sigterm: Duration::from_millis(1500),
    after_sigkill: Duration::from_millis(500),
};
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// How long each step of a stop may take. A stop takes at most their sum.
#[derive(Clone, Copy)]
struct StopPatience {
    /// To take the connection and write `Stop`, both within this one deadline.
    send: Duration,
    /// For the runtime to go by itself once it has `Stop`.
    exit: Duration,
    after_sigterm: Duration,
    after_sigkill: Duration,
}

/// What the runtime tells every shell about this app.
pub struct ShellContext {
    /// `taco-shells` or `taco-shells-dev`.
    pub app: String,
    /// This app's `focus-port` file.
    pub port_file: PathBuf,
}

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
    patience: StopPatience,
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
        context: &ShellContext,
        on_event: impl FnMut(RuntimeEvent) + Send + 'static,
    ) -> io::Result<RuntimeStart> {
        RuntimeClient::start_with(program, context, on_event, STOP_PATIENCE)
    }

    fn start_with(
        program: &Path,
        context: &ShellContext,
        on_event: impl FnMut(RuntimeEvent) + Send + 'static,
        patience: StopPatience,
    ) -> io::Result<RuntimeStart> {
        let (control, control_end) = UnixStream::pair()?;
        let (terminals, terminals_end) = UnixStream::pair()?;
        let control_fd = control_end.as_raw_fd();
        let terminals_fd = terminals_end.as_raw_fd();

        let mut port_file_argument = OsString::from("--port-file=");
        port_file_argument.push(&context.port_file);
        let mut command = Command::new(program);
        command.arg(format!("--control-fd={CONTROL_FD}"));
        command.arg(format!("--terminals-fd={TERMINALS_FD}"));
        command.arg(format!("--app={}", context.app));
        command.arg(port_file_argument);
        command.stdin(Stdio::null());
        // Between fork and exec only async-signal-safe calls. The copies at 3 and 4 stay open across exec; the ones above close.
        unsafe {
            command.pre_exec(move || {
                // A mask survives exec, and std does not reset it on this path: the runtime starts with none blocked.
                let mut nothing_blocked: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut nothing_blocked);
                libc::pthread_sigmask(libc::SIG_SETMASK, &nothing_blocked, std::ptr::null_mut());
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
            patience,
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

    /// Sends `Stop`, then SIGTERM, then SIGKILL, until the runtime has gone; one that cannot be sent `Stop` goes straight to SIGTERM.
    /// Four seconds at worst. A stop never produces `RuntimeEvent::Died`.
    pub fn stop(&self) {
        let patience = self.shared.patience;
        self.shared.stopping.store(true, Ordering::SeqCst);
        let stop_sent = self.send_stop(patience.send);
        if stop_sent && self.wait_for_exit(patience.exit) {
            return;
        }
        self.signal(libc::SIGTERM);
        if self.wait_for_exit(patience.after_sigterm) {
            return;
        }
        self.signal(libc::SIGKILL);
        self.wait_for_exit(patience.after_sigkill);
    }

    /// Whether `Stop` went out in time. A send stuck on a full connection holds it, so a stop does not wait its turn.
    fn send_stop(&self, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        let Some(mut control) = self.control_by(deadline) else {
            return false;
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        if control.set_write_timeout(Some(left)).is_err() {
            return false;
        }
        taco_wire::write_frame(&mut *control, &RuntimeCommand::Stop).is_ok()
    }

    fn control_by(&self, deadline: Instant) -> Option<MutexGuard<'_, UnixStream>> {
        loop {
            match self.shared.control.try_lock() {
                Ok(control) => return Some(control),
                Err(TryLockError::Poisoned(poisoned)) => return Some(poisoned.into_inner()),
                Err(TryLockError::WouldBlock) => {}
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(LOCK_RETRY);
        }
    }

    fn signal(&self, signal: libc::c_int) {
        unsafe { libc::kill(self.shared.pid as libc::pid_t, signal) };
    }

    /// Whether the runtime has exited, waiting up to `patience` for it.
    fn wait_for_exit(&self, patience: Duration) -> bool {
        let exit = self.shared.exit.lock().expect("runtime exit lock");
        let (exit, _) = self
            .shared
            .exited
            .wait_timeout_while(exit, patience, |status| status.is_none())
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

    /// A stand-in runtime. It writes whether SIGTERM is blocked in it to `<itself>.mask`, takes one byte of what it is sent, and exits with `code`.
    fn stand_in(name: &str, code: i32) -> PathBuf {
        let script = format!(
            r#"#!/usr/bin/perl
use POSIX;
my $old = POSIX::SigSet->new;
sigprocmask(SIG_BLOCK, POSIX::SigSet->new, $old);
open(my $mask, '>', "$0.mask") or die;
print $mask ($old->ismember(SIGTERM) ? 1 : 0);
close $mask;
open(my $control, '<&=', {CONTROL_FD}) or die;
read($control, my $byte, 1);
exit {code};
"#
        );
        program(name, &script)
    }

    fn program(name: &str, script: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("taco-runtime-client-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, script).unwrap();
        let executable = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, executable).unwrap();
        path
    }

    /// Short enough to keep the tests quick, long enough for a killed stand-in to be reaped.
    const QUICK: StopPatience = StopPatience {
        send: Duration::from_millis(300),
        exit: Duration::from_millis(300),
        after_sigterm: Duration::from_millis(300),
        after_sigkill: Duration::from_millis(1000),
    };
    /// What a busy machine may add to the steps' own waits.
    const SCHEDULING: Duration = Duration::from_millis(300);

    fn started_quick(program: &Path) -> RuntimeStart {
        RuntimeClient::start_with(program, &test_context(), |_| {}, QUICK).unwrap()
    }

    fn test_context() -> ShellContext {
        ShellContext {
            app: "taco-shells-test".into(),
            port_file: PathBuf::from("/nonexistent/focus-port"),
        }
    }

    /// Stops it on a thread of its own, so a stop that hangs fails the test instead of hanging it, and holds it to the sum of its steps.
    fn stopped_in_time(client: &RuntimeClient) -> Option<ExitStatus> {
        let bound = QUICK.send + QUICK.exit + QUICK.after_sigterm + QUICK.after_sigkill;
        let began = Instant::now();
        let (done, stopped) = mpsc::channel();
        let stopping = client.clone();
        thread::spawn(move || {
            stopping.stop();
            let _ = done.send(());
        });
        stopped
            .recv_timeout(bound + SCHEDULING)
            .expect("the stop returned within the sum of its steps");
        let took = began.elapsed();
        assert!(took <= bound + SCHEDULING, "the stop took {took:?}");
        *client.shared.exit.lock().unwrap()
    }

    #[test]
    fn a_stop_ends_a_runtime_that_reads_nothing_while_a_send_is_stuck() {
        let program = program("reads-nothing", "#!/bin/sh\nexec sleep 999\n");
        let started = started_quick(&program);
        let sender = started.client.clone();
        thread::spawn(move || {
            for lines in 0..100_000 {
                sender.send(RuntimeCommand::Scrollback { lines });
            }
        });
        // Long enough for the connection to fill and the sender to stick, holding it.
        thread::sleep(Duration::from_millis(200));
        let exit = stopped_in_time(&started.client);
        assert!(exit.is_some(), "the runtime is gone");
    }

    #[test]
    fn a_stop_ends_a_runtime_that_reads_nothing_once_its_connection_is_full() {
        let program = program("reads-nothing-full", "#!/bin/sh\nexec sleep 999\n");
        let started = started_quick(&program);
        {
            let mut control = started.client.shared.control.lock().unwrap();
            control.set_nonblocking(true).unwrap();
            let filler = [0u8; 4096];
            while io::Write::write(&mut *control, &filler).is_ok() {}
            control.set_nonblocking(false).unwrap();
        }
        let exit = stopped_in_time(&started.client);
        assert!(exit.is_some(), "the runtime is gone");
    }

    #[test]
    fn a_stop_kills_a_runtime_that_ignores_sigterm() {
        let program = program(
            "ignores-sigterm",
            "#!/usr/bin/perl\n$SIG{TERM} = 'IGNORE';\nopen(my $ready, '>', \"$0.ready\");\nclose $ready;\nsleep 999;\n",
        );
        let ready = program.with_extension("ready");
        let _ = std::fs::remove_file(&ready);
        let started = started_quick(&program);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(Instant::now() < deadline, "the stand-in never got ready");
            thread::sleep(Duration::from_millis(10));
        }
        let exit = stopped_in_time(&started.client).expect("the runtime is gone");
        assert_eq!(exit.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn an_ask_in_flight_when_the_runtime_dies_is_answered_with_none() {
        // Whatever the starting thread blocks, the runtime starts with nothing blocked.
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGTERM);
            libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        }
        let (events, received) = mpsc::channel();
        let program = stand_in("dies", 7);
        let started = RuntimeClient::start(&program, &test_context(), move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };

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
        let mask_report = program.with_extension("mask");
        let sigterm_blocked = std::fs::read_to_string(mask_report).unwrap();
        assert_eq!(sigterm_blocked, "0", "SIGTERM blocked in the runtime");

        // A runtime that goes because it was asked to is not reported as dying.
        let (events, received) = mpsc::channel();
        let program = stand_in("stops", 0);
        let started = RuntimeClient::start(&program, &test_context(), move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        started.client.stop();
        // The reader drops `on_event` as it ends, so this returns once it has, with an event only if it sent one.
        assert!(received.recv().is_err());
    }
}
