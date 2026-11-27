use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use taco_identity::{
    RELEASE_KEYS, RELEASE_SIGNING_NAMESPACE, UPDATE_KEY_VARIABLE, release_signature_name,
};

use super::{Feed, ReleaseInfo};

// By absolute path: an app opened from Finder has no Homebrew on its PATH.
const SSH_KEYGEN: &str = "/usr/bin/ssh-keygen";
const CHILD_PATH: &str = "/usr/bin:/bin";
const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);
const VERIFY_POLL: Duration = Duration::from_millis(20);
const MAX_SIGNATURE_BYTES: u64 = 64 * 1024;
const ALLOWED_SIGNERS_NAME: &str = "allowed_signers";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SignatureRefusal {
    pub(crate) reason: String,
}

fn refuse(reason: String) -> SignatureRefusal {
    SignatureRefusal { reason }
}

fn dev_key() -> Option<String> {
    let name = UPDATE_KEY_VARIABLE?;
    let path = std::env::var_os(name)?;
    let read = fs::read_to_string(path);
    let text = read.ok()?;
    let first = text.lines().next()?;
    let key = first.trim();
    if key.is_empty() {
        return None;
    }
    Some(key.to_string())
}

pub(crate) fn release_keys() -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for key in RELEASE_KEYS {
        keys.push(key.to_string());
    }
    let dev = dev_key();
    if let Some(key) = dev {
        keys.push(key);
    }
    keys
}

fn write_allowed_signers(path: &Path, keys: &[String]) -> io::Result<()> {
    let namespace = RELEASE_SIGNING_NAMESPACE;
    let mut text = String::new();
    for key in keys {
        text.push_str(&format!("{namespace} namespaces=\"{namespace}\" {key}\n"));
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())
}

fn wait_with_deadline(child: &mut Child) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    loop {
        let polled = child.try_wait();
        match polled {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(VERIFY_POLL);
    }
}

fn failure_reason(stderr: &str, status: ExitStatus) -> String {
    let first = stderr.lines().next();
    let line = match first {
        Some(line) => line.trim(),
        None => "",
    };
    if line.is_empty() {
        return format!("{SSH_KEYGEN} exited with {status}");
    }
    line.to_string()
}

fn run_verify(allowed: &Path, signature: &Path, zip: &Path) -> Result<(), String> {
    let opened = File::open(zip);
    let message = opened.map_err(|error| format!("the zip could not be opened ({error})"))?;
    let namespace = RELEASE_SIGNING_NAMESPACE;
    let mut command = Command::new(SSH_KEYGEN);
    command
        .args(["-Y", "verify", "-f"])
        .arg(allowed)
        .args(["-I", namespace, "-n", namespace, "-s"])
        .arg(signature)
        .env_clear()
        .env("PATH", CHILD_PATH)
        .stdin(Stdio::from(message))
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let spawned = command.spawn();
    let mut child =
        spawned.map_err(|error| format!("{SSH_KEYGEN} could not be started ({error})"))?;

    let stderr_pipe = child.stderr.take();
    let reader = thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = stderr_pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    });
    let waited = wait_with_deadline(&mut child);
    let joined = reader.join();
    let mut stderr = String::new();
    if let Ok(text) = joined {
        stderr = text;
    }

    match waited {
        Err(error) => Err(format!("{SSH_KEYGEN} could not be waited for ({error})")),
        Ok(None) => Err(format!("{SSH_KEYGEN} overran its timeout")),
        Ok(Some(status)) => {
            if status.success() {
                return Ok(());
            }
            Err(failure_reason(&stderr, status))
        }
    }
}

fn check(
    release: &ReleaseInfo,
    zip: &Path,
    signature: &Path,
    allowed: &Path,
    feed: &dyn Feed,
    keys: &[String],
) -> Result<(), SignatureRefusal> {
    let Some(url) = &release.signature_url else {
        return Err(refuse("the release has no signature asset".to_string()));
    };
    let downloaded = feed.download(url, signature);
    let download = match downloaded {
        Ok(download) => download,
        Err(error) => {
            return Err(refuse(format!(
                "the signature could not be downloaded ({error})"
            )));
        }
    };
    if download.bytes > MAX_SIGNATURE_BYTES {
        let bytes = download.bytes;
        return Err(refuse(format!(
            "the signature was {bytes} bytes, too large"
        )));
    }
    if keys.is_empty() {
        return Err(refuse("no release key is configured".to_string()));
    }
    let written = write_allowed_signers(allowed, keys);
    if let Err(error) = written {
        return Err(refuse(format!(
            "the allowed signers could not be written ({error})"
        )));
    }
    let verified = run_verify(allowed, signature, zip);
    verified.map_err(refuse)
}

pub(crate) fn verify_release(
    release: &ReleaseInfo,
    zip: &Path,
    staging: &Path,
    feed: &dyn Feed,
    keys: &[String],
) -> Result<(), SignatureRefusal> {
    let signature_name = release_signature_name(&release.version);
    let signature = staging.join(signature_name);
    let allowed = staging.join(ALLOWED_SIGNERS_NAME);
    let outcome = check(release, zip, &signature, &allowed, feed, keys);
    let _ = fs::remove_file(&signature);
    let _ = fs::remove_file(&allowed);
    outcome
}
