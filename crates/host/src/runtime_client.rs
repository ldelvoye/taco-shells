//! The host's side of the terminal runtime: starting `taco-shells-runtime`, its control connection, and its exit.

use std::collections::HashMap;
use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
// The locks are taken past a poisoning: a panic another host thread survived must not make every later send, ask or stop panic too.
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, SendTimeoutError, Sender, bounded};
use taco_identity::RUNTIME_PROGRAM;
use taco_protocol::{RuntimeAnswer, RuntimeCommand, RuntimeQuery, RuntimeReport};
use taco_wire::launch::{self, Launch, ShellContext};

/// Where the runtime finds its two connections, as its arguments say.
const CONTROL_FD: RawFd = 3;
const TERMINALS_FD: RawFd = 4;
/// Above both, so moving one connection into place cannot overwrite the other.
const ABOVE_BOTH: RawFd = 10;
/// The stop's steps come to four seconds in all, which the app's quit allows for after its save. A runtime that is not wedged goes within milliseconds of `Stop`.
/// `stalled` is well past any pause of a reading runtime, whose control reader never waits on its core.
const PATIENCE: Patience = Patience {
    send: Duration::from_millis(500),
    exit: Duration::from_millis(1500),
    after_sigterm: Duration::from_millis(1500),
    after_sigkill: Duration::from_millis(500),
    stalled: Duration::from_secs(5),
};
/// Commands sent and not yet written: the workspace sends a few per action, so this many waiting means a runtime that has stopped reading, not a busy one.
const QUEUED_COMMANDS: usize = 4096;
const STALLED: &str = "The terminals stopped responding and were closed.";

/// How long each step of a stop may take, and a send for room in the queue.
#[derive(Clone, Copy)]
struct Patience {
    /// To queue `Stop`.
    send: Duration,
    /// For the runtime to go by itself once it has `Stop`.
    exit: Duration,
    after_sigterm: Duration,
    after_sigkill: Duration,
    /// For room in a full queue, after which the runtime counts as dead: it is killed and reported like one that died.
    stalled: Duration,
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
    /// To the thread that writes them on the control connection, in order.
    commands: Sender<RuntimeCommand>,
    /// Set once a send found the queue full for `Patience::stalled`, and the runtime was killed for it.
    stalled: AtomicBool,
    pid: u32,
    next_ask: AtomicU64,
    /// Asks waiting for their answer. Taken whole, and answered with none, once the runtime is gone.
    asks: Mutex<Option<HashMap<u64, mpsc::Sender<Option<RuntimeAnswer>>>>>,
    stopping: AtomicBool,
    patience: Patience,
    exit: Mutex<Exit>,
    exited: Condvar,
    #[cfg(test)]
    signals_sent: AtomicU64,
}

#[derive(Clone, Copy)]
enum Exit {
    Running,
    Reaped(Option<ExitStatus>),
}

impl RuntimeClient {
    /// `taco-shells-runtime` beside the running executable, in the bundle and in `target/<profile>/` alike.
    pub fn program_beside_this_one() -> io::Result<PathBuf> {
        let this_one = std::env::current_exe()?;
        let Some(directory) = this_one.parent() else {
            let reason = format!("{} has no directory", this_one.display());
            return Err(io::Error::new(io::ErrorKind::NotFound, reason));
        };
        Ok(directory.join(RUNTIME_PROGRAM))
    }

    /// Starts it with two socket pairs at descriptors 3 and 4; `on_event` runs on the client's reader thread, in arrival order.
    pub fn start(
        program: &Path,
        context: &ShellContext,
        on_event: impl FnMut(RuntimeEvent) + Send + 'static,
    ) -> io::Result<RuntimeStart> {
        RuntimeClient::start_with(program, context, on_event, PATIENCE)
    }

    fn start_with(
        program: &Path,
        context: &ShellContext,
        on_event: impl FnMut(RuntimeEvent) + Send + 'static,
        patience: Patience,
    ) -> io::Result<RuntimeStart> {
        let (control, control_end) = UnixStream::pair()?;
        let (terminals, terminals_end) = UnixStream::pair()?;
        let control_fd = control_end.as_raw_fd();
        let terminals_fd = terminals_end.as_raw_fd();

        let given = Launch {
            control: CONTROL_FD,
            terminals: TERMINALS_FD,
            context: context.clone(),
        };
        let arguments = launch::arguments(&given);
        let mut command = Command::new(program);
        command.args(arguments);
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
        let (commands, queued) = bounded(QUEUED_COMMANDS);
        let shared = Arc::new(Shared {
            commands,
            stalled: AtomicBool::new(false),
            pid: child.id(),
            next_ask: AtomicU64::new(0),
            asks: Mutex::new(Some(HashMap::new())),
            stopping: AtomicBool::new(false),
            patience,
            exit: Mutex::new(Exit::Running),
            exited: Condvar::new(),
            #[cfg(test)]
            signals_sent: AtomicU64::new(0),
        });

        let waiter_shared = shared.clone();
        // Reaped only under the exit lock, so a signal sent while holding it never reaches a pid given to another process.
        thread::Builder::new()
            .name("wait for the runtime".into())
            .spawn(move || {
                let waited = wait_without_reaping(waiter_shared.pid);
                let mut exit;
                let status;
                match waited {
                    Ok(()) => {
                        exit = waiter_shared
                            .exit
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        status = child.wait().ok();
                    }
                    Err(error) => {
                        eprintln!("the runtime could not be waited on without reaping it: {error}");
                        status = child.wait().ok();
                        exit = waiter_shared
                            .exit
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                    }
                }
                *exit = Exit::Reaped(status);
                waiter_shared.exited.notify_all();
            })?;

        let reader_shared = shared.clone();
        thread::Builder::new()
            .name("read the runtime".into())
            .spawn(move || {
                read_reports(reader, reader_shared, on_event);
            })?;

        thread::Builder::new()
            .name("write to the runtime".into())
            .spawn(move || write_commands(control, queued))?;

        let client = RuntimeClient { shared };
        Ok(RuntimeStart { client, terminals })
    }

    /// Queued for the writer; it waits only on a full queue, for up to `Patience::stalled`, after which the runtime is ended as stalled and later sends are dropped at once.
    pub fn send(&self, command: RuntimeCommand) {
        if self.shared.stalled.load(Ordering::SeqCst) {
            return;
        }
        let queued = self
            .shared
            .commands
            .send_timeout(command, self.shared.patience.stalled);
        if let Err(SendTimeoutError::Timeout(_)) = queued {
            self.end_stalled();
        }
    }

    fn end_stalled(&self) {
        let already = self.shared.stalled.swap(true, Ordering::SeqCst);
        if !already {
            self.signal(libc::SIGKILL);
        }
    }

    /// Receives exactly one value: the answer, or `None` once the runtime is gone, including for asks still in flight when it died.
    pub fn ask(&self, query: RuntimeQuery) -> mpsc::Receiver<Option<RuntimeAnswer>> {
        let (answer, answered) = mpsc::channel();
        let ask = self.shared.next_ask.fetch_add(1, Ordering::SeqCst);
        {
            let mut asks = self
                .shared
                .asks
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
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

    /// Whether `Stop` was queued in time. A runtime that does not read it is signalled once the exit step runs out.
    fn send_stop(&self, patience: Duration) -> bool {
        let queued = self
            .shared
            .commands
            .send_timeout(RuntimeCommand::Stop, patience);
        queued.is_ok()
    }

    fn signal(&self, signal: libc::c_int) {
        let exit = self
            .shared
            .exit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !exit.running() {
            return;
        }
        unsafe { libc::kill(self.shared.pid as libc::pid_t, signal) };
        #[cfg(test)]
        self.shared.signals_sent.fetch_add(1, Ordering::SeqCst);
    }

    /// Whether the runtime has exited, waiting up to `patience` for it.
    fn wait_for_exit(&self, patience: Duration) -> bool {
        let exit = self
            .shared
            .exit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (exit, _) = self
            .shared
            .exited
            .wait_timeout_while(exit, patience, |exit| exit.running())
            .unwrap_or_else(PoisonError::into_inner);
        !exit.running()
    }
}

impl Exit {
    fn running(&self) -> bool {
        matches!(self, Exit::Running)
    }

    fn status(&self) -> Option<ExitStatus> {
        match self {
            Exit::Running => None,
            Exit::Reaped(status) => *status,
        }
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
    let in_flight = shared
        .asks
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(in_flight) = in_flight {
        for (_, waiting) in in_flight {
            let _ = waiting.send(None);
        }
    }

    let exit = shared.exit.lock().unwrap_or_else(PoisonError::into_inner);
    let exit = shared
        .exited
        .wait_while(exit, |exit| exit.running())
        .unwrap_or_else(PoisonError::into_inner);
    let status = exit.status();
    drop(exit);
    if shared.stopping.load(Ordering::SeqCst) {
        return;
    }
    if shared.stalled.load(Ordering::SeqCst) {
        let reason = STALLED.to_string();
        on_event(RuntimeEvent::Died { reason });
        return;
    }
    let reason = format!("The terminal runtime stopped ({}).", how_it_ended(status));
    on_event(RuntimeEvent::Died { reason });
}

/// Writes each command queued by `send`, in order, until the runtime's connection fails. A command too large for a frame is left out, since nothing of it was written.
fn write_commands(mut control: UnixStream, queued: Receiver<RuntimeCommand>) {
    for command in queued {
        let written = taco_wire::write_frame(&mut control, &command);
        let Err(error) = written else {
            continue;
        };
        if error.kind() != io::ErrorKind::InvalidInput {
            return;
        }
        eprintln!("a command for the runtime was not sent: {error}");
    }
}

fn wait_without_reaping(pid: u32) -> io::Result<()> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let options = libc::WEXITED | libc::WNOWAIT;
    loop {
        let waited = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, options) };
        if waited == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn answer(shared: &Shared, ask: u64, value: Option<RuntimeAnswer>) {
    let mut asks = shared.asks.lock().unwrap_or_else(PoisonError::into_inner);
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
    use crossbeam_channel::TrySendError;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    /// A stand-in runtime. It writes whether SIGTERM is blocked in it to `<itself>.mask`, takes one byte of what it is sent, and exits with `code`.
    fn stand_in(name: &str, code: i32) -> StandIn {
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

    /// A stand-in's script, in a folder of its own that is removed with it.
    struct StandIn {
        directory: PathBuf,
        path: PathBuf,
    }

    impl Drop for StandIn {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn program(name: &str, script: &str) -> StandIn {
        let folder = format!("taco-runtime-client-{}-{name}", std::process::id());
        let directory = std::env::temp_dir().join(folder);
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(name);
        std::fs::write(&path, script).unwrap();
        let executable = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, executable).unwrap();
        StandIn { directory, path }
    }

    /// Short enough to keep the tests quick, long enough for a killed stand-in to be reaped.
    const QUICK: Patience = Patience {
        send: Duration::from_millis(300),
        exit: Duration::from_millis(300),
        after_sigterm: Duration::from_millis(300),
        after_sigkill: Duration::from_millis(1000),
        // Past what the stop tests take, so the stall never ends a runtime they mean to stop.
        stalled: Duration::from_secs(30),
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
            list_entries: Vec::new(),
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
        client.shared.exit.lock().unwrap().status()
    }

    fn exited_unreaped(pid: u32) -> bool {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let options = libc::WEXITED | libc::WNOWAIT | libc::WNOHANG;
        let waited = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, options) };
        let exited = unsafe { info.si_pid() };
        waited == 0 && exited == pid as libc::pid_t
    }

    #[test]
    fn a_stop_ends_a_runtime_that_reads_nothing_while_a_send_waits_for_room() {
        let program = program("reads-nothing", "#!/bin/sh\nexec sleep 999\n");
        let started = started_quick(&program.path);
        let sender = started.client.clone();
        let sent = Arc::new(AtomicU64::new(0));
        let counted = Arc::clone(&sent);
        thread::spawn(move || {
            for lines in 0..100_000 {
                sender.send(RuntimeCommand::Scrollback { lines });
                counted.fetch_add(1, Ordering::SeqCst);
            }
        });
        // Seen stuck: past the queue's size, and no further over a while.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(
                Instant::now() < deadline,
                "the sender never waited for room"
            );
            let before = sent.load(Ordering::SeqCst);
            thread::sleep(Duration::from_millis(50));
            let after = sent.load(Ordering::SeqCst);
            if after == before && after >= QUEUED_COMMANDS as u64 && after < 100_000 {
                break;
            }
        }
        let exit = stopped_in_time(&started.client);
        assert!(exit.is_some(), "the runtime is gone");
    }

    #[test]
    fn a_runtime_that_stops_reading_is_ended_and_reported_once_one_send_has_waited() {
        let program = program("stops-reading", "#!/bin/sh\nexec sleep 999\n");
        let (events, received) = mpsc::channel();
        let patience = Patience {
            stalled: Duration::from_millis(300),
            ..QUICK
        };
        let report = move |event| {
            let _ = events.send(event);
        };
        let started =
            RuntimeClient::start_with(&program.path, &test_context(), report, patience).unwrap();

        // Far more than the queue and the connection hold together.
        let began = Instant::now();
        for lines in 0..100_000 {
            started.client.send(RuntimeCommand::Scrollback { lines });
        }
        let took = began.elapsed();
        assert!(
            took >= patience.stalled,
            "a full queue is waited on first: {took:?}"
        );
        let bound = patience.stalled + SCHEDULING;
        assert!(took <= bound, "one send waits, and none after it: {took:?}");

        let event = received.recv_timeout(Duration::from_secs(10)).unwrap();
        let RuntimeEvent::Died { reason } = event else {
            panic!("the runtime is reported as dead");
        };
        assert_eq!(reason, STALLED);
        let exit = started.client.shared.exit.lock().unwrap().status();
        let killed_by = exit.and_then(|status| status.signal());
        assert_eq!(killed_by, Some(libc::SIGKILL));
    }

    #[test]
    fn a_stop_kills_a_runtime_that_ignores_sigterm() {
        let program = program(
            "ignores-sigterm",
            "#!/usr/bin/perl\n$SIG{TERM} = 'IGNORE';\nopen(my $ready, '>', \"$0.ready\");\nclose $ready;\nsleep 999;\n",
        );
        let ready = program.path.with_extension("ready");
        let started = started_quick(&program.path);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            assert!(Instant::now() < deadline, "the stand-in never got ready");
            thread::sleep(Duration::from_millis(10));
        }
        let exit = stopped_in_time(&started.client).expect("the runtime is gone");
        assert_eq!(exit.signal(), Some(libc::SIGKILL));
    }

    #[test]
    fn a_runtime_that_dies_is_reported_with_its_exit_and_leaves_no_ask_waiting() {
        let (events, received) = mpsc::channel();
        let program = stand_in("dies", 7);
        let started = RuntimeClient::start(&program.path, &test_context(), move |event| {
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
    }

    #[test]
    fn the_runtime_starts_with_no_signal_blocked_whatever_its_starter_blocks() {
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGTERM);
            libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        }
        let program = stand_in("masked", 0);
        let started = RuntimeClient::start(&program.path, &test_context(), |_| {}).unwrap();
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };

        // Its report written before it is stopped, however slow the machine.
        let mask_report = program.path.with_extension("mask");
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut sigterm_blocked = String::new();
        while sigterm_blocked.is_empty() {
            assert!(
                Instant::now() < deadline,
                "the stand-in never wrote its mask"
            );
            thread::sleep(Duration::from_millis(10));
            let read = std::fs::read_to_string(&mask_report);
            if let Ok(text) = read {
                sigterm_blocked = text;
            }
        }
        stopped_in_time(&started.client);
        assert_eq!(sigterm_blocked, "0", "SIGTERM blocked in the runtime");
    }

    #[test]
    fn a_runtime_stopped_on_request_is_not_reported_as_dying() {
        let (events, received) = mpsc::channel();
        let program = stand_in("stops", 0);
        let started = RuntimeClient::start(&program.path, &test_context(), move |event| {
            let _ = events.send(event);
        })
        .unwrap();
        started.client.stop();
        // The reader drops `on_event` as it ends, so this returns once it has, with an event only if it sent one.
        assert!(received.recv().is_err());
    }

    #[test]
    fn a_runtime_is_reaped_only_while_nothing_can_signal_it_and_never_signalled_after() {
        let program = stand_in("reaped-under-the-lock", 5);
        let started = started_quick(&program.path);
        let client = &started.client;
        let pid = client.shared.pid;

        let held = client.shared.exit.lock().unwrap();
        client.send(RuntimeCommand::Scrollback { lines: 1 });
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited_unreaped(pid) {
            assert!(
                Instant::now() < deadline,
                "the stand-in was never seen exited and still unreaped"
            );
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(200));
        let while_held = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(while_held, 0, "the exited runtime is still ours to signal");
        assert!(held.running(), "no reap is recorded while the lock is held");
        drop(held);

        let reaped = client.wait_for_exit(Duration::from_secs(2));
        assert!(reaped, "the reap is recorded once the lock is free");
        let after_reap = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(after_reap, -1, "the reaped pid is no longer ours");

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let queued = client
                .shared
                .commands
                .try_send(RuntimeCommand::Scrollback { lines: 1 });
            if let Err(TrySendError::Disconnected(_)) = queued {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the writer never gave up on the gone runtime, so a stop would not reach its signals"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let before = client.shared.signals_sent.load(Ordering::SeqCst);
        client.stop();
        client.end_stalled();
        let after = client.shared.signals_sent.load(Ordering::SeqCst);
        assert_eq!(after, before, "a signal was sent after the reap");
    }
}
