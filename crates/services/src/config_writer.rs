use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use taco_settings::{
    CommandId, KEYBINDINGS_FILE_NAME, KeymapChange, SETTINGS_FILE_NAME, SettingChange, edit_keymap,
    set_top_level,
};

use crate::file_write::{read_text, write_if_unchanged};

type AfterWrite = Box<dyn Fn() + Send + Sync>;
type OnRefused = Box<dyn Fn(Refusal) + Send + Sync>;

pub enum ConfigChange {
    Setting(SettingChange),
    Keymap(KeymapChange),
}

pub enum Refusal {
    Setting { key: &'static str, reason: String },
    Keymap { command: CommandId, reason: String },
}

pub struct ConfigWriter {
    changes: Option<Sender<ConfigChange>>,
    thread: Option<JoinHandle<()>>,
}

fn refuse_read_only(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::metadata(path);
    match metadata {
        Ok(found) if found.permissions().readonly() => {
            Err(format!("{label} is read-only, so nothing was changed."))
        }
        _ => Ok(()),
    }
}

fn edit_text(change: &ConfigChange, original: Option<&str>, label: &str) -> Result<String, String> {
    match change {
        ConfigChange::Setting(setting) => {
            let key = setting.key();
            let value = setting.value();
            set_top_level(original, key, &value, label)
        }
        ConfigChange::Keymap(keymap) => edit_keymap(original, keymap, label),
    }
}

fn apply(path: &Path, label: &str, change: &ConfigChange) -> Result<(), String> {
    if let ConfigChange::Setting(setting) = change {
        setting.check()?;
    }
    refuse_read_only(path, label)?;
    let original = read_text(path, label)?;
    let text = edit_text(change, original.as_deref(), label)?;
    let before = match &original {
        Some(read) => read.as_str(),
        None => "",
    };
    if text == before {
        return Ok(());
    }
    write_if_unchanged(path, label, original.as_deref(), &text)
}

fn file_for<'a>(
    change: &ConfigChange,
    settings_path: &'a Path,
    keybindings_path: &'a Path,
) -> (&'a Path, &'static str) {
    match change {
        ConfigChange::Setting(_) => (settings_path, SETTINGS_FILE_NAME),
        ConfigChange::Keymap(_) => (keybindings_path, KEYBINDINGS_FILE_NAME),
    }
}

fn refusal_of(change: &ConfigChange, reason: String) -> Refusal {
    match change {
        ConfigChange::Setting(setting) => Refusal::Setting {
            key: setting.key(),
            reason,
        },
        ConfigChange::Keymap(keymap) => Refusal::Keymap {
            command: keymap.command(),
            reason,
        },
    }
}

fn write_loop(
    settings_path: PathBuf,
    keybindings_path: PathBuf,
    changes: Receiver<ConfigChange>,
    after_write: AfterWrite,
    on_refused: OnRefused,
) {
    // One at a time, each reading the file fresh, so quick changes land in the order asked.
    for change in changes {
        let (path, label) = file_for(&change, &settings_path, &keybindings_path);
        let applied = apply(path, label, &change);
        match applied {
            Ok(()) => after_write(),
            Err(reason) => {
                let refusal = refusal_of(&change, reason);
                on_refused(refusal);
            }
        }
    }
}

impl ConfigWriter {
    pub fn start(
        settings_path: PathBuf,
        keybindings_path: PathBuf,
        after_write: AfterWrite,
        on_refused: OnRefused,
    ) -> ConfigWriter {
        let (sender, receiver) = mpsc::channel();
        let thread = thread::spawn(move || {
            write_loop(
                settings_path,
                keybindings_path,
                receiver,
                after_write,
                on_refused,
            );
        });
        ConfigWriter {
            changes: Some(sender),
            thread: Some(thread),
        }
    }

    pub fn write(&self, change: ConfigChange) {
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

impl Drop for ConfigWriter {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;
    use std::time::Duration;

    use taco_settings::{ShellRowName, ThemeChoice, parse_chord};
    use tempfile::TempDir;

    use super::*;

    const WAIT: Duration = Duration::from_secs(2);
    const ORIGINAL: &str = "{\n  // my note\n  \"futureThing\": 1,\n  \"fontSize\": 12\n}\n";
    const KEYS: &str = "{\n  // mine\n  \"cmd+d\": \"terminal.split\"\n}\n";

    enum Event {
        Written,
        Refused(Refusal),
    }

    fn start_writer(settings: &Path, keybindings: &Path) -> (ConfigWriter, Receiver<Event>) {
        let (events, heard) = mpsc::channel();
        let written = events.clone();
        let after_write = move || {
            let _ = written.send(Event::Written);
        };
        let on_refused = move |refusal: Refusal| {
            let _ = events.send(Event::Refused(refusal));
        };
        let writer = ConfigWriter::start(
            settings.to_path_buf(),
            keybindings.to_path_buf(),
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
        let Event::Refused(Refusal::Setting { key: refused, .. }) = event else {
            panic!("expected a setting refusal");
        };
        assert_eq!(refused, key);
    }

    fn expect_keymap_refused(heard: &Receiver<Event>, command: CommandId) -> String {
        let event = heard.recv_timeout(WAIT).unwrap();
        let Event::Refused(Refusal::Keymap {
            command: refused,
            reason,
        }) = event
        else {
            panic!("expected a keymap refusal");
        };
        assert!(refused == command);
        reason
    }

    fn keymap_add(id: &str, chord: &str) -> ConfigChange {
        let command = CommandId::from_id(id).unwrap();
        let chord = parse_chord(chord).unwrap();
        ConfigChange::Keymap(KeymapChange::Add { command, chord })
    }

    fn keymap_remove(id: &str, chord: &str) -> ConfigChange {
        let command = CommandId::from_id(id).unwrap();
        let chord = parse_chord(chord).unwrap();
        ConfigChange::Keymap(KeymapChange::Remove { command, chord })
    }

    fn font_size(size: f64) -> ConfigChange {
        ConfigChange::Setting(SettingChange::FontSize(size))
    }

    #[test]
    fn writes_keep_the_rest_of_the_file_refusals_never_touch_it_and_a_symlink_is_followed() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("settings.json");
        let keybindings = temp.path().join("keybindings.json");
        fs::write(&path, ORIGINAL).unwrap();
        let (writer, heard) = start_writer(&path, &keybindings);

        writer.write(font_size(20.0));
        expect_written(&heard);
        let changed = fs::read_to_string(&path).unwrap();
        assert_eq!(changed, ORIGINAL.replace("12", "20"));

        fs::write(&path, ORIGINAL).unwrap();
        writer.write(font_size(0.0));
        expect_refused(&heard, "fontSize");
        let rejected = fs::read_to_string(&path).unwrap();
        assert_eq!(rejected, ORIGINAL);

        let broken = "{ \"fontSize\": }";
        fs::write(&path, broken).unwrap();
        writer.write(ConfigChange::Setting(SettingChange::Theme(
            ThemeChoice::Dark,
        )));
        expect_refused(&heard, "theme");
        let unparsed = fs::read_to_string(&path).unwrap();
        assert_eq!(unparsed, broken);

        fs::write(&path, ORIGINAL).unwrap();
        for size in [13.0, 14.0, 15.0, 16.0, 17.0] {
            writer.write(font_size(size));
        }
        for _ in 0..5 {
            expect_written(&heard);
        }
        let ordered = fs::read_to_string(&path).unwrap();
        assert_eq!(ordered, ORIGINAL.replace("12", "17"));

        let mut read_only = fs::metadata(&path).unwrap().permissions();
        read_only.set_readonly(true);
        fs::set_permissions(&path, read_only).unwrap();
        writer.write(font_size(20.0));
        expect_refused(&heard, "fontSize");
        let protected = fs::read_to_string(&path).unwrap();
        assert_eq!(protected, ORIGINAL.replace("12", "17"));

        fs::remove_file(&path).unwrap();
        writer.write(font_size(20.0));
        expect_written(&heard);
        let created = fs::read_to_string(&path).unwrap();
        assert_eq!(created, "{\n  \"fontSize\": 20\n}\n");
        writer.stop();

        let config = TempDir::new().unwrap();
        let elsewhere = TempDir::new().unwrap();
        let target = elsewhere.path().join("real.json");
        fs::write(&target, "{}\n").unwrap();
        let link = config.path().join("settings.json");
        symlink(&target, &link).unwrap();
        let link_keybindings = config.path().join("keybindings.json");
        let (writer, heard) = start_writer(&link, &link_keybindings);

        let row_name = SettingChange::ShellRowName(ShellRowName::Path);
        writer.write(ConfigChange::Setting(row_name));
        expect_written(&heard);

        let metadata = fs::symlink_metadata(&link).unwrap();
        assert!(metadata.file_type().is_symlink());
        let held = fs::read_to_string(&target).unwrap();
        assert!(held.contains("\"shellRowName\": \"path\""), "{held}");
        writer.stop();
    }

    #[test]
    fn keymap_changes_keep_the_file_refusals_name_it_and_order_holds_across_both_files() {
        let temp = TempDir::new().unwrap();
        let settings = temp.path().join("settings.json");
        let keybindings = temp.path().join("keybindings.json");
        fs::write(&settings, ORIGINAL).unwrap();
        fs::write(&keybindings, KEYS).unwrap();
        let (writer, heard) = start_writer(&settings, &keybindings);
        let clear = CommandId::from_id("terminal.clear").unwrap();

        writer.write(keymap_add("terminal.clear", "cmd+u"));
        expect_written(&heard);
        let added = fs::read_to_string(&keybindings).unwrap();
        let expected =
            "{\n  // mine\n  \"cmd+d\": \"terminal.split\",\n  \"cmd+u\": \"terminal.clear\"\n}\n";
        assert_eq!(added, expected);

        writer.write(keymap_remove("terminal.clear", "cmd+j"));
        let reason = expect_keymap_refused(&heard, clear);
        let expected_reason =
            "keybindings.json: \"cmd+j\" is no longer a shortcut for terminal.clear";
        assert_eq!(reason, expected_reason);
        let untouched = fs::read_to_string(&keybindings).unwrap();
        assert_eq!(untouched, expected);

        writer.write(font_size(14.0));
        writer.write(keymap_remove("terminal.clear", "cmd+u"));
        writer.write(font_size(15.0));
        for _ in 0..3 {
            expect_written(&heard);
        }
        let settings_text = fs::read_to_string(&settings).unwrap();
        assert!(
            settings_text.contains("\"fontSize\": 15"),
            "{settings_text}"
        );
        let keys_text = fs::read_to_string(&keybindings).unwrap();
        assert!(!keys_text.contains("cmd+u"), "{keys_text}");

        fs::remove_file(&keybindings).unwrap();
        writer.write(keymap_add("terminal.clear", "cmd+k"));
        expect_written(&heard);
        assert!(!keybindings.exists());

        fs::write(&keybindings, KEYS).unwrap();
        let mut read_only = fs::metadata(&keybindings).unwrap().permissions();
        read_only.set_readonly(true);
        fs::set_permissions(&keybindings, read_only).unwrap();
        writer.write(keymap_add("terminal.clear", "cmd+u"));
        let blocked = expect_keymap_refused(&heard, clear);
        assert_eq!(
            blocked,
            "keybindings.json is read-only, so nothing was changed."
        );
        writer.stop();
    }
}
