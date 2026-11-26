use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use taco_protocol::ConfigFile;
use taco_settings::{ConfigTexts, DEFAULT_KEYBINDINGS_TEXT, DEFAULT_SETTINGS_TEXT};

const SETTINGS_FILE: &str = "settings.json";
const KEYBINDINGS_FILE: &str = "keybindings.json";
const FOCUS_PORT_FILE: &str = "focus-port";

pub fn config_file_path(directory: &Path, which: ConfigFile) -> PathBuf {
    let file_name = match which {
        ConfigFile::Settings => SETTINGS_FILE,
        ConfigFile::Keybindings => KEYBINDINGS_FILE,
    };
    directory.join(file_name)
}

pub fn focus_port_path(directory: &Path) -> PathBuf {
    directory.join(FOCUS_PORT_FILE)
}

fn read_config_file(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok()
}

pub fn read_config_texts(directory: &Path) -> ConfigTexts {
    let settings_path = config_file_path(directory, ConfigFile::Settings);
    let settings = read_config_file(&settings_path);
    let keybindings_path = config_file_path(directory, ConfigFile::Keybindings);
    let keybindings = read_config_file(&keybindings_path);
    ConfigTexts {
        settings,
        keybindings,
    }
}

fn seed_config_file(path: &Path, text: &str, problems: &mut Vec<String>) {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let opened = options.open(path);
    let mut file = match opened {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            return;
        }
        Err(error) => {
            problems.push(format!("{} could not be created: {error}", path.display()));
            return;
        }
    };

    let written = file.write_all(text.as_bytes());
    if let Err(error) = written {
        problems.push(format!("{} could not be created: {error}", path.display()));
    }
}

fn report_if_unreadable(path: &Path, problems: &mut Vec<String>) {
    let entry = fs::symlink_metadata(path);
    if entry.is_err() {
        return;
    }
    let existing = read_config_file(path);
    if existing.is_some() {
        return;
    }
    problems.push(format!("{} exists but could not be read", path.display()));
}

/// The config files that are there but cannot be read, such as a symlink to nothing.
pub fn unreadable_problems(directory: &Path) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    let settings_path = config_file_path(directory, ConfigFile::Settings);
    report_if_unreadable(&settings_path, &mut problems);
    let keybindings_path = config_file_path(directory, ConfigFile::Keybindings);
    report_if_unreadable(&keybindings_path, &mut problems);
    problems
}

pub fn seed_config_files(directory: &Path) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();

    let created = fs::create_dir_all(directory);
    if let Err(error) = created {
        problems.push(format!(
            "{} could not be created: {error}",
            directory.display()
        ));
        return problems;
    }

    let settings_path = config_file_path(directory, ConfigFile::Settings);
    seed_config_file(&settings_path, DEFAULT_SETTINGS_TEXT, &mut problems);
    let keybindings_path = config_file_path(directory, ConfigFile::Keybindings);
    seed_config_file(&keybindings_path, DEFAULT_KEYBINDINGS_TEXT, &mut problems);

    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeding_never_overwrites_and_a_file_that_cannot_be_read_is_reported() {
        let temp = tempfile::TempDir::new().unwrap();
        let directory = temp.path().join("existing");
        fs::create_dir(&directory).unwrap();
        let settings_path = directory.join(SETTINGS_FILE);
        fs::write(&settings_path, r#"{"fontSize": 20}"#).unwrap();
        let keybindings_path = directory.join(KEYBINDINGS_FILE);
        std::os::unix::fs::symlink(directory.join("missing-target"), &keybindings_path).unwrap();

        let problems = seed_config_files(&directory);

        let settings_after = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(settings_after, r#"{"fontSize": 20}"#);
        let link_metadata = fs::symlink_metadata(&keybindings_path).unwrap();
        assert!(link_metadata.file_type().is_symlink());
        assert!(fs::metadata(&keybindings_path).is_err());
        assert!(problems.is_empty());

        let unreadable = unreadable_problems(&directory);
        assert_eq!(unreadable.len(), 1);
        let keybindings_text = keybindings_path.display().to_string();
        assert!(unreadable[0].contains(&keybindings_text));

        fs::write(&settings_path, [0xff, 0xfe]).unwrap();
        let after_bad_save = unreadable_problems(&directory);
        assert_eq!(after_bad_save.len(), 2);

        let fresh = temp.path().join("fresh");
        let fresh_problems = seed_config_files(&fresh);

        assert!(fresh.join(SETTINGS_FILE).is_file());
        assert!(fresh.join(KEYBINDINGS_FILE).is_file());
        assert!(fresh_problems.is_empty());
    }
}
