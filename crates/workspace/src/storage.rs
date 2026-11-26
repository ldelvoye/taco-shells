use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use taco_protocol::TerminalId;

use crate::save::{ELECTRON_VERSION, SAVE_VERSION, read_saved, version_of};

const SAVE_FILE: &str = "workspace.json";
const TEMPORARY_FILE: &str = "workspace.json.tmp";
/// Where the Electron app's save is kept when the Rust app replaces it.
pub const ELECTRON_COPY_FILE: &str = "workspace.electron.json";
/// Where a save this build cannot read, such as one from a newer build, is kept when it is replaced.
pub const UNREADABLE_COPY_FILE: &str = "workspace.unreadable.json";
/// The folder of the config directory that holds one Agent Activity log per terminal.
pub const ACTIVITY_DIRECTORY: &str = "activity";
const LOG_SUFFIX: &str = ".jsonl";

/// Where the workspace keeps what outlives a launch.
pub trait Storage: Send {
    /// The save file's text, or none when there is none to read.
    fn read_save(&self) -> Option<String>;
    /// Replaces the save file, and says whether it did; the workspace tries a failed save again.
    fn write_save(&mut self, text: &str) -> bool;
    /// The lines of a terminal's activity log, oldest first, or none when it has no log.
    fn read_log(&self, terminal: &TerminalId) -> Vec<String>;
    fn append_log(&mut self, terminal: &TerminalId, line: &str);
    /// Replaces a terminal's log with these lines.
    fn rewrite_log(&mut self, terminal: &TerminalId, lines: &[String]);
    fn delete_log(&mut self, terminal: &TerminalId);
    /// Every terminal that has a log.
    fn logs(&self) -> Vec<TerminalId>;
}

/// Storage in the app's config directory.
pub struct FileStorage {
    directory: PathBuf,
}

/// The file name a terminal's log has, or none for an id that is not safe as one, which then has no log on disk.
fn log_file_name(terminal: &TerminalId) -> Option<String> {
    let TerminalId(id) = terminal;
    let safe = id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    if id.is_empty() || !safe {
        return None;
    }
    Some(format!("{id}{LOG_SUFFIX}"))
}

/// The copy a file this build did not write needs before it is replaced, or none for our own save.
fn copy_for(replaced: &[u8]) -> Option<&'static str> {
    let Ok(text) = std::str::from_utf8(replaced) else {
        return Some(UNREADABLE_COPY_FILE);
    };
    let version = version_of(text);
    if version == Some(ELECTRON_VERSION) {
        return Some(ELECTRON_COPY_FILE);
    }
    let ours = version == Some(SAVE_VERSION) && read_saved(text).is_some();
    if ours {
        return None;
    }
    Some(UNREADABLE_COPY_FILE)
}

impl FileStorage {
    pub fn new(directory: PathBuf) -> Self {
        FileStorage { directory }
    }

    fn log_path(&self, terminal: &TerminalId) -> Option<PathBuf> {
        let name = log_file_name(terminal)?;
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        Some(folder.join(name))
    }
}

impl Storage for FileStorage {
    fn read_save(&self) -> Option<String> {
        let path = self.directory.join(SAVE_FILE);
        fs::read_to_string(path).ok()
    }

    fn write_save(&mut self, text: &str) -> bool {
        let path = self.directory.join(SAVE_FILE);
        if let Ok(replaced) = fs::read(&path)
            && let Some(copy_name) = copy_for(&replaced)
        {
            let copy = self.directory.join(copy_name);
            // the original stays until its copy is safe
            if fs::write(copy, &replaced).is_err() {
                return false;
            }
        }
        let temporary = self.directory.join(TEMPORARY_FILE);
        if fs::write(&temporary, text).is_err() {
            return false;
        }
        // the rename makes the write whole or nothing, so a quit mid-write never leaves half a file
        fs::rename(&temporary, path).is_ok()
    }

    fn read_log(&self, terminal: &TerminalId) -> Vec<String> {
        let Some(path) = self.log_path(terminal) else {
            return Vec::new();
        };
        let Ok(text) = fs::read_to_string(path) else {
            return Vec::new();
        };
        let lines = text.lines().map(str::to_string);
        lines.collect()
    }

    fn append_log(&mut self, terminal: &TerminalId, line: &str) {
        let Some(path) = self.log_path(terminal) else {
            return;
        };
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        let _ = fs::create_dir_all(folder);
        let opened = OpenOptions::new().create(true).append(true).open(path);
        if let Ok(mut file) = opened {
            let _ = writeln!(file, "{line}");
        }
    }

    fn rewrite_log(&mut self, terminal: &TerminalId, lines: &[String]) {
        let Some(path) = self.log_path(terminal) else {
            return;
        };
        let temporary = path.with_extension("jsonl.tmp");
        let mut text = lines.join("\n");
        text.push('\n');
        if fs::write(&temporary, text).is_ok() {
            let _ = fs::rename(&temporary, path);
        }
    }

    fn delete_log(&mut self, terminal: &TerminalId) {
        if let Some(path) = self.log_path(terminal) {
            let _ = fs::remove_file(path);
        }
    }

    fn logs(&self) -> Vec<TerminalId> {
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        let Ok(entries) = fs::read_dir(folder) else {
            return Vec::new();
        };
        let names = entries.filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let id = name.strip_suffix(LOG_SUFFIX)?;
            Some(TerminalId(id.to_string()))
        });
        names.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &str = r#"{"version":2,"rows":[{}]}"#;

    #[test]
    fn a_file_this_build_did_not_write_is_kept_beside_the_new_save_before_it_is_replaced() {
        let directory = tempfile::tempdir().unwrap();
        let save = directory.path().join(SAVE_FILE);
        let mut storage = FileStorage::new(directory.path().to_path_buf());
        assert_eq!(storage.read_save(), None);
        assert!(storage.write_save(OURS));

        let electron = r#"{"version":1,"groups":[]}"#;
        fs::write(&save, electron).unwrap();
        assert!(storage.write_save(OURS));
        let electron_copy = directory.path().join(ELECTRON_COPY_FILE);
        assert_eq!(fs::read_to_string(&electron_copy).unwrap(), electron);
        assert_eq!(storage.read_save().as_deref(), Some(OURS));

        let unreadable_copy = directory.path().join(UNREADABLE_COPY_FILE);
        assert!(storage.write_save(OURS));
        assert!(!unreadable_copy.exists(), "our own save is not copied");
        for unreadable in [r#"{"version":3,"rows":[]}"#, "not json"] {
            fs::write(&save, unreadable).unwrap();
            assert!(storage.write_save(OURS));
            assert_eq!(fs::read_to_string(&unreadable_copy).unwrap(), unreadable);
        }

        // a copy that cannot be written leaves the original where it was
        fs::write(&save, electron).unwrap();
        fs::remove_file(&electron_copy).unwrap();
        fs::create_dir(&electron_copy).unwrap();
        assert!(!storage.write_save(OURS));
        assert_eq!(fs::read_to_string(&save).unwrap(), electron);

        let mut gone = FileStorage::new(directory.path().join("missing"));
        assert!(!gone.write_save("{}"), "a failed write says so");
    }

    #[test]
    fn a_log_is_a_file_per_terminal_and_an_id_unsafe_as_a_file_name_has_none() {
        let directory = tempfile::tempdir().unwrap();
        let mut storage = FileStorage::new(directory.path().to_path_buf());
        let terminal = TerminalId("3fa9c1d2".into());

        storage.append_log(&terminal, "one");
        storage.append_log(&terminal, "two");
        assert_eq!(storage.read_log(&terminal), vec!["one", "two"]);
        storage.rewrite_log(&terminal, &["two".to_string()]);
        assert_eq!(storage.read_log(&terminal), vec!["two"]);
        assert_eq!(storage.logs(), vec![terminal.clone()]);

        let escaping = TerminalId("../../workspace".into());
        storage.append_log(&escaping, "lost");
        assert!(storage.read_log(&escaping).is_empty());
        let outside = directory.path().parent().unwrap().join("workspace.jsonl");
        assert!(
            !outside.exists(),
            "nothing is written outside the activity folder"
        );

        storage.delete_log(&terminal);
        assert!(storage.logs().is_empty());
    }
}
