use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::updater::is_version;

const APPLICATIONS_DIRECTORY: &str = "/Applications";
const VERSION_KEY: &str = "CFBundleShortVersionString";
const EXTRACT_POLL: Duration = Duration::from_millis(50);
const UNBUNDLED_VERSION: &str = "dev";

fn bundle_of(executable: &Path) -> Option<PathBuf> {
    let macos_directory = executable.parent()?;
    let contents_directory = macos_directory.parent()?;
    let bundle = contents_directory.parent()?;

    let macos_name = macos_directory.file_name()?;
    let contents_name = contents_directory.file_name()?;
    let bundle_extension = bundle.extension()?;
    if macos_name != "MacOS" || contents_name != "Contents" || bundle_extension != "app" {
        return None;
    }
    Some(bundle.to_path_buf())
}

pub(crate) fn bundle_version(executable: &Path) -> Option<String> {
    let bundle = bundle_of(executable)?;
    let info_path = bundle.join("Contents").join("Info.plist");
    let parsed = plist::Value::from_file(info_path);
    let Ok(info) = parsed else {
        return None;
    };
    let dictionary = info.as_dictionary()?;
    let value = dictionary.get(VERSION_KEY)?;
    let version = value.as_string()?;
    if !is_version(version) {
        return None;
    }
    Some(version.to_string())
}

pub fn app_version(executable: &Path) -> String {
    let found = bundle_version(executable);
    match found {
        Some(version) => version,
        None => UNBUNDLED_VERSION.to_string(),
    }
}

pub(crate) fn installed_bundle(executable: &Path) -> Option<PathBuf> {
    let bundle = bundle_of(executable)?;
    if !bundle.starts_with(APPLICATIONS_DIRECTORY) {
        return None;
    }
    Some(bundle)
}

pub(crate) fn extract_zip(zip: &Path, into: &Path, timeout: Duration) -> io::Result<()> {
    let mut child = Command::new("ditto")
        .arg("-x")
        .arg("-k")
        .arg(zip)
        .arg(into)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + timeout;
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

pub(crate) fn spawn_update_helper(
    helper: &Path,
    pid: u32,
    installed: &Path,
    extracted: &Path,
) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_bundle_with_version(root: &Path, version_xml: &str) -> PathBuf {
        let contents = root.join("X.app").join("Contents");
        let macos = contents.join("MacOS");
        fs::create_dir_all(&macos).unwrap();
        let info = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
             <key>CFBundleShortVersionString</key><string>{version_xml}</string></dict></plist>"
        );
        fs::write(contents.join("Info.plist"), info).unwrap();
        macos.join("x")
    }

    #[test]
    fn bundle_version_is_a_plain_version_read_from_the_bundle_or_nothing() {
        let temp = tempfile::TempDir::new().unwrap();

        let plain = write_bundle_with_version(&temp.path().join("a"), "1.2.3");
        assert_eq!(bundle_version(&plain), Some("1.2.3".to_string()));

        let beta = write_bundle_with_version(&temp.path().join("b"), "1.2.3-beta");
        assert_eq!(bundle_version(&beta), None);

        let loose = temp.path().join("target").join("debug").join("taco");
        assert_eq!(bundle_version(&loose), None);
    }

    #[test]
    fn only_a_bundle_under_applications_is_installed() {
        let installed =
            installed_bundle(Path::new("/Applications/Taco Shells.app/Contents/MacOS/x"));
        assert_eq!(
            installed,
            Some(PathBuf::from("/Applications/Taco Shells.app"))
        );

        let elsewhere = installed_bundle(Path::new("/tmp/Taco Shells.app/Contents/MacOS/x"));
        assert_eq!(elsewhere, None);

        let not_a_bundle = installed_bundle(Path::new("/Applications/taco"));
        assert_eq!(not_a_bundle, None);
    }
}
