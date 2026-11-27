//! Tests of `install.sh`, the first install, which `promote` attaches to every stable release. Each run gets a
//! throwaway install folder and stand-ins for `open` and, where a case needs it, `mv`, so nothing opens on screen.

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use taco_identity::{APP_PROGRAM, PROGRAMS_PATH, STABLE};

const RUNNING_DEADLINE: Duration = Duration::from_secs(5);
const RUNNING_POLL: Duration = Duration::from_millis(50);

static SCRATCH_COUNT: AtomicU64 = AtomicU64::new(0);

/// A folder under the system's temp directory, removed when it goes out of scope.
struct Scratch {
    path: PathBuf,
}

/// A download, and whether `mv` is made to fail while it is installed.
struct Case {
    zip: PathBuf,
    mv_fails: bool,
}

impl Scratch {
    fn new() -> Scratch {
        let count = SCRATCH_COUNT.fetch_add(1, Ordering::Relaxed);
        let name = format!("taco-install-test-{}-{count}", std::process::id());
        let path = env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Scratch { path }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// install.sh names the bundle by hand, so a rename of the stable app that misses it fails here.
fn app_name() -> String {
    format!("{}.app", STABLE.application_name)
}

fn install_script() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("install.sh")
}

fn write_marker(bundle: &Path, text: &str) {
    let contents = bundle.join("Contents");
    fs::create_dir_all(&contents).unwrap();
    fs::write(contents.join("marker"), text).unwrap();
}

fn marker(install_dir: &Path) -> String {
    let path = install_dir.join(app_name()).join("Contents").join("marker");
    fs::read_to_string(path).unwrap()
}

fn zip_of(scratch: &Scratch, bundle_name: &str) -> PathBuf {
    let bundle = scratch.path.join(bundle_name);
    write_marker(&bundle, "new");
    // curl refuses a file:// URL with a space in it, as the bundle's name has.
    let zip_name = bundle_name.replace(' ', "-");
    let zip = scratch.path.join(format!("{zip_name}.zip"));
    let mut ditto = Command::new("ditto");
    ditto.args(["-c", "-k", "--keepParent"]);
    ditto.arg(&bundle);
    ditto.arg(&zip);
    let made = ditto.status().unwrap();
    assert!(made.success());
    zip
}

fn write_stub(stubs: &Path, command: &str, body: &str) {
    let path = stubs.join(command);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let permissions = fs::Permissions::from_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
}

/// `install.sh` against a zip URL, with the stubs folder first on PATH; true when it exits zero.
fn install(install_dir: &Path, zip_url: &str, stubs: &Path) -> bool {
    let mut path = stubs.as_os_str().to_os_string();
    if let Some(inherited) = env::var_os("PATH") {
        path.push(":");
        path.push(inherited);
    }
    let mut sh = Command::new("sh");
    sh.arg(install_script());
    sh.env("TACO_SHELLS_INSTALL_DIR", install_dir);
    sh.env("TACO_SHELLS_ZIP_URL", zip_url);
    sh.env("PATH", path);
    sh.stdout(Stdio::null());
    sh.stderr(Stdio::null());
    let status = sh.status().unwrap();
    status.success()
}

fn is_running(pattern: &str) -> bool {
    let mut pgrep = Command::new("pgrep");
    pgrep.args(["-f", pattern]);
    pgrep.stdout(Stdio::null());
    let status = pgrep.status().unwrap();
    status.success()
}

fn wait_until_running(pattern: &str) {
    let started = Instant::now();
    while !is_running(pattern) {
        assert!(
            started.elapsed() < RUNNING_DEADLINE,
            "nothing matched {pattern}"
        );
        thread::sleep(RUNNING_POLL);
    }
}

fn end(mut child: Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn a_bad_download_or_a_failed_move_leaves_the_old_install_and_opens_nothing() {
    let downloads = Scratch::new();
    let not_a_zip = downloads.path.join("not-a-zip.zip");
    fs::write(&not_a_zip, "this is not a zip").unwrap();
    let missing = downloads.path.join("missing.zip");
    let another_app = zip_of(&downloads, "Another.app");
    let good = zip_of(&downloads, &app_name());
    let cases = [
        Case {
            zip: missing,
            mv_fails: false,
        },
        Case {
            zip: not_a_zip,
            mv_fails: false,
        },
        Case {
            zip: another_app,
            mv_fails: false,
        },
        Case {
            zip: good,
            mv_fails: true,
        },
    ];

    for Case { zip, mv_fails } in cases {
        let install_dir = Scratch::new();
        let stubs = Scratch::new();
        let opened = stubs.path.join("opened");
        write_stub(
            &stubs.path,
            "open",
            &format!("echo \"$@\" >> '{}'", opened.display()),
        );
        if mv_fails {
            write_stub(&stubs.path, "mv", "exit 1");
        }
        write_marker(&install_dir.path.join(app_name()), "old");

        let zip_url = format!("file://{}", zip.display());
        let installed = install(&install_dir.path, &zip_url, &stubs.path);

        assert!(!installed, "{} succeeded", zip.display());
        assert_eq!(marker(&install_dir.path), "old", "for {}", zip.display());
        assert!(!opened.exists(), "{} opened something", zip.display());
    }
}

#[test]
fn it_refuses_while_the_installed_app_runs() {
    let downloads = Scratch::new();
    let zip = zip_of(&downloads, &app_name());
    let install_dir = Scratch::new();
    let stubs = Scratch::new();
    write_stub(&stubs.path, "open", "exit 0");
    let bundle = install_dir.path.join(app_name());
    write_marker(&bundle, "old");
    let programs = bundle.join(PROGRAMS_PATH);
    fs::create_dir_all(&programs).unwrap();
    // A loop of short sleeps, so the shell carrying the bundle's path is the process that ends at the kill.
    write_stub(&programs, APP_PROGRAM, "while :; do sleep 0.1; done");

    let running = Command::new(programs.join(APP_PROGRAM)).spawn().unwrap();
    let pattern = format!("{}/", programs.display());
    wait_until_running(&pattern);
    let zip_url = format!("file://{}", zip.display());
    let installed = install(&install_dir.path, &zip_url, &stubs.path);
    end(running);

    assert!(!installed);
    assert_eq!(marker(&install_dir.path), "old");
}
