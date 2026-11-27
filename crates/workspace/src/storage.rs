use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use taco_protocol::TerminalId;

use crate::save::{SAVE_VERSION, read_saved, version_of};

const SAVE_FILE: &str = "workspace.json";
const TEMPORARY_FILE: &str = "workspace.json.tmp";
/// Where a save this build cannot read, such as one from a newer build, is kept when it is replaced.
pub const UNREADABLE_COPY_FILE: &str = "workspace.unreadable.json";
/// The folder of the config directory that holds one Agent Activity log per terminal.
pub const ACTIVITY_DIRECTORY: &str = "activity";
const LOG_SUFFIX: &str = ".jsonl";
const LOG_TEMPORARY_EXTENSION: &str = "jsonl.tmp";
const STATE_SUFFIX: &str = ".state.json";
const STATE_TEMPORARY_EXTENSION: &str = "json.tmp";
const ACTIVITY_SUFFIXES: [&str; 2] = [LOG_SUFFIX, STATE_SUFFIX];
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;
#[cfg(unix)]
const PRIVATE_FOLDER_MODE: u32 = 0o700;

/// Where the workspace keeps what outlives a launch.
pub trait Storage: Send {
    /// The save file's text, or none when there is none to read.
    fn read_save(&self) -> Option<String>;
    /// Replaces the save file, and says whether it did; the workspace tries a failed save again.
    fn write_save(&mut self, text: &str) -> bool;
    /// The lines of a terminal's activity log, oldest first; empty when it has no log.
    fn read_log(&self, terminal: &TerminalId) -> Vec<String>;
    /// Adds a line to a terminal's log, and says whether it did; the workspace rewrites a log whose append failed.
    fn append_log(&mut self, terminal: &TerminalId, line: &str) -> bool;
    /// Replaces a terminal's log with these lines, and says whether it did.
    fn rewrite_log(&mut self, terminal: &TerminalId, lines: &[String]) -> bool;
    fn delete_log(&mut self, terminal: &TerminalId);
    /// The text of a terminal's activity state file, which holds its now line and Shared, or none when it has none.
    fn read_state(&self, terminal: &TerminalId) -> Option<String>;
    /// Replaces a terminal's state file, and says whether it did.
    fn write_state(&mut self, terminal: &TerminalId, text: &str) -> bool;
    fn delete_state(&mut self, terminal: &TerminalId);
    /// Every terminal that has a log or a state file.
    fn activity_terminals(&self) -> Vec<TerminalId>;
}

/// Storage in the app's config directory.
pub struct FileStorage {
    directory: PathBuf,
}

/// A terminal's activity file name with this suffix, or none for an id that is unsafe as a file name.
fn activity_file_name(terminal: &TerminalId, suffix: &str) -> Option<String> {
    let TerminalId(id) = terminal;
    let safe = id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    if id.is_empty() || !safe {
        return None;
    }
    Some(format!("{id}{suffix}"))
}

fn replace_privately(path: &Path, temporary: &Path, text: &str) -> bool {
    // a file's mode is set only when it is created, so a temporary file left by a crash goes first
    let _ = fs::remove_file(temporary);
    let mut writing = private_file();
    writing.write(true).create_new(true);
    let Ok(mut file) = writing.open(temporary) else {
        return false;
    };
    if file.write_all(text.as_bytes()).is_err() {
        return false;
    }
    let renamed = fs::rename(temporary, path).is_ok();
    if !renamed {
        let _ = fs::remove_file(temporary);
    }
    renamed
}

// A log holds what the user typed to an agent and the commands it ran, and the save its folders and session ids, so on
// Unix only their own account can read them.
fn private_file() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(PRIVATE_FILE_MODE);
    }
    options
}

fn create_private_folder(folder: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(PRIVATE_FOLDER_MODE);
    }
    builder.create(folder)
}

fn ends_a_line(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return true;
    };
    if file.seek(SeekFrom::End(-1)).is_err() {
        return true;
    }
    let mut last = [0u8; 1];
    let read = file.read_exact(&mut last);
    read.is_err() || last[0] == b'\n'
}

/// The copy a file this build did not write needs before it is replaced, or none for our own save.
fn copy_for(replaced: &[u8]) -> Option<&'static str> {
    let Ok(text) = std::str::from_utf8(replaced) else {
        return Some(UNREADABLE_COPY_FILE);
    };
    let version = version_of(text);
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

    fn activity_path(&self, terminal: &TerminalId, suffix: &str) -> Option<PathBuf> {
        let name = activity_file_name(terminal, suffix)?;
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        Some(folder.join(name))
    }

    fn log_path(&self, terminal: &TerminalId) -> Option<PathBuf> {
        self.activity_path(terminal, LOG_SUFFIX)
    }

    fn state_path(&self, terminal: &TerminalId) -> Option<PathBuf> {
        self.activity_path(terminal, STATE_SUFFIX)
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
        // the rename makes the write whole or nothing, so a quit mid-write never leaves half a file
        replace_privately(&path, &temporary, text)
    }

    // Read as bytes, so a crash that cut a character in half costs that line and not the whole log.
    fn read_log(&self, terminal: &TerminalId) -> Vec<String> {
        let Some(path) = self.log_path(terminal) else {
            return Vec::new();
        };
        let Ok(bytes) = fs::read(path) else {
            return Vec::new();
        };
        let lines = bytes.split(|byte| *byte == b'\n');
        let texts = lines.filter_map(|line| std::str::from_utf8(line).ok());
        let kept = texts.filter(|text| !text.is_empty()).map(str::to_string);
        kept.collect()
    }

    fn append_log(&mut self, terminal: &TerminalId, line: &str) -> bool {
        let Some(path) = self.log_path(terminal) else {
            return false;
        };
        let mut appending = private_file();
        appending.create(true).append(true);
        let mut opened = appending.open(&path);
        if opened.is_err() {
            let folder = self.directory.join(ACTIVITY_DIRECTORY);
            let _ = create_private_folder(&folder);
            opened = appending.open(&path);
        }
        let Ok(mut file) = opened else {
            return false;
        };
        // a line a crash cut short is ended first, so the new entry does not join it
        let mut separated = String::new();
        if !ends_a_line(&path) {
            separated.push('\n');
        }
        separated.push_str(line);
        separated.push('\n');
        file.write_all(separated.as_bytes()).is_ok()
    }

    fn rewrite_log(&mut self, terminal: &TerminalId, lines: &[String]) -> bool {
        let Some(path) = self.log_path(terminal) else {
            return false;
        };
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        let _ = create_private_folder(&folder);
        let temporary = path.with_extension(LOG_TEMPORARY_EXTENSION);
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        replace_privately(&path, &temporary, &text)
    }

    fn delete_log(&mut self, terminal: &TerminalId) {
        if let Some(path) = self.log_path(terminal) {
            let _ = fs::remove_file(path);
        }
    }

    fn read_state(&self, terminal: &TerminalId) -> Option<String> {
        let path = self.state_path(terminal)?;
        fs::read_to_string(path).ok()
    }

    fn write_state(&mut self, terminal: &TerminalId, text: &str) -> bool {
        let Some(path) = self.state_path(terminal) else {
            return false;
        };
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        let _ = create_private_folder(&folder);
        let temporary = path.with_extension(STATE_TEMPORARY_EXTENSION);
        replace_privately(&path, &temporary, text)
    }

    fn delete_state(&mut self, terminal: &TerminalId) {
        if let Some(path) = self.state_path(terminal) {
            let _ = fs::remove_file(path);
        }
    }

    fn activity_terminals(&self) -> Vec<TerminalId> {
        let folder = self.directory.join(ACTIVITY_DIRECTORY);
        let Ok(entries) = fs::read_dir(folder) else {
            return Vec::new();
        };
        let names = entries.filter_map(|entry| entry.ok()?.file_name().into_string().ok());
        let mut terminals = BTreeSet::new();
        for name in names {
            for suffix in ACTIVITY_SUFFIXES {
                if let Some(id) = name.strip_suffix(suffix) {
                    terminals.insert(TerminalId(id.to_string()));
                }
            }
        }
        terminals.into_iter().collect()
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&save).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "readable by the user's own account only");
        }

        let unreadable_copy = directory.path().join(UNREADABLE_COPY_FILE);
        assert!(storage.write_save(OURS));
        assert!(!unreadable_copy.exists(), "our own save is not copied");
        let electron = r#"{"version":1,"groups":[]}"#;
        let no_rows = r#"{"version":2}"#;
        for unreadable in [electron, no_rows, r#"{"version":3,"rows":[]}"#, "not json"] {
            fs::write(&save, unreadable).unwrap();
            assert!(storage.write_save(OURS));
            assert_eq!(fs::read_to_string(&unreadable_copy).unwrap(), unreadable);
        }
        assert_eq!(storage.read_save().as_deref(), Some(OURS));

        // a copy that cannot be written leaves the original where it was
        fs::write(&save, electron).unwrap();
        fs::remove_file(&unreadable_copy).unwrap();
        fs::create_dir(&unreadable_copy).unwrap();
        assert!(!storage.write_save(OURS));
        assert_eq!(fs::read_to_string(&save).unwrap(), electron);

        let mut gone = FileStorage::new(directory.path().join("missing"));
        assert!(!gone.write_save("{}"), "a failed write says so");
    }

    #[test]
    fn a_log_is_a_file_per_terminal_and_an_id_unsafe_as_a_file_name_has_none() {
        let directory = tempfile::tempdir().unwrap();
        let mut storage = FileStorage::new(directory.path().to_path_buf());
        let terminal = TerminalId("3fa9c1d2-b_7".into());
        let file = directory
            .path()
            .join(ACTIVITY_DIRECTORY)
            .join("3fa9c1d2-b_7.jsonl");

        assert!(storage.append_log(&terminal, "one"));
        #[cfg(unix)]
        let modes = || {
            use std::os::unix::fs::PermissionsExt;
            let folder = fs::metadata(file.parent().unwrap()).unwrap();
            let log = fs::metadata(&file).unwrap();
            (
                folder.permissions().mode() & 0o777,
                log.permissions().mode() & 0o777,
            )
        };
        #[cfg(unix)]
        assert_eq!(
            modes(),
            (0o700, 0o600),
            "readable by the user's own account only"
        );
        assert!(storage.rewrite_log(&terminal, &["two".to_string()]));
        #[cfg(unix)]
        assert_eq!(modes(), (0o700, 0o600), "and still after a rewrite");
        assert!(storage.append_log(&terminal, "three"));
        assert_eq!(storage.read_log(&terminal), vec!["two", "three"]);
        assert_eq!(storage.activity_terminals(), vec![terminal.clone()]);

        // a crash can cut the last line short, even inside a character
        let mut torn = fs::read(&file).unwrap();
        torn.extend_from_slice("{\"seq\":9,\"kind\":\"😀".as_bytes());
        torn.truncate(torn.len() - 2);
        fs::write(&file, torn).unwrap();
        assert!(storage.append_log(&terminal, "four"));
        assert_eq!(storage.read_log(&terminal), vec!["two", "three", "four"]);

        for unsafe_id in ["../escaped", ""] {
            let escaping = TerminalId(unsafe_id.into());
            assert!(!storage.append_log(&escaping, "lost"));
            assert!(storage.read_log(&escaping).is_empty());
        }
        assert!(!directory.path().join("escaped.jsonl").exists());

        storage.delete_log(&terminal);
        assert!(storage.activity_terminals().is_empty());
    }

    #[test]
    fn a_state_file_is_private_replaced_whole_and_listed_with_the_logs() {
        let directory = tempfile::tempdir().unwrap();
        let mut storage = FileStorage::new(directory.path().to_path_buf());
        let with_log = TerminalId("t1".into());
        let state_only = TerminalId("t2".into());
        let file = directory
            .path()
            .join(ACTIVITY_DIRECTORY)
            .join("t2.state.json");

        assert!(storage.append_log(&with_log, "one"));
        assert!(storage.write_state(&with_log, r#"{"now":null}"#));
        assert!(storage.write_state(&state_only, "first"));
        assert!(storage.write_state(&state_only, "second"));
        assert_eq!(storage.read_state(&state_only).as_deref(), Some("second"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "readable by the user's own account only");
        }
        assert_eq!(
            storage.activity_terminals(),
            vec![with_log.clone(), state_only.clone()],
            "each terminal once, with a log or only a state file"
        );

        assert!(!storage.write_state(&TerminalId("../escaped".into()), "lost"));
        storage.delete_state(&state_only);
        assert_eq!(storage.read_state(&state_only), None);
        assert_eq!(storage.activity_terminals(), vec![with_log]);
    }
}
