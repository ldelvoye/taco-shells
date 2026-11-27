use std::fs;
use std::path::Path;
use std::process::Command;

use taco_identity::{
    APP_PROGRAM, DEVELOPMENT, INFO_PLIST_PATH, PLUGIN_PROGRAM, PROGRAMS_PATH, RELEASE_KEYS,
    RUNTIME_PROGRAM, STABLE, UPDATE_KEY_VARIABLE,
};

const DEV_BUNDLE_ID: &str = DEVELOPMENT.bundle_id;
const STABLE_BUNDLE_ID: &str = STABLE.bundle_id;
const DEV_ID_ENDING: &str = ".dev";
const DEV_APPLICATION_NAME: &str = DEVELOPMENT.application_name;
const EXECUTABLES: [&str; 3] = [APP_PROGRAM, RUNTIME_PROGRAM, PLUGIN_PROGRAM];

/// A dev build carrying the stable identity would read and write the app in daily use. The stable config folder's
/// name is not scanned for: `.taco-shells` also forms where two unrelated strings meet in a binary.
pub fn holds_only_dev_names(app: &Path) -> Result<(), String> {
    let plist = app.join(INFO_PLIST_PATH);
    let identifier = plist_value(&plist, "CFBundleIdentifier")?;
    if identifier != DEV_BUNDLE_ID {
        return Err(format!("its bundle id is {identifier}"));
    }

    let programs = app.join(PROGRAMS_PATH);
    for executable in EXECUTABLES {
        let path = programs.join(executable);
        let bytes = read(&path)?;
        if holds_the_stable_id(&bytes) {
            return Err(format!(
                "{executable} holds the stable app's {STABLE_BUNDLE_ID}"
            ));
        }
        let is_app = executable == APP_PROGRAM;
        if is_app && find(&bytes, DEV_APPLICATION_NAME.as_bytes()).is_none() {
            return Err(format!(
                "{executable} holds no \"{DEV_APPLICATION_NAME}\", so it was not built as the dev app"
            ));
        }
    }
    Ok(())
}

/// A stable app carrying a dev name would touch the Dev app's folder, and one without the release key, or with the
/// dev builds' extra-key variable, would take an update it should refuse or refuse every one.
pub fn holds_only_stable_names_and_the_release_key(app: &Path) -> Result<(), String> {
    let plist = app.join(INFO_PLIST_PATH);
    let identifier = plist_value(&plist, "CFBundleIdentifier")?;
    if identifier != STABLE_BUNDLE_ID {
        return Err(format!("its bundle id is {identifier}"));
    }

    let mut unwanted = vec![DEV_BUNDLE_ID, DEV_APPLICATION_NAME];
    if let Some(variable) = UPDATE_KEY_VARIABLE {
        unwanted.push(variable);
    }
    let programs = app.join(PROGRAMS_PATH);
    for executable in EXECUTABLES {
        let bytes = read(&programs.join(executable))?;
        for name in &unwanted {
            if find(&bytes, name.as_bytes()).is_some() {
                return Err(format!("{executable} holds \"{name}\""));
            }
        }
    }

    let app_bytes = read(&programs.join(APP_PROGRAM))?;
    for key in RELEASE_KEYS {
        let fields: Vec<&str> = key.split(' ').collect();
        let Some(body) = fields.get(1) else {
            return Err(format!("the release key \"{key}\" has no key body"));
        };
        if find(&app_bytes, body.as_bytes()).is_none() {
            return Err(format!(
                "{APP_PROGRAM} does not hold the release key {body}, so it would refuse every update"
            ));
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))
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
        let cases: [(&[u8], bool); 3] = [
            (b"..com.ldelvoye.tacoshells.dev..", false),
            (b"..com.ldelvoye.tacoshells..", true),
            (
                b"com.ldelvoye.tacoshells.devcom.ldelvoye.tacoshellsTaco",
                true,
            ),
        ];
        for (bytes, expected) in cases {
            let found = holds_the_stable_id(bytes);
            assert_eq!(found, expected, "for {:?}", String::from_utf8_lossy(bytes));
        }
    }
}
