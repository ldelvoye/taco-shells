use std::fs;
use std::path::Path;
use std::process::Command;

const DEV_BUNDLE_ID: &str = "com.ldelvoye.tacoshells.dev";
const EXECUTABLES: [&str; 2] = ["taco-shells", "taco-shells-runtime"];

// Each stable name is the start of its dev one, so it counts only where the dev ending does not follow.
const STABLE_NAMES: [(&str, &str); 2] = [
    ("com.ldelvoye.tacoshells", ".dev"),
    (".taco-shells", "-dev"),
];

/// A dev build that carried the stable app's id or config directory would read and write the app in daily use.
pub fn holds_only_dev_names(app: &Path) -> Result<(), String> {
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
        if let Some(name) = stable_name_in(&bytes) {
            return Err(format!("{executable} holds the stable app's {name}"));
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

fn stable_name_in(bytes: &[u8]) -> Option<&'static str> {
    for (name, dev_ending) in STABLE_NAMES {
        if appears_without(bytes, name.as_bytes(), dev_ending.as_bytes()) {
            return Some(name);
        }
    }
    None
}

fn appears_without(bytes: &[u8], name: &[u8], ending: &[u8]) -> bool {
    let mut start = 0;
    while let Some(offset) = find(&bytes[start..], name) {
        let end = start + offset + name.len();
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
    fn a_stable_name_counts_only_without_the_dev_ending() {
        let cases: [(&[u8], Option<&str>); 5] = [
            (
                b"..com.ldelvoye.tacoshells.dev..~/.taco-shells-dev/..",
                None,
            ),
            (
                b"..com.ldelvoye.tacoshells..",
                Some("com.ldelvoye.tacoshells"),
            ),
            (
                b"~/.taco-shells-dev ~/.taco-shellsTaco Shells",
                Some(".taco-shells"),
            ),
            (b"~/.taco-shells", Some(".taco-shells")),
            (b"", None),
        ];
        for (bytes, expected) in cases {
            let found = stable_name_in(bytes);
            assert_eq!(found, expected, "for {:?}", String::from_utf8_lossy(bytes));
        }
    }
}
