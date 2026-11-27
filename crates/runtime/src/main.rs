use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use taco_runtime::program;
use taco_runtime::pty::{ShellContext, ShellProgram};

const USAGE: &str = "usage: taco-shells-runtime --control-fd=<fd> --terminals-fd=<fd> --app=<name> --port-file=<path>";
const BAD_START: u8 = 2;
const PANICKED: i32 = 101;
/// From `<sys/syslimits.h>`.
const OPEN_MAX: libc::rlim_t = 10240;

struct Arguments {
    /// The app's control connection.
    control: UnixStream,
    /// The window's terminal channel.
    terminals: UnixStream,
    context: ShellContext,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(Arguments {
        control,
        terminals,
        context,
    }) = parsed(&arguments)
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(BAD_START);
    };

    // A panic on any thread ends the runtime, which the app sees as an exit with this code.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("taco-shells-runtime panicked: {info}");
        std::process::exit(PANICKED);
    }));
    let stopping = program::termination_signals();
    unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &stopping, std::ptr::null_mut()) };
    let inherited_limit = raise_open_file_limit();
    // Read before any thread starts.
    let mut shell = ShellProgram::login(context);
    shell.open_file_limit = inherited_limit;

    match program::run(control, terminals, shell) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("taco-shells-runtime could not start: {error}");
            ExitCode::from(BAD_START)
        }
    }
}

/// All four arguments, or none if one is missing, empty or not known.
fn parsed(arguments: &[String]) -> Option<Arguments> {
    let mut control = None;
    let mut terminals = None;
    let mut app = None;
    let mut port_file = None;
    for argument in arguments {
        let (name, value) = argument.split_once('=')?;
        if value.is_empty() {
            return None;
        }
        match name {
            "--control-fd" => control = Some(open_fd(value)?),
            "--terminals-fd" => terminals = Some(open_fd(value)?),
            "--app" => app = Some(value.to_string()),
            "--port-file" => port_file = Some(value.to_string()),
            _ => return None,
        }
    }
    let context = ShellContext {
        app: app?,
        port_file: port_file?,
    };
    Some(Arguments {
        control: control?,
        terminals: terminals?,
        context,
    })
}

fn open_fd(value: &str) -> Option<UnixStream> {
    let fd: RawFd = value.parse().ok()?;
    if fd <= libc::STDERR_FILENO {
        return None;
    }
    // Inherited without close-on-exec, so set here: otherwise every shell would hold the app's connections.
    let marked = unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    if marked < 0 {
        return None;
    }
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    Some(UnixStream::from(owned))
}

/// Lifts the soft limit on open files to what the system allows, and gives back the limit it had, for the shells.
/// launchd starts apps with 256, and each terminal holds five.
fn raise_open_file_limit() -> Option<libc::rlimit> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let read = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) };
    if read < 0 {
        return None;
    }
    let original = limit;
    // macOS refuses an unlimited soft limit for files; OPEN_MAX is the most it accepts.
    let mut wanted = limit.rlim_max;
    if wanted > OPEN_MAX {
        wanted = OPEN_MAX;
    }
    if wanted <= limit.rlim_cur {
        return None;
    }
    limit.rlim_cur = wanted;
    let raised = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) };
    if raised < 0 {
        return None;
    }
    Some(original)
}
