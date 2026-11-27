mod environment;
mod reader;

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use taco_protocol::{TerminalId, TerminalSize};
use taco_wire::launch::ShellContext;

use crate::budget::{Budget, Budgets};
use crate::core::Input;
use crate::platform;
use crate::shells::Shells;
use reader::Reader;

const FALLBACK_SHELL: &str = "/bin/zsh";
/// A process ended by a signal reports 128 plus the signal, as shells do.
const SIGNAL_EXIT_BASE: i32 = 128;
/// What an exit reports when its status could not be read.
const UNKNOWN_EXIT_CODE: i32 = 1;

#[cfg(test)]
thread_local! {
    static HANG_UPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// The program each terminal runs, and where it starts when no directory is given or the one given is not a directory.
pub struct ShellProgram {
    pub path: String,
    pub arguments: Vec<String>,
    pub home: String,
    /// The open-file limit the runtime was started with, given back to each shell when the runtime raised its own.
    pub open_file_limit: Option<libc::rlimit>,
    pub context: ShellContext,
}

struct Shell {
    pid: u32,
    /// Set once the shell has been waited for, which happens only under this lock: a hang-up holding it never reaches a pid given to another process.
    reaped: Arc<Mutex<bool>>,
    master: OwnedFd,
    writes: Sender<Vec<u8>>,
}

/// What a shell's threads take over besides their own copies of the master: the reader's copy of the slave, and the wake pipe from the waiter to the reader.
struct ThreadEnds {
    shell_side: OwnedFd,
    wake_reader: OwnedFd,
    wake_writer: OwnedFd,
}

/// The terminals' shells, each in a pty of its own, with a reader, a writer and a waiter thread.
pub struct PtyShells {
    program: ShellProgram,
    inputs: Sender<Input>,
    budgets: Arc<Budgets>,
    shells: HashMap<TerminalId, Shell>,
    agent_plugin: bool,
}

impl ShellProgram {
    /// The user's login shell, the way macOS terminals start one. Before any thread starts: `getpwuid` is not thread-safe.
    pub fn login(context: ShellContext) -> ShellProgram {
        let user = unsafe { libc::getuid() };
        let record = unsafe { libc::getpwuid(user) };
        let mut path = String::new();
        let mut home = String::new();
        if !record.is_null() {
            let shell = unsafe { CStr::from_ptr((*record).pw_shell) };
            let directory = unsafe { CStr::from_ptr((*record).pw_dir) };
            path = shell.to_string_lossy().into_owned();
            home = directory.to_string_lossy().into_owned();
        }
        if path.is_empty()
            && let Ok(from_environment) = std::env::var("SHELL")
        {
            path = from_environment;
        }
        if path.is_empty() {
            path = FALLBACK_SHELL.to_string();
        }
        if home.is_empty()
            && let Ok(from_environment) = std::env::var("HOME")
        {
            home = from_environment;
        }
        if home.is_empty() {
            home = "/".to_string();
        }
        ShellProgram {
            path,
            arguments: vec!["-l".to_string()],
            home,
            open_file_limit: None,
            context,
        }
    }
}

impl PtyShells {
    pub fn new(program: ShellProgram, inputs: Sender<Input>, budgets: Arc<Budgets>) -> PtyShells {
        PtyShells {
            program,
            inputs,
            budgets,
            shells: HashMap::new(),
            agent_plugin: false,
        }
    }

    /// Drops what is left of a shell whose exit the core has handled.
    pub fn forget(&mut self, id: &TerminalId) {
        self.shells.remove(id);
        self.budgets.forget(id);
    }

    /// Ends every shell, for when the runtime itself is stopping.
    pub fn stop_all(&mut self) {
        for shell in self.shells.values() {
            hang_up(shell.pid, &shell.reaped);
        }
    }

    fn spawn(&mut self, id: &TerminalId, directory: &str, size: TerminalSize) -> io::Result<Shell> {
        let (master, slave) = open_pty(size)?;
        let mut command = Command::new(&self.program.path);
        command.args(&self.program.arguments);
        command.current_dir(directory);
        environment::apply(&mut command, &self.program.context, self.agent_plugin);
        let shell_side = slave.try_clone()?;
        command.stdin(Stdio::from(slave.try_clone()?));
        command.stdout(Stdio::from(slave.try_clone()?));
        command.stderr(Stdio::from(slave));
        // Between fork and exec only async-signal-safe calls: a session of its own, with the pty as its controlling terminal.
        unsafe {
            let open_file_limit = self.program.open_file_limit;
            command.pre_exec(move || {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                // Fd 0 is the pty's slave by now.
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                // The runtime blocks SIGTERM, and a mask survives exec: without this, no one could SIGTERM what they run.
                let mut nothing_blocked: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut nothing_blocked);
                libc::pthread_sigmask(libc::SIG_SETMASK, &nothing_blocked, std::ptr::null_mut());
                // The shell gets the limit it would have had in any other terminal.
                if let Some(limit) = open_file_limit {
                    libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
                }
                Ok(())
            });
        }
        let (wake_reader, wake_writer) = wake_pipe()?;
        let child = command.spawn()?;
        // Closes the copies the shell was given; the reader keeps its own until the shell's last output is read.
        drop(command);
        let pid = child.id();

        let budget = self.budgets.open(id);
        let reaped = Arc::new(Mutex::new(false));
        let ends = ThreadEnds {
            shell_side,
            wake_reader,
            wake_writer,
        };
        let started = self.start_threads(id, child, &master, budget, &reaped, ends);
        match started {
            Ok(writes) => Ok(Shell {
                pid,
                reaped,
                master,
                writes,
            }),
            Err(error) => {
                // Nothing would ever report this shell's exit, so it does not get to run.
                hang_up(pid, &reaped);
                self.budgets.forget(id);
                Err(error)
            }
        }
    }

    /// The waiter starts first, so a shell that has started is always waited for.
    fn start_threads(
        &self,
        id: &TerminalId,
        mut child: std::process::Child,
        master: &OwnedFd,
        budget: Arc<Budget>,
        reaped: &Arc<Mutex<bool>>,
        ends: ThreadEnds,
    ) -> io::Result<Sender<Vec<u8>>> {
        let exit_code = Arc::new(Mutex::new(None));
        let waiter_budget = budget.clone();
        let waiter_exit_code = exit_code.clone();
        let waiter_reaped = reaped.clone();
        let wake_writer = ends.wake_writer;
        thread::Builder::new()
            .name(format!("wait {}", id.0))
            .spawn(move || {
                let waited = wait_without_reaping(child.id());
                let mut reaped;
                let status;
                match waited {
                    Ok(()) => {
                        reaped = waiter_reaped
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        status = child.wait();
                    }
                    Err(error) => {
                        eprintln!("a shell could not be waited on without reaping it: {error}");
                        status = child.wait();
                        reaped = waiter_reaped
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                    }
                }
                let code = match status {
                    Ok(status) => exit_code_of(status),
                    Err(_) => UNKNOWN_EXIT_CODE,
                };
                *reaped = true;
                drop(reaped);
                *waiter_exit_code.lock().expect("exit code lock") = Some(code);
                // The reader may be waiting on its budget rather than polling, so both are woken.
                waiter_budget.close();
                let mut wake = File::from(wake_writer);
                let _ = wake.write_all(b"x");
            })?;

        let reader_master = master.try_clone()?;
        let reader = Reader {
            id: id.clone(),
            master: File::from(reader_master),
            shell_side: ends.shell_side,
            wake: ends.wake_reader,
            exit_code,
            budget,
            inputs: self.inputs.clone(),
        };
        thread::Builder::new()
            .name(format!("read {}", id.0))
            .spawn(move || reader.run())?;

        let (writes, queued) = mpsc::channel::<Vec<u8>>();
        let mut writer_master = File::from(master.try_clone()?);
        thread::Builder::new()
            .name(format!("write {}", id.0))
            .spawn(move || {
                // A shell that is not reading blocks only this thread, never the core.
                for data in queued {
                    if writer_master.write_all(&data).is_err() {
                        break;
                    }
                }
            })?;

        Ok(writes)
    }
}

impl Shells for PtyShells {
    fn start(
        &mut self,
        id: &TerminalId,
        directory: Option<&str>,
        size: TerminalSize,
    ) -> Option<String> {
        let mut usable = self.program.home.clone();
        if let Some(directory) = directory
            && Path::new(directory).is_dir()
        {
            usable = directory.to_string();
        }
        // The path the kernel will report for it (`/tmp` is `/private/tmp`), so the first read is not taken for a change.
        if let Ok(resolved) = std::fs::canonicalize(&usable) {
            usable = resolved.to_string_lossy().into_owned();
        }
        match self.spawn(id, &usable, size) {
            Ok(shell) => {
                self.shells.insert(id.clone(), shell);
                Some(usable)
            }
            Err(error) => {
                eprintln!("the shell for {} could not start: {error}", id.0);
                None
            }
        }
    }

    fn write(&mut self, id: &TerminalId, data: &[u8]) {
        if let Some(shell) = self.shells.get(id) {
            let _ = shell.writes.send(data.to_vec());
        }
    }

    fn resize(&mut self, id: &TerminalId, size: TerminalSize) {
        if let Some(shell) = self.shells.get(id) {
            let window = window_size(size);
            unsafe { libc::ioctl(shell.master.as_raw_fd(), libc::TIOCSWINSZ, &window) };
        }
    }

    fn close(&mut self, id: &TerminalId) {
        if let Some(shell) = self.shells.get(id) {
            hang_up(shell.pid, &shell.reaped);
        }
    }

    fn directory(&self, id: &TerminalId) -> Option<String> {
        let shell = self.shells.get(id)?;
        platform::directory_of(shell.pid)
    }

    fn at_prompt(&self, id: &TerminalId) -> bool {
        let Some(shell) = self.shells.get(id) else {
            return false;
        };
        let foreground = unsafe { libc::tcgetpgrp(shell.master.as_raw_fd()) };
        foreground == shell.pid as libc::pid_t
    }

    fn foreground_commands(&self, id: &TerminalId) -> Vec<String> {
        let Some(shell) = self.shells.get(id) else {
            return Vec::new();
        };
        let foreground = unsafe { libc::tcgetpgrp(shell.master.as_raw_fd()) };
        if foreground <= 0 {
            return Vec::new();
        }
        platform::group_commands(foreground as u32)
    }

    fn shell_pid(&self, id: &TerminalId) -> Option<u32> {
        let shell = self.shells.get(id)?;
        Some(shell.pid)
    }

    fn set_agent_plugin(&mut self, on: bool) {
        self.agent_plugin = on;
    }
}

fn open_pty(size: TerminalSize) -> io::Result<(OwnedFd, OwnedFd)> {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let mut window = window_size(size);
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut window,
        )
    };
    if opened < 0 {
        return Err(io::Error::last_os_error());
    }
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    let slave = unsafe { OwnedFd::from_raw_fd(slave) };
    // Without this, every later shell would inherit this terminal's pty and keep it open after its own shell has gone.
    // It comes after creation, so only one thread may open ptys and start shells: a fork from another thread in between would pass these on.
    set_close_on_exec(master.as_raw_fd());
    set_close_on_exec(slave.as_raw_fd());
    Ok((master, slave))
}

fn wake_pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut ends = [0 as RawFd; 2];
    let made = unsafe { libc::pipe(ends.as_mut_ptr()) };
    if made < 0 {
        return Err(io::Error::last_os_error());
    }
    let reader = unsafe { OwnedFd::from_raw_fd(ends[0]) };
    let writer = unsafe { OwnedFd::from_raw_fd(ends[1]) };
    set_close_on_exec(reader.as_raw_fd());
    set_close_on_exec(writer.as_raw_fd());
    Ok((reader, writer))
}

fn window_size(size: TerminalSize) -> libc::winsize {
    libc::winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

fn set_close_on_exec(fd: RawFd) {
    unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
}

/// SIGHUP to the shell's process group, as a terminal closing does; the shell leads its group.
fn hang_up(pid: u32, reaped: &Mutex<bool>) {
    let reaped = reaped.lock().unwrap_or_else(PoisonError::into_inner);
    if *reaped {
        return;
    }
    let group = pid as libc::pid_t;
    unsafe { libc::killpg(group, libc::SIGHUP) };
    #[cfg(test)]
    {
        let sent = HANG_UPS.get();
        HANG_UPS.set(sent + 1);
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

fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    match status.signal() {
        Some(signal) => SIGNAL_EXIT_BASE + signal,
        None => UNKNOWN_EXIT_CODE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };
    /// How much of what a shell printed a failure shows.
    const TAIL_CHARS: usize = 80;
    /// Only a safety net, for each wait as a whole: every wait below ends on output the shell prints or on its exit.
    const PATIENCE: Duration = Duration::from_secs(5);
    /// Prints whether SIGTERM is blocked, and how many devices and sockets the process holds besides its own terminal on 0 to 2.
    const REPORT_INHERITED: &str = r#"my $old = POSIX::SigSet->new; sigprocmask(SIG_BLOCK, POSIX::SigSet->new, $old); my $devices = grep { my $fd; open($fd, "<&=", $_) && -c $fd } 3..255; print "sigterm blocked: ", ($old->ismember(SIGTERM) ? 1 : 0), ", other devices: $devices\n";"#;

    /// The runtime starts shells from one thread. Tests starting them on several at once could pass a pty one has just opened, before its close-on-exec is set, to a shell another starts.
    static STARTING_SHELLS: Mutex<()> = Mutex::new(());

    fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
        let locked = STARTING_SHELLS.lock();
        locked.unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn test_context() -> ShellContext {
        ShellContext {
            app: "taco-shells-test".into(),
            port_file: "/nonexistent/focus-port".into(),
            plugin_directory: "/nonexistent/claude-plugin".into(),
        }
    }

    fn perl_shells(script: String, received_by: Sender<Input>, budgets: Arc<Budgets>) -> PtyShells {
        let program = ShellProgram {
            path: "/usr/bin/perl".into(),
            arguments: vec!["-MPOSIX".into(), "-e".into(), script],
            home: "/".into(),
            open_file_limit: None,
            context: test_context(),
        };
        PtyShells::new(program, received_by, budgets)
    }

    /// What the shell printed until `ready` holds of it, or its exit code if it exited first.
    fn read_until(
        received: &Receiver<Input>,
        watched: &TerminalId,
        ready: impl Fn(&str) -> bool,
    ) -> Result<String, i32> {
        let mut seen = String::new();
        let deadline = Instant::now() + PATIENCE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let input = received.recv_timeout(left);
            let Ok(input) = input else {
                panic!("nothing more from {} after {}", watched.0, described(&seen));
            };
            match input {
                Input::Output { id, data } if id == *watched => {
                    let text = String::from_utf8_lossy(&data);
                    seen.push_str(&text);
                    if ready(&seen) {
                        return Ok(seen);
                    }
                }
                Input::Exited { id, code } if id == *watched => {
                    // Shown with a failure when the exit was not what the test waited for.
                    eprintln!(
                        "{} exited with {code} after {}",
                        watched.0,
                        described(&seen)
                    );
                    return Err(code);
                }
                Input::Output { .. } | Input::Exited { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    /// What a shell printed, for a failure: its length and how it ends.
    fn described(seen: &str) -> String {
        let reversed_tail: String = seen.chars().rev().take(TAIL_CHARS).collect();
        let tail: String = reversed_tail.chars().rev().collect();
        format!("{} bytes ending {tail:?}", seen.len())
    }

    #[test]
    fn a_shell_starts_where_asked_with_its_limit_and_follows_its_cd_size_program_and_exit() {
        let _alone = one_at_a_time();
        let scratch = std::env::temp_dir().join(format!("taco-runtime-pty-{}", std::process::id()));
        let inner = scratch.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let start_in = scratch
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let inner_path = inner.canonicalize().unwrap().to_string_lossy().into_owned();

        let (inputs, received) = mpsc::channel();
        // zsh with no startup files: a real shell binary (macOS's /bin/sh is a launcher), quick to start and the same everywhere.
        let program = ShellProgram {
            path: "/bin/zsh".into(),
            arguments: vec!["-f".into()],
            home: "/".into(),
            // As if the runtime had raised its own limit above this one.
            open_file_limit: Some(libc::rlimit {
                rlim_cur: 200,
                rlim_max: libc::RLIM_INFINITY,
            }),
            context: test_context(),
        };
        let mut shells = PtyShells::new(program, inputs, Arc::new(Budgets::default()));
        let id = TerminalId("pty".into());

        let started = shells.start(&id, Some(&start_in), SIZE);
        assert_eq!(started.as_deref(), Some(start_in.as_str()));
        let missing = TerminalId("missing".into());
        let fell_back = shells.start(&missing, Some("/no/such/directory"), SIZE);
        assert_eq!(fell_back.as_deref(), Some("/"));
        // Closing hangs the shell up, and a shell ended by a signal reports 128 plus it.
        shells.close(&missing);
        let hung_up = read_until(&received, &missing, |_| false);
        assert_eq!(hung_up, Err(128 + libc::SIGHUP));

        // The line as typed echoes `th""ere`; only running it prints `there`.
        shells.write(&id, b"cd inner && echo th\"\"ere $(ulimit -n)\n");
        read_until(&received, &id, |seen| seen.contains("there 200\r\n")).unwrap();
        assert_eq!(shells.directory(&id), Some(inner_path));
        assert!(shells.at_prompt(&id));

        let resized = TerminalSize {
            cols: 100,
            rows: 30,
        };
        shells.resize(&id, resized);
        shells.write(&id, b"echo si\"\"ze $(stty size)\n");
        read_until(&received, &id, |seen| seen.contains("size 30 100\r\n")).unwrap();

        // `ping` comes back twice once cat runs: the terminal's echo, then cat's copy.
        shells.write(&id, b"cat\n");
        shells.write(&id, b"ping\n");
        read_until(&received, &id, |seen| seen.matches("ping").count() >= 2).unwrap();
        assert!(!shells.at_prompt(&id), "cat holds the foreground");
        assert_eq!(shells.foreground_commands(&id), ["cat"]);

        // What a shell prints as it exits is reported before its exit.
        shells.write(&id, b"\x04");
        shells.write(&id, b"echo by\"\"e; exit 3\n");
        read_until(&received, &id, |seen| seen.contains("bye\r\n")).unwrap();
        let exited = read_until(&received, &id, |_| false);
        assert_eq!(exited, Err(3));
        std::fs::remove_dir_all(&scratch).unwrap();
    }

    /// The runtime blocks SIGTERM before its threads start; a shell must not inherit that, or `kill` would stop working in it, nor another terminal's pty, nor the app's connections.
    /// zsh happens to clear its own mask, and macOS's `ps` does not show one, so a program that leaves it alone reports it.
    #[test]
    fn a_shell_starts_with_no_signal_blocked_and_nothing_of_the_runtime_open() {
        let _alone = one_at_a_time();
        // A connection as the runtime gets one, open across exec until it is adopted. A device stands in for the app's socket, which the perl below counts.
        let handed_over = unsafe { libc::open(c"/dev/null".as_ptr(), libc::O_RDONLY) };
        let _adopted = crate::program::adopt_connection(handed_over).unwrap();
        let mut blocked: libc::sigset_t = unsafe { std::mem::zeroed() };
        let mut previous: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut blocked);
            libc::sigaddset(&mut blocked, libc::SIGTERM);
            libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        }

        let (inputs, received) = mpsc::channel();
        let budgets = Arc::new(Budgets::default());
        let mut shells = perl_shells(REPORT_INHERITED.into(), inputs, budgets);
        // The runtime holds the first shell's pty for as long as its terminal lives, so it is open when the second starts.
        let first = TerminalId("first".into());
        let second = TerminalId("second".into());
        shells.start(&first, None, SIZE);
        shells.start(&second, None, SIZE);
        unsafe { libc::pthread_sigmask(libc::SIG_SETMASK, &previous, std::ptr::null_mut()) };

        let reported = read_until(&received, &second, |seen| seen.contains("\r\n")).unwrap();
        assert_eq!(reported.trim(), "sigterm blocked: 0, other devices: 0");
    }

    /// A process the shell left behind, deaf to the hang-up, holds the pty and prints until it is closed, or for 30 s should the test fail first.
    #[test]
    fn the_exit_of_a_shell_is_reported_though_a_process_it_left_behind_keeps_printing() {
        let _alone = one_at_a_time();
        let leave_a_printer = r#"$| = 1; if (fork() == 0) { $SIG{HUP} = "IGNORE"; alarm 30; while (print "x" x 65536) {} exit 0; } exit 7;"#;
        let (inputs, received) = mpsc::channel();
        let mut shells = perl_shells(leave_a_printer.into(), inputs, Arc::new(Budgets::default()));
        let id = TerminalId("left-behind".into());
        shells.start(&id, None, SIZE);

        let exited = read_until(&received, &id, |_| false);
        assert_eq!(exited, Err(7));
        // With the pty's last copy closed, the printer's next write fails and it stops.
        shells.forget(&id);
    }

    fn exited_unreaped(pid: u32) -> bool {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let options = libc::WEXITED | libc::WNOWAIT | libc::WNOHANG;
        let waited = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, options) };
        let exited = unsafe { info.si_pid() };
        waited == 0 && exited == pid as libc::pid_t
    }

    #[test]
    fn a_shell_is_reaped_only_while_nothing_can_hang_it_up_and_never_hung_up_after() {
        let _alone = one_at_a_time();
        let (inputs, _received) = mpsc::channel();
        let budgets = Arc::new(Budgets::default());
        let mut shells = perl_shells("<STDIN>; exit 5;".into(), inputs, budgets);
        let id = TerminalId("reaped".into());
        shells.start(&id, None, SIZE);
        let pid = shells.shell_pid(&id).unwrap();
        let reaped = shells.shells[&id].reaped.clone();

        let held = reaped.lock().unwrap();
        shells.write(&id, b"go\n");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !exited_unreaped(pid) {
            assert!(
                Instant::now() < deadline,
                "the shell was never seen exited and still unreaped"
            );
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(Duration::from_millis(200));
        let while_held = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(while_held, 0, "the exited shell is still ours to signal");
        assert!(!*held, "no reap is recorded while the lock is held");
        drop(held);

        let deadline = Instant::now() + Duration::from_secs(2);
        while !*reaped.lock().unwrap() {
            assert!(Instant::now() < deadline, "the reap was never recorded");
            thread::sleep(Duration::from_millis(10));
        }
        let after_reap = unsafe { libc::kill(pid as libc::pid_t, 0) };
        assert_eq!(after_reap, -1, "the reaped pid is no longer ours");

        let before = HANG_UPS.get();
        shells.close(&id);
        shells.stop_all();
        let after = HANG_UPS.get();
        assert_eq!(after, before, "a hang-up was sent after the reap");
    }
}
