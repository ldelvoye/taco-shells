use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

// Stands in for macOS's open: it records what it was asked to open and refuses an app holding a "broken" file.
const OPEN_STUB: &str =
    "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$OPEN_LOG\"\nif [ -e \"$1/broken\" ]; then exit 1; fi\n";

struct Scratch {
    home: TempDir,
    installed: PathBuf,
    extracted: PathBuf,
}

fn helper() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../../resources/update-helper.sh")
}

fn make_app(path: &Path, marker: &str) {
    fs::create_dir_all(path).unwrap();
    fs::write(path.join(marker), "").unwrap();
}

fn scratch() -> Scratch {
    let home = TempDir::new().unwrap();
    let installed = home.path().join("Applications/Fake App.app");
    let extracted = home.path().join("updates/Fake App.app");
    make_app(&installed, "old");
    Scratch {
        home,
        installed,
        extracted,
    }
}

fn run_helper(scratch: &Scratch) -> Vec<PathBuf> {
    let home = scratch.home.path();
    let stubs = home.join("bin");
    fs::create_dir(&stubs).unwrap();
    let open = stubs.join("open");
    fs::write(&open, OPEN_STUB).unwrap();
    fs::set_permissions(&open, fs::Permissions::from_mode(0o755)).unwrap();
    let open_log = home.join("opened");

    let mut quit_app = Command::new("/usr/bin/true").spawn().unwrap();
    let quit_pid = quit_app.id().to_string();
    quit_app.wait().unwrap();

    let path = format!("{}:/bin:/usr/bin", stubs.display());
    let status = Command::new("/bin/sh")
        .arg(helper())
        .arg(&quit_pid)
        .arg(&scratch.installed)
        .arg(&scratch.extracted)
        .env_clear()
        .env("HOME", home)
        .env("OPEN_LOG", &open_log)
        .env("PATH", path)
        .status()
        .unwrap();
    let _ = status;

    let log = fs::read_to_string(&open_log).unwrap_or_default();
    log.lines().map(PathBuf::from).collect()
}

#[test]
fn a_failed_update_always_leaves_the_old_app_in_place_and_reopened() {
    let no_trash = scratch();
    make_app(&no_trash.extracted, "new");
    let opened = run_helper(&no_trash);
    assert!(
        no_trash.installed.join("old").exists(),
        "no Trash: old app kept"
    );
    assert_eq!(opened, vec![no_trash.installed.clone()], "no Trash");

    let no_new_app = scratch();
    fs::create_dir(no_new_app.home.path().join(".Trash")).unwrap();
    let opened = run_helper(&no_new_app);
    assert!(
        no_new_app.installed.join("old").exists(),
        "no new app: old app back"
    );
    assert_eq!(opened, vec![no_new_app.installed.clone()], "no new app");

    let new_app_will_not_open = scratch();
    fs::create_dir(new_app_will_not_open.home.path().join(".Trash")).unwrap();
    make_app(&new_app_will_not_open.extracted, "new");
    fs::write(new_app_will_not_open.extracted.join("broken"), "").unwrap();
    let opened = run_helper(&new_app_will_not_open);
    let installed = new_app_will_not_open.installed.clone();
    assert!(
        installed.join("old").exists(),
        "broken new app: old app back"
    );
    assert!(
        new_app_will_not_open.extracted.join("new").exists(),
        "broken new app: put back where it was"
    );
    assert_eq!(
        opened,
        vec![installed.clone(), installed],
        "broken new app: new, then old"
    );
}
