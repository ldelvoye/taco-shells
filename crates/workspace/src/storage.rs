use std::fs;
use std::path::PathBuf;

use crate::save::{ELECTRON_VERSION, version_of};

const SAVE_FILE: &str = "workspace.json";
const TEMPORARY_FILE: &str = "workspace.json.tmp";
/// Where the Electron app's last save is kept when the Rust app first replaces it.
pub const ELECTRON_COPY_FILE: &str = "workspace.electron.json";

/// Where the workspace keeps what outlives a launch.
pub trait Storage: Send {
    /// The save file's text, or none when there is none to read.
    fn read_save(&self) -> Option<String>;
    /// Replaces the save file, and says whether it did; the workspace tries a failed save again.
    fn write_save(&mut self, text: &str) -> bool;
}

/// Storage in the app's config directory.
pub struct FileStorage {
    directory: PathBuf,
}

impl FileStorage {
    pub fn new(directory: PathBuf) -> Self {
        FileStorage { directory }
    }
}

impl Storage for FileStorage {
    fn read_save(&self) -> Option<String> {
        let path = self.directory.join(SAVE_FILE);
        fs::read_to_string(path).ok()
    }

    fn write_save(&mut self, text: &str) -> bool {
        let path = self.directory.join(SAVE_FILE);
        if let Some(replaced) = self.read_save()
            && version_of(&replaced) == Some(ELECTRON_VERSION)
        {
            let copy = self.directory.join(ELECTRON_COPY_FILE);
            let _ = fs::write(copy, replaced);
        }
        let temporary = self.directory.join(TEMPORARY_FILE);
        if fs::write(&temporary, text).is_err() {
            return false;
        }
        // the rename makes the write whole or nothing, so a quit mid-write never leaves half a file
        fs::rename(&temporary, path).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_reads_back_and_the_electron_file_it_replaces_is_kept_beside_it() {
        let directory = tempfile::tempdir().unwrap();
        let mut storage = FileStorage::new(directory.path().to_path_buf());
        assert_eq!(storage.read_save(), None);

        let electron = r#"{"version":1,"groups":[]}"#;
        fs::write(directory.path().join(SAVE_FILE), electron).unwrap();
        storage.write_save(r#"{"version":2,"rows":[]}"#);
        let copy = directory.path().join(ELECTRON_COPY_FILE);
        assert_eq!(fs::read_to_string(&copy).unwrap(), electron);

        storage.write_save(r#"{"version":2,"rows":[{}]}"#);
        assert_eq!(
            storage.read_save().as_deref(),
            Some(r#"{"version":2,"rows":[{}]}"#)
        );
        assert_eq!(
            fs::read_to_string(&copy).unwrap(),
            electron,
            "only a version 1 file is copied"
        );

        let mut gone = FileStorage::new(directory.path().join("missing"));
        assert!(!gone.write_save("{}"), "a failed write says so");
    }
}
