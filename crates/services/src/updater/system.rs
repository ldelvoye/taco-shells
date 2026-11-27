use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{self, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const EXTRACT_POLL: Duration = Duration::from_millis(50);

pub(super) fn extract_zip(zip: &Path, into: &Path) -> io::Result<()> {
    let mut child = Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(into)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + EXTRACT_TIMEOUT;
    loop {
        let status = child.try_wait()?;
        if let Some(status) = status {
            if status.success() {
                return Ok(());
            }
            return Err(io::Error::other(format!("ditto exited with {status}")));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "ditto overran its timeout",
            ));
        }
        thread::sleep(EXTRACT_POLL);
    }
}

pub(super) fn spawn_update_helper(
    helper: &Path,
    installed: &Path,
    extracted: &Path,
) -> io::Result<()> {
    let pid = process::id();
    let mut command = Command::new("/bin/sh");
    command
        .arg(helper)
        .arg(pid.to_string())
        .arg(installed)
        .arg(extracted)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // A process group of its own, so the helper outlives the app it is about to replace.
        .process_group(0);
    command.spawn()?;
    Ok(())
}
