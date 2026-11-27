use std::fs;
use std::path::Path;
use std::process::Command;

use crate::Smoke;

const DEV_BUNDLE_ID: &str = "com.ldelvoye.tacoshells.dev";
const STABLE_BUNDLE_ID: &str = "com.ldelvoye.tacoshells";
const DEV_ID_ENDING: &str = ".dev";
const DEV_APPLICATION_NAME: &str = "Taco Shells Dev";
const APP_EXECUTABLE: &str = "taco-shells";
const EXECUTABLES: [&str; 3] = [APP_EXECUTABLE, "taco-shells-runtime", "taco-shells-plugin"];

/// A dev build that carried the stable app's identity would read and write the app in daily use.
///
/// `services` picks the whole identity, name and config directory, with one compile-time switch, so finding the dev
/// name in the app shows which side was built. The stable config directory is not searched for: `.taco-shells` also
/// forms wherever one string ends in a dot and the next begins with the app's name, and nothing marks where strings
/// meet in a binary.
pub fn holds_only_dev_names(smoke: &Smoke) -> Result<(), String> {
    let app = &smoke.app;
    let plist = app.join("Contents").join("Info.plist");
    let identifier = plist_value(&plist, "CFBundleIdentifier")?;
    if identifier != DEV_BUNDLE_ID {
        return Err(format!("its bundle id is {identifier}"));
    }

    let programs = app.join("Contents").join("MacOS");
    for executable in EXECUTABLES {
        let path = programs.join(executable);
        let bytes = fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        if holds_the_stable_id(&bytes) {
            return Err(format!(
                "{executable} holds the stable app's {STABLE_BUNDLE_ID}"
            ));
        }
        let is_app = executable == APP_EXECUTABLE;
        if is_app && find(&bytes, DEV_APPLICATION_NAME.as_bytes()).is_none() {
            return Err(format!(
                "{executable} holds no \"{DEV_APPLICATION_NAME}\", so it was not built as the dev app"
            ));
        }
    }
    Ok(())
}

fn plist_value(plist: &Path, key: &str) -> Result<String, String> {
    let mut plutil = Command::new("plutil");
    plutil.args(["-extract", key, "raw", "-o", "-"]);
    plutil.arg(plist);
    let output = plutil
        .output()
        .map_err(|error| format!("could not run plutil: {error}"))?;
    if !output.status.success() {
        return Err(format!("{} has no {key}", plist.display()));
    }
    let value = String::from_utf8_lossy(&output.stdout);
    Ok(value.trim().to_string())
}

// The stable id is the start of the dev one, so it counts only where the dev ending does not follow.
fn holds_the_stable_id(bytes: &[u8]) -> bool {
    let id = STABLE_BUNDLE_ID.as_bytes();
    let ending = DEV_ID_ENDING.as_bytes();
    let mut start = 0;
    while let Some(offset) = find(&bytes[start..], id) {
        let end = start + offset + id.len();
        let after = &bytes[end..];
        if !after.starts_with(ending) {
            return true;
        }
        start = end;
    }
    false
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let mut windows = haystack.windows(needle.len());
    windows.position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_stable_id_without_the_dev_ending_counts() {
        let cases: [(&[u8], bool); 5] = [
            (b"..com.ldelvoye.tacoshells.dev..", false),
            (b"..com.ldelvoye.tacoshells..", true),
            (
                b"com.ldelvoye.tacoshells.devcom.ldelvoye.tacoshellsTaco",
                true,
            ),
            (b"relied on for safety.taco-shells-runtime", false),
            (b"", false),
        ];
        for (bytes, expected) in cases {
            let found = holds_the_stable_id(bytes);
            assert_eq!(found, expected, "for {:?}", String::from_utf8_lossy(bytes));
        }
    }
}
