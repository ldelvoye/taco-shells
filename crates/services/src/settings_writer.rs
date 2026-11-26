use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use taco_settings::{SettingChange, set_top_level};

use crate::file_write::{real_file_of, replace_file};

const SETTINGS_LABEL: &str = "settings.json";

type AfterWrite = Box<dyn Fn() + Send + Sync>;
type OnRefused = Box<dyn Fn(&'static str, String) + Send + Sync>;

pub struct SettingsWriter {
    changes: Option<Sender<SettingChange>>,
    thread: Option<JoinHandle<()>>,
}

fn read_fresh(path: &Path) -> Result<Option<String>, String> {
    let read = fs::read_to_string(path);
    match read {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{SETTINGS_LABEL} could not be read: {error}")),
    }
}

fn apply(path: &Path, change: &SettingChange) -> Result<(), String> {
    change.check()?;
    let original = read_fresh(path)?;
    let key = change.key();
    let value = change.value();
    let text = set_top_level(original.as_deref(), key, &value, SETTINGS_LABEL)?;
    let real = real_file_of(path);
    let written = real.and_then(|real| replace_file(&real, &text));
    match written {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("{SETTINGS_LABEL} could not be written: {error}")),
    }
}

fn write_loop(
    path: PathBuf,
    changes: Receiver<SettingChange>,
    after_write: AfterWrite,
    on_refused: OnRefused,
) {
    // One change at a time, each reading the file fresh, so changes asked quickly land in the order asked.
    for change in changes {
        let applied = apply(&path, &change);
        match applied {
            Ok(()) => after_write(),
            Err(reason) => on_refused(change.key(), reason),
        }
    }
}

impl SettingsWriter {
    pub fn start(
        settings_path: PathBuf,
        after_write: AfterWrite,
        on_refused: OnRefused,
    ) -> SettingsWriter {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            write_loop(settings_path, receiver, after_write, on_refused);
        });
        SettingsWriter {
            changes: Some(sender),
            thread: Some(thread),
        }
    }

    pub fn write(&self, change: SettingChange) {
        if let Some(changes) = &self.changes {
            let _ = changes.send(change);
        }
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.changes = None;
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SettingsWriter {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    use taco_settings::{ShellRowName, ThemeChoice};
    use tempfile::TempDir;

    use super::*;

    const WAIT: Duration = Duration::from_secs(2);
    const ORIGINAL: &str = "{\n  // my note\n  \"futureThing\": 1,\n  \"fontSize\": 12\n}\n";

    enum Event {
        Written,
        Refused(&'static str),
    }

    fn start_writer(path: &Path) -> (SettingsWriter, Receiver<Event>) {
        let (events, heard) = mpsc::channel();
        let written = events.clone();
        let after_write = move || {
            let _ = written.send(Event::Written);
        };
        let on_refused = move |key: &'static str, _reason: String| {
            let _ = events.send(Event::Refused(key));
        };
        let writer = SettingsWriter::start(
            path.to_path_buf(),
            Box::new(after_write),
            Box::new(on_refused),
        );
        (writer, heard)
    }

    fn expect_written(heard: &Receiver<Event>) {
        let event = heard.recv_timeout(WAIT).unwrap();
        assert!(matches!(event, Event::Written));
    }

    fn expect_refused(heard: &Receiver<Event>, key: &str) {
        let event = heard.recv_timeout(WAIT).unwrap();
        let Event::Refused(refused) = event else {
            panic!("expected a refusal");
        };
        assert_eq!(refused, key);
    }

    #[test]
    fn writes_keep_the_rest_of_the_file_refusals_never_touch_it_and_a_symlink_is_followed() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, ORIGINAL).unwrap();
        let (writer, heard) = start_writer(&path);

        writer.write(SettingChange::FontSize(20.0));
        expect_written(&heard);
        let changed = fs::read_to_string(&path).unwrap();
        assert_eq!(changed, ORIGINAL.replace("12", "20"));

        fs::write(&path, ORIGINAL).unwrap();
        writer.write(SettingChange::FontSize(0.0));
        expect_refused(&heard, "fontSize");
        let rejected = fs::read_to_string(&path).unwrap();
        assert_eq!(rejected, ORIGINAL);

        let broken = "{ \"fontSize\": }";
        fs::write(&path, broken).unwrap();
        writer.write(SettingChange::Theme(ThemeChoice::Dark));
        expect_refused(&heard, "theme");
        let unparsed = fs::read_to_string(&path).unwrap();
        assert_eq!(unparsed, broken);

        fs::write(&path, ORIGINAL).unwrap();
        for size in [13.0, 14.0, 15.0, 16.0, 17.0] {
            writer.write(SettingChange::FontSize(size));
        }
        for _ in 0..5 {
            expect_written(&heard);
        }
        let ordered = fs::read_to_string(&path).unwrap();
        assert_eq!(ordered, ORIGINAL.replace("12", "17"));
        writer.stop();

        let config = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let target = elsewhere.path().join("real.json");
        fs::write(&target, "{}\n").unwrap();
        let link = config.path().join("settings.json");
        symlink(&target, &link).unwrap();
        let (writer, heard) = start_writer(&link);

        writer.write(SettingChange::ShellRowName(ShellRowName::Path));
        expect_written(&heard);

        let metadata = fs::symlink_metadata(&link).unwrap();
        assert!(metadata.file_type().is_symlink());
        let held = fs::read_to_string(&target).unwrap();
        assert!(held.contains("\"shellRowName\": \"path\""), "{held}");
        writer.stop();
    }
}
