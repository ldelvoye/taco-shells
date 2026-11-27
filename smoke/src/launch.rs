use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

use taco_drive::SOCKET_VARIABLE;

use crate::driver::Driver;

const CONNECT_DEADLINE: Duration = Duration::from_secs(15);
const QUIT_DEADLINE: Duration = Duration::from_secs(10);
const QUIT_POLL: Duration = Duration::from_millis(100);
const APP_EXECUTABLE: &str = "Contents/MacOS/taco-shells";
const RUNTIME_NAME: &str = "taco-shells-runtime";

pub struct Launched {
    pub child: Child,
    pub driver: Driver,
}

pub fn launch(app: &Path, config: &Path, log: &Path) -> Result<Launched, String> {
    let socket = socket_path();
    remove_socket(&socket);

    let stdout = File::create(log)
        .map_err(|error| format!("could not create {}: {error}", log.display()))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("could not share {}: {error}", log.display()))?;

    let executable = app.join(APP_EXECUTABLE);
    let mut command = Command::new(&executable);
    command.env("TACO_SHELLS_CONFIG_DIR", config);
    command.env("TACO_SHELLS_BACKGROUND_WINDOW", "1");
    command.env(SOCKET_VARIABLE, &socket);
    command.stdout(stdout);
    command.stderr(stderr);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", executable.display()))?;

    let connected = Driver::connect(&socket, CONNECT_DEADLINE);
    match connected {
        Ok(driver) => Ok(Launched { child, driver }),
        Err(error) => {
            end(&mut child);
            remove_socket(&socket);
            Err(error)
        }
    }
}

pub fn runtime_pid(app_pid: u32) -> Option<u32> {
    let mut pgrep = Command::new("pgrep");
    pgrep.args(["-P", &app_pid.to_string(), "-f", RUNTIME_NAME]);
    let output = pgrep.output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next()?;
    first.trim().parse().ok()
}

pub fn quit(launched: Launched) {
    let mut child = launched.child;
    let pid = child.id();
    let mut kill = Command::new("kill");
    kill.args(["-TERM", &pid.to_string()]);
    let terminated = kill.status();
    if terminated.is_ok() && exited_within(&mut child, QUIT_DEADLINE) {
        remove_socket(&socket_path());
        return;
    }
    end(&mut child);
    remove_socket(&socket_path());
}

fn socket_path() -> PathBuf {
    // macOS caps a socket path at 104 bytes, and a worktree's target directory can be longer.
    let name = format!("taco-drive-{}.sock", std::process::id());
    std::env::temp_dir().join(name)
}

fn remove_socket(socket: &Path) {
    if socket.exists() {
        let _ = fs::remove_file(socket);
    }
}

fn exited_within(child: &mut Child, deadline: Duration) -> bool {
    let start = Instant::now();
    loop {
        let status = child.try_wait();
        match status {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return false,
        }
        if start.elapsed() >= deadline {
            return false;
        }
        thread::sleep(QUIT_POLL);
    }
}

fn end(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
