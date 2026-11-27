use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use taco_agents::{Harness, Version};

const PASSED_VARIABLES: [&str; 5] = ["HOME", "USER", "PATH", "LANG", "TMPDIR"];
const RUN_LIMIT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

fn numbered_version(text: &str) -> Option<(u32, u32, u32)> {
    let mut numbers = text.split('.');
    let major_text = numbers.next()?;
    let minor_text = numbers.next()?;
    let patch_text = numbers.next()?;
    let major = major_text.parse().ok()?;
    let minor = minor_text.parse().ok()?;
    let patch_digits = leading_digits(patch_text);
    let patch = patch_digits.parse().ok()?;
    Some((major, minor, patch))
}

fn leading_digits(text: &str) -> &str {
    let digit_count = text.chars().take_while(char::is_ascii_digit).count();
    &text[..digit_count]
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let mode = metadata.permissions().mode();
    let runnable = mode & 0o111 != 0;
    metadata.is_file() && runnable
}

fn node_directory_version(name: &str) -> Option<(u32, u32, u32)> {
    let numbers = name.strip_prefix('v')?;
    numbered_version(numbers)
}

fn newest_nvm_program(nvm_folder: &Path, command: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(nvm_folder).ok()?;
    let mut newest: Option<((u32, u32, u32), PathBuf)> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let shown_name = name.to_string_lossy();
        let Some(version) = node_directory_version(&shown_name) else {
            continue;
        };
        let is_newer = match &newest {
            Some((best, _)) => version > *best,
            None => true,
        };
        if is_newer {
            newest = Some((version, entry.path()));
        }
    }
    let (_, directory) = newest?;
    let program = directory.join("bin").join(command);
    if is_executable_file(&program) {
        return Some(program);
    }
    None
}

fn run_for_output(program: &Path, arguments: &[&str], limit: Duration) -> Option<String> {
    let mut command = Command::new(program);
    command.args(arguments).env_clear();
    for name in PASSED_VARIABLES {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .stdout(Stdio::piped())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (sender, received) = mpsc::channel();
    // Read apart from the wait, so a login shell that leaves a process holding stdout open cannot hold the caller past the limit.
    thread::spawn(move || {
        let mut output = String::new();
        let read = stdout.read_to_string(&mut output);
        if read.is_ok() {
            let _ = sender.send(output);
        }
    });

    let started = Instant::now();
    loop {
        let status = child.try_wait();
        match status {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => {
                end_group(&mut child);
                return None;
            }
        }
        if started.elapsed() >= limit {
            end_group(&mut child);
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }

    let spent = started.elapsed();
    let left = limit.saturating_sub(spent);
    received.recv_timeout(left).ok()
}

fn end_group(child: &mut Child) {
    let group = child.id() as libc::pid_t;
    // The child was started as the leader of its own group, so this reaches only what it started.
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
    let _ = child.wait();
}

fn last_executable_line(output: &str) -> Option<PathBuf> {
    let mut lines = output.lines();
    let last = lines.rfind(|line| !line.trim().is_empty())?;
    let found = PathBuf::from(last.trim());
    if is_executable_file(&found) {
        return Some(found);
    }
    None
}

fn login_shell() -> Option<PathBuf> {
    let shell = std::env::var_os("SHELL")?;
    Some(PathBuf::from(shell))
}

fn login_shell_program(command: &str) -> Option<PathBuf> {
    let shell = login_shell()?;
    let arguments = ["-l", "-c", "command -v \"$1\"", "taco-shells", command];
    let output = run_for_output(&shell, &arguments, RUN_LIMIT)?;
    last_executable_line(&output)
}

pub(crate) fn find_program(home: &Path, command: &str) -> Option<PathBuf> {
    let fixed = [
        home.join(".local/bin").join(command),
        Path::new("/opt/homebrew/bin").join(command),
        Path::new("/usr/local/bin").join(command),
        home.join(".npm-global/bin").join(command),
    ];
    for candidate in fixed {
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    let nvm_folder = home.join(".nvm/versions/node");
    let nvm_program = newest_nvm_program(&nvm_folder, command);
    if nvm_program.is_some() {
        return nvm_program;
    }

    login_shell_program(command)
}

pub(crate) fn program_version(program: &Path, harness: &Harness) -> Option<Version> {
    let setup = harness.setup()?;
    let flag = setup.version_flag;
    let direct = run_for_output(program, &[flag], RUN_LIMIT);
    if let Some(output) = direct {
        let version = harness.version_of(&output);
        if version.is_some() {
            return version;
        }
    }

    // An agent installed through npm or nvm is a node script, and the app's own PATH may not reach node.
    let shell = login_shell()?;
    let program_text = program.to_str()?;
    let arguments = [
        "-l",
        "-c",
        "\"$1\" \"$2\"",
        "taco-shells",
        program_text,
        flag,
    ];
    let through_shell = run_for_output(&shell, &arguments, RUN_LIMIT)?;
    harness.version_of(&through_shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_LIMIT: Duration = Duration::from_millis(200);

    fn executable_file(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_login_shells_answer_is_its_last_line() {
        let directory = tempfile::tempdir().unwrap();
        let claude = directory.path().join("claude");
        executable_file(&claude, "");
        let shown = claude.display();
        let with_banner = format!("Now using node v18\nWelcome back\n{shown}\n\n");

        let found = last_executable_line(&with_banner);

        assert_eq!(found, Some(claude));
        assert_eq!(last_executable_line("Welcome back\n"), None);
    }

    #[test]
    fn nvm_versions_compare_as_numbers() {
        let home = tempfile::tempdir().unwrap();
        let nvm = home.path().join("versions/node");
        executable_file(&nvm.join("v9.0.0/bin/claude"), "");
        executable_file(&nvm.join("v18.2.0/bin/claude"), "");

        let found = newest_nvm_program(&nvm, "claude");

        assert_eq!(found, Some(nvm.join("v18.2.0/bin/claude")));
    }

    #[test]
    fn a_process_that_outlives_the_limit_is_killed_and_yields_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("slow");
        executable_file(&script, "#!/bin/sh\nsleep 10 &\nwait\n");

        let started = Instant::now();
        let output = run_for_output(&script, &[], TEST_LIMIT);

        assert_eq!(output, None);
        assert!(started.elapsed() < TEST_LIMIT + Duration::from_millis(500));
    }
}
