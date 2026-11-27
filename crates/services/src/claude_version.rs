use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub const MINIMUM_VERSION: (u32, u32, u32) = (2, 1, 280);

const PASSED_VARIABLES: [&str; 5] = ["HOME", "USER", "PATH", "LANG", "TMPDIR"];
const RUN_LIMIT: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

pub fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    let first_line = output.lines().next()?;
    let first_word = first_line.split_whitespace().next()?;
    let mut numbers = first_word.split('.');
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

pub fn is_too_old(version: (u32, u32, u32)) -> bool {
    version < MINIMUM_VERSION
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
    parse_version(numbers)
}

fn newest_nvm_claude(nvm_folder: &Path) -> Option<PathBuf> {
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
    let claude = directory.join("bin/claude");
    if is_executable_file(&claude) {
        return Some(claude);
    }
    None
}

fn run_for_output(program: &Path, arguments: &[&str]) -> Option<String> {
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
        if started.elapsed() >= RUN_LIMIT {
            end_group(&mut child);
            return None;
        }
        thread::sleep(POLL_INTERVAL);
    }

    let spent = started.elapsed();
    let left = RUN_LIMIT.saturating_sub(spent);
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

fn login_shell_claude() -> Option<PathBuf> {
    let shell = login_shell()?;
    let output = run_for_output(&shell, &["-l", "-c", "command -v claude"])?;
    last_executable_line(&output)
}

pub fn find_claude(home: &Path) -> Option<PathBuf> {
    let fixed = [
        home.join(".local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        home.join(".npm-global/bin/claude"),
    ];
    for candidate in fixed {
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    let nvm_folder = home.join(".nvm/versions/node");
    let nvm_claude = newest_nvm_claude(&nvm_folder);
    if nvm_claude.is_some() {
        return nvm_claude;
    }

    login_shell_claude()
}

pub fn claude_version(claude: &Path) -> Option<(u32, u32, u32)> {
    let direct = run_for_output(claude, &["--version"]);
    if let Some(output) = direct {
        let version = parse_version(&output);
        if version.is_some() {
            return version;
        }
    }

    // A claude installed through npm or nvm is a node script, and the app's own PATH may not reach node.
    let shell = login_shell()?;
    let claude_text = claude.to_str()?;
    let arguments = ["-l", "-c", "\"$1\" --version", "taco-shells", claude_text];
    let through_shell = run_for_output(&shell, &arguments)?;
    parse_version(&through_shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn executable_file(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn reads_the_leading_version() {
        assert_eq!(parse_version("2.1.332 (Claude Code)"), Some((2, 1, 332)));
        assert_eq!(parse_version("10.0.1\n"), Some((10, 0, 1)));
        assert_eq!(parse_version("Claude Code"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("2.1.332-beta.1"), Some((2, 1, 332)));
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
    fn the_minimum_is_the_first_version_that_is_not_too_old() {
        assert!(is_too_old((2, 1, 279)));
        assert!(!is_too_old((2, 1, 280)));
        assert!(!is_too_old((2, 2, 0)));
        assert!(!is_too_old((3, 0, 0)));
    }

    #[test]
    fn nvm_versions_compare_as_numbers() {
        let home = tempfile::tempdir().unwrap();
        let nvm = home.path().join("versions/node");
        executable_file(&nvm.join("v9.0.0/bin/claude"), "");
        executable_file(&nvm.join("v18.2.0/bin/claude"), "");

        let found = newest_nvm_claude(&nvm);

        assert_eq!(found, Some(nvm.join("v18.2.0/bin/claude")));
    }

    #[test]
    fn a_process_that_outlives_the_limit_is_killed_and_yields_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("slow");
        executable_file(&script, "#!/bin/sh\nsleep 10 &\nwait\n");

        let started = Instant::now();
        let output = run_for_output(&script, &[]);

        assert_eq!(output, None);
        assert!(started.elapsed() < Duration::from_millis(3500));
    }
}
