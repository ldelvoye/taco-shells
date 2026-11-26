use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use taco_protocol::{TerminalId, TerminalSize};

use crate::budget::{Budget, Budgets};
use crate::core::Input;
use crate::platform;
use crate::shells::Shells;

/// The most read from a pty at once. One read is what another terminal's echo can wait behind.
const READ_BYTES: usize = 64 * 1024;
const FALLBACK_SHELL: &str = "/bin/zsh";
/// A process ended by a signal reports 128 plus the signal, as shells do.
const SIGNAL_EXIT_BASE: i32 = 128;
/// What an exit reports when its status could not be read.
const UNKNOWN_EXIT_CODE: i32 = 1;
/// The most read after a shell has exited, before its exit is reported anyway.
const DRAIN_BYTES: usize = 1024 * 1024;

/// The program each terminal runs, and where it starts when no directory is given or the one given is not a directory.
pub struct ShellProgram {
    pub path: String,
    pub arguments: Vec<String>,
    pub home: String,
    /// The open-file limit the runtime was started with, given back to each shell when the runtime raised its own.
    pub open_file_limit: Option<libc::rlimit>,
}

struct Shell {
    pid: u32,
    /// Set once the shell has been waited for: its pid may then belong to another process.
    reaped: Arc<AtomicBool>,
    master: Arc<OwnedFd>,
    writes: Sender<Vec<u8>>,
}

/// The terminals' shells, each in a pty of its own, with a reader, a writer and a waiter thread.
pub struct PtyShells {
    program: ShellProgram,
    inputs: Sender<Input>,
    budgets: Arc<Budgets>,
    shells: HashMap<TerminalId, Shell>,
}

impl ShellProgram {
    /// The user's login shell, the way macOS terminals start one. Before any thread starts: `getpwuid` is not thread-safe.
    pub fn login() -> ShellProgram {
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
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        // Most CLIs only emit OSC 8 hyperlinks for terminals they recognise by name.
        command.env("FORCE_HYPERLINK", "1");
        if std::env::var_os("LANG").is_none() {
            command.env("LANG", "en_US.UTF-8");
        }
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
                // The shell gets the limit it would have had in any other terminal.
                if let Some(limit) = open_file_limit {
                    libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
                }
                Ok(())
            });
        }
        let (wake_reader, wake_writer) = wake_pipe()?;
        let child = command.spawn()?;
        // Closes this process's copies of the pty's slave, so the pty closes once the shell's side lets go.
        drop(command);
        let pid = child.id();

        let budget = self.budgets.open(id);
        let master = Arc::new(master);
        let reaped = Arc::new(AtomicBool::new(false));
        let started = self.start_threads(
            id,
            child,
            &master,
            budget,
            &reaped,
            wake_reader,
            wake_writer,
        );
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
    #[allow(clippy::too_many_arguments)]
    fn start_threads(
        &self,
        id: &TerminalId,
        mut child: std::process::Child,
        master: &Arc<OwnedFd>,
        budget: Arc<Budget>,
        reaped: &Arc<AtomicBool>,
        wake_reader: OwnedFd,
        wake_writer: OwnedFd,
    ) -> io::Result<Sender<Vec<u8>>> {
        let exit_code = Arc::new(Mutex::new(None));
        let waiter_budget = budget.clone();
        let waiter_exit_code = exit_code.clone();
        let waiter_reaped = reaped.clone();
        thread::Builder::new()
            .name(format!("wait {}", id.0))
            .spawn(move || {
                let code = match child.wait() {
                    Ok(status) => exit_code_of(status),
                    Err(_) => UNKNOWN_EXIT_CODE,
                };
                waiter_reaped.store(true, Ordering::SeqCst);
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
            wake: wake_reader,
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
}

/// One shell's output, read in order and then its exit, so the exit always follows the last output.
struct Reader {
    id: TerminalId,
    master: File,
    wake: OwnedFd,
    exit_code: Arc<Mutex<Option<i32>>>,
    budget: Arc<Budget>,
    inputs: Sender<Input>,
}

enum Polled {
    Readable,
    Exited,
    Interrupted,
    Failed,
}

enum Drained {
    Data(usize),
    /// Every process has let go of the pty, or nothing is left to read.
    Closed,
    /// The core has gone, so nothing more is wanted.
    Unwanted,
}

impl Reader {
    fn run(mut self) {
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

fn set_nonblocking(fd: RawFd) {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
}

/// SIGHUP to the shell's process group, as a terminal closing does; the shell leads its group.
fn hang_up(pid: u32, reaped: &AtomicBool) {
    if reaped.load(Ordering::SeqCst) {
        return;
    }
    let group = pid as libc::pid_t;
    unsafe { libc::killpg(group, libc::SIGHUP) };
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
    use std::time::Duration;

    const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };
    /// Only a safety net: every wait below ends on output the shell prints.
    const PATIENCE: Duration = Duration::from_secs(20);

    /// Reads the shell's output until `ready` says enough has come, and gives back its exit if it came first.
    fn read_until(
        received: &Receiver<Input>,
        watched: &TerminalId,
        ready: impl Fn(&str) -> bool,
    ) -> Option<i32> {
        let mut seen = String::new();
        loop {
            let input = received
                .recv_timeout(PATIENCE)
                .expect("the shell went quiet");
            match input {
                Input::Output { id, data } if id == *watched => {
                    let text = String::from_utf8_lossy(&data);
                    seen.push_str(&text);
                    if ready(&seen) {
                        return None;
                    }
                }
                Input::Exited { id, code } if id == *watched => return Some(code),
                Input::Output { .. } | Input::Exited { .. } => {}
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn a_shell_starts_where_asked_follows_its_cd_shows_its_program_and_exits_with_its_code() {
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
        assert_eq!(hung_up, Some(128 + libc::SIGHUP));

        // The line as typed echoes `th""ere`; only running it prints `there`.
        shells.write(&id, b"cd inner && echo th\"\"ere $(ulimit -n)\n");
        read_until(&received, &id, |seen| seen.contains("there 200\r\n"));
        assert_eq!(shells.directory(&id), Some(inner_path));
        assert!(shells.at_prompt(&id));

        let resized = TerminalSize {
            cols: 100,
            rows: 30,
        };
        shells.resize(&id, resized);
        shells.write(&id, b"echo si\"\"ze $(stty size)\n");
        read_until(&received, &id, |seen| seen.contains("size 30 100\r\n"));

        // `ping` comes back twice once cat runs: the terminal's echo, then cat's copy.
        shells.write(&id, b"cat\n");
        shells.write(&id, b"ping\n");
        read_until(&received, &id, |seen| seen.matches("ping").count() >= 2);
        assert!(!shells.at_prompt(&id), "cat holds the foreground");
        assert_eq!(shells.foreground_commands(&id), ["cat"]);

        // What a shell prints as it exits is reported before its exit.
        shells.write(&id, b"\x04");
        shells.write(&id, b"echo by\"\"e; exit 3\n");
        let exited_first = read_until(&received, &id, |seen| seen.contains("bye\r\n"));
        assert_eq!(exited_first, None);
        let code = read_until(&received, &id, |_| false);
        assert_eq!(code, Some(3));
        std::fs::remove_dir_all(&scratch).unwrap();
    }
}
