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

/// A stable app carrying a dev name would touch the Dev app's folder; one without the release key refuses every
/// update, and one holding the dev builds' extra-key variable could take an update it should refuse.
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
        // A key reads "ssh-ed25519 <body> <comment>", and the body is what the binary must hold.
        let mut fields = key.split_whitespace();
        let Some(body) = fields.nth(1) else {
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
    use std::path::PathBuf;

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

    // A bundle whose Info.plist names `bundle_id` and whose app program holds `app_text`; the other two hold nothing.
    fn fake_bundle(folder: &Path, bundle_id: &str, app_text: &str) -> PathBuf {
        let app = folder.join("Fake.app");
        let programs = app.join(PROGRAMS_PATH);
        fs::create_dir_all(&programs).unwrap();
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{bundle_id}</string></dict></plist>\n"
        );
        fs::write(app.join(INFO_PLIST_PATH), plist).unwrap();
        fs::write(programs.join(APP_PROGRAM), app_text).unwrap();
        fs::write(programs.join(RUNTIME_PROGRAM), "").unwrap();
        fs::write(programs.join(PLUGIN_PROGRAM), "").unwrap();
        app
    }

    #[test]
    fn a_stable_bundle_needs_the_stable_id_every_release_key_and_no_dev_name() {
        let all_keys = RELEASE_KEYS.join("\n");
        let update_key = UPDATE_KEY_VARIABLE.unwrap();
        let cases = [
            (
                "a good stable bundle",
                STABLE_BUNDLE_ID,
                all_keys.clone(),
                true,
            ),
            ("the dev bundle id", DEV_BUNDLE_ID, all_keys.clone(), false),
            ("no release key", STABLE_BUNDLE_ID, String::new(), false),
            (
                "the dev app's name",
                STABLE_BUNDLE_ID,
                format!("{all_keys}\n{DEV_APPLICATION_NAME}"),
                false,
            ),
            (
                "the extra-key variable",
                STABLE_BUNDLE_ID,
                format!("{all_keys}\n{update_key}"),
                false,
            ),
        ];
        for (index, (name, bundle_id, app_text, expected)) in cases.into_iter().enumerate() {
            let folder = std::env::temp_dir()
                .join(format!("taco-bundle-test-{}-{index}", std::process::id()));
            let app = fake_bundle(&folder, bundle_id, &app_text);
            let checked = holds_only_stable_names_and_the_release_key(&app);
            let _ = fs::remove_dir_all(&folder);
            assert_eq!(checked.is_ok(), expected, "{name}: {checked:?}");
        }
    }
}
