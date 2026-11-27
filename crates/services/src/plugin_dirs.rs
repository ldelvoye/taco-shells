use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstObjectProp, CstRootNode};
use jsonc_parser::parse_to_serde_value;
use serde_json::{Value, json};
use taco_settings::JSONC_PARSE_OPTIONS;

use crate::agent_hooks::{HookEdit, properties_named, uneditable_problem, unreadable_problem};
use crate::agent_setup_state::PluginDirs;
use crate::file_write::replace_file;

pub(crate) const MANAGED_SETTINGS_PATH: &str =
    "/Library/Application Support/ClaudeCode/managed-settings.json";

const ENV_KEY: &str = "env";
const RECORD_FILE_NAME: &str = "claude-plugin-dirs.json";
const SETTINGS_FIELD: &str = "settings";
const BEFORE_FIELD: &str = "before";
const AFTER_FIELD: &str = "after";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PluginDirsRecord {
    pub(crate) settings: PathBuf,
    pub(crate) before: String,
    pub(crate) after: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Appended {
    Changed {
        text: String,
        before: String,
        after: String,
    },
    Unchanged,
    Refused(String),
}

enum Located {
    Missing,
    Ambiguous,
    Found(CstObjectProp),
}

fn plugin_dirs_value(text: &str, variable: &str) -> Option<String> {
    let parsed = parse_to_serde_value(text, &JSONC_PARSE_OPTIONS);
    let Ok(Some(Value::Object(root))) = parsed else {
        return None;
    };
    let env = root.get(ENV_KEY)?;
    let entry = env.get(variable)?;
    let value = entry.as_str()?;
    Some(value.to_string())
}

fn expand_home(entry: &str, home: &Path) -> String {
    let home_text = home.to_string_lossy();
    if entry == "~" {
        return home_text.into_owned();
    }
    if let Some(rest) = entry.strip_prefix("~/") {
        return format!("{home_text}/{rest}");
    }
    entry.to_string()
}

fn without_trailing_slash(text: &str) -> &str {
    match text.strip_suffix('/') {
        Some(trimmed) => trimmed,
        None => text,
    }
}

fn lists_folder(value: &str, folder: &Path, home: &Path) -> bool {
    let folder_text = folder.to_string_lossy();
    for entry in value.split(':') {
        let expanded = expand_home(entry, home);
        let trimmed = without_trailing_slash(&expanded);
        if trimmed == folder_text {
            return true;
        }
    }
    false
}

pub(crate) fn plugin_dirs_of(
    user_text: Option<&str>,
    user_file: &Path,
    managed_text: Option<&str>,
    folder: &Path,
    home: &Path,
    variable: &str,
) -> PluginDirs {
    let managed_value = managed_text.and_then(|text| plugin_dirs_value(text, variable));
    if let Some(value) = &managed_value
        && !lists_folder(value, folder, home)
    {
        return PluginDirs::ManagedHides;
    }
    let user_value = user_text.and_then(|text| plugin_dirs_value(text, variable));
    if let Some(value) = &user_value
        && !lists_folder(value, folder, home)
    {
        return PluginDirs::Hides {
            file: user_file.to_path_buf(),
        };
    }
    if managed_value.is_some() || user_value.is_some() {
        return PluginDirs::Lists;
    }
    PluginDirs::Unset
}

fn locate(root: &CstRootNode, variable: &str) -> Located {
    let Some(object) = root.object_value() else {
        return Located::Missing;
    };
    let env_count = properties_named(&object, ENV_KEY);
    if env_count == 0 {
        return Located::Missing;
    }
    if env_count > 1 {
        return Located::Ambiguous;
    }
    let Some(env) = object.object_value(ENV_KEY) else {
        return Located::Missing;
    };
    let variable_count = properties_named(&env, variable);
    if variable_count == 0 {
        return Located::Missing;
    }
    if variable_count > 1 {
        return Located::Ambiguous;
    }
    match env.get(variable) {
        Some(property) => Located::Found(property),
        None => Located::Missing,
    }
}

fn string_of(property: &CstObjectProp) -> Option<String> {
    let node = property.value()?;
    let literal = node.as_string_lit()?;
    literal.decoded_value().ok()
}

pub(crate) fn append_folder(
    text: &str,
    folder: &Path,
    home: &Path,
    label: &str,
    variable: &str,
) -> Appended {
    let parsed = CstRootNode::parse(text, &JSONC_PARSE_OPTIONS);
    let Ok(root) = parsed else {
        return Appended::Refused(unreadable_problem(label));
    };
    let property = match locate(&root, variable) {
        Located::Missing => return Appended::Unchanged,
        Located::Ambiguous => return Appended::Refused(uneditable_problem(label)),
        Located::Found(property) => property,
    };
    let Some(before) = string_of(&property) else {
        return Appended::Unchanged;
    };
    if lists_folder(&before, folder, home) {
        return Appended::Unchanged;
    }
    let folder_text = folder.to_string_lossy();
    let mut after = folder_text.to_string();
    if !before.is_empty() {
        after = format!("{before}:{folder_text}");
    }
    property.set_value(CstInputValue::from(after.clone()));
    Appended::Changed {
        text: root.to_string(),
        before,
        after,
    }
}

pub(crate) fn restore_value(
    text: &str,
    before: &str,
    after: &str,
    label: &str,
    variable: &str,
) -> HookEdit {
    let parsed = CstRootNode::parse(text, &JSONC_PARSE_OPTIONS);
    let Ok(root) = parsed else {
        return HookEdit::Refused(unreadable_problem(label));
    };
    let property = match locate(&root, variable) {
        Located::Missing => return HookEdit::Unchanged,
        Located::Ambiguous => return HookEdit::Refused(uneditable_problem(label)),
        Located::Found(property) => property,
    };
    let Some(current) = string_of(&property) else {
        return HookEdit::Unchanged;
    };
    if current != after {
        return HookEdit::Unchanged;
    }
    property.set_value(CstInputValue::from(before.to_string()));
    HookEdit::Changed(root.to_string())
}

fn record_path(config_directory: &Path) -> PathBuf {
    config_directory.join(RECORD_FILE_NAME)
}

pub(crate) fn write_record(config_directory: &Path, record: &PluginDirsRecord) -> io::Result<()> {
    fs::create_dir_all(config_directory)?;
    let body = json!({
        SETTINGS_FIELD: record.settings.to_string_lossy(),
        BEFORE_FIELD: record.before,
        AFTER_FIELD: record.after,
    });
    let text = body.to_string();
    let path = record_path(config_directory);
    replace_file(&path, &text)
}

pub(crate) fn read_record(config_directory: &Path) -> Option<PluginDirsRecord> {
    let path = record_path(config_directory);
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let settings_value = value.get(SETTINGS_FIELD)?;
    let settings = settings_value.as_str()?;
    let before_value = value.get(BEFORE_FIELD)?;
    let before = before_value.as_str()?;
    let after_value = value.get(AFTER_FIELD)?;
    let after = after_value.as_str()?;
    Some(PluginDirsRecord {
        settings: PathBuf::from(settings),
        before: before.to_string(),
        after: after.to_string(),
    })
}

pub(crate) fn remove_record(config_directory: &Path) -> io::Result<()> {
    let path = record_path(config_directory);
    let removed = fs::remove_file(path);
    match removed {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: &str = "settings.json";
    const FOLDER: &str = "/h/.taco-shells/claude-plugin";
    const USER_FILE: &str = r#"{
  // mine
  "model": "opus",
  "env": {
    "OTHER": "1", // keep
    "CLAUDE_CODE_PLUGIN_DIRS": "~/.claude/mods/face-band"
  },
  "theme": "dark"
}
"#;

    fn folder() -> PathBuf {
        PathBuf::from(FOLDER)
    }

    fn home() -> PathBuf {
        PathBuf::from("/h")
    }

    fn variable() -> &'static str {
        taco_agents::CLAUDE.setup().unwrap().plugin_variable
    }

    fn with_value(value: &str) -> String {
        let variable = variable();
        let key = format!("\"{variable}\": \"{value}\"");
        format!("{{\"env\": {{{key}}}}}")
    }

    #[test]
    fn a_listed_entry_matches_only_this_builds_folder() {
        let folder = folder();
        let home = home();
        let cases = [
            ("~/.taco-shells/claude-plugin", true),
            ("/a:~/.taco-shells/claude-plugin", true),
            ("~/.taco-shells/claude-plugin/", true),
            ("/h/.taco-shells/claude-plugin", true),
            ("~/.claude/mods/face-band", false),
            ("~/.taco-shells-dev/claude-plugin", false),
            ("", false),
        ];
        for (value, expected) in cases {
            assert_eq!(lists_folder(value, &folder, &home), expected, "{value}");
        }
    }

    #[test]
    fn a_managed_value_wins_and_a_user_value_that_lacks_the_folder_hides_it() {
        let folder = folder();
        let home = home();
        let file = PathBuf::from("/h/.claude/settings.json");
        let lacking = with_value("/other");
        let listing = with_value("/other:~/.taco-shells/claude-plugin");
        let no_env = "{\"model\": \"opus\"}";

        let variable = variable();

        let managed_over_user = plugin_dirs_of(
            Some(&listing),
            &file,
            Some(&lacking),
            &folder,
            &home,
            variable,
        );
        assert_eq!(managed_over_user, PluginDirs::ManagedHides);
        let hides = plugin_dirs_of(Some(&lacking), &file, None, &folder, &home, variable);
        assert_eq!(hides, PluginDirs::Hides { file: file.clone() });
        let lists = plugin_dirs_of(Some(&listing), &file, None, &folder, &home, variable);
        assert_eq!(lists, PluginDirs::Lists);
        let unset = plugin_dirs_of(Some(no_env), &file, None, &folder, &home, variable);
        assert_eq!(unset, PluginDirs::Unset);
    }

    #[test]
    fn appending_changes_only_the_one_string() {
        let folder = folder();
        let home = home();
        let variable = variable();

        let edit = append_folder(USER_FILE, &folder, &home, LABEL, variable);

        let expected = USER_FILE.replace("face-band\"", &format!("face-band:{FOLDER}\""));
        let before = "~/.claude/mods/face-band".to_string();
        let after = format!("{before}:{FOLDER}");
        assert_eq!(
            edit,
            Appended::Changed {
                text: expected,
                before,
                after
            }
        );
        let absent = "{\"env\": {\"OTHER\": \"1\"}}";
        assert_eq!(
            append_folder(absent, &folder, &home, LABEL, variable),
            Appended::Unchanged
        );
        let listed = with_value("~/.taco-shells/claude-plugin");
        assert_eq!(
            append_folder(&listed, &folder, &home, LABEL, variable),
            Appended::Unchanged
        );
        let number = format!("{{\"env\": {{\"{variable}\": 5}}}}");
        assert_eq!(
            append_folder(&number, &folder, &home, LABEL, variable),
            Appended::Unchanged
        );
        let key = format!("\"{variable}\"");
        let duplicated = format!("{{\"env\": {{{key}: \"/a\", {key}: \"/b\"}}}}");
        let ambiguous = append_folder(&duplicated, &folder, &home, LABEL, variable);
        assert!(matches!(ambiguous, Appended::Refused(_)));
        let restored = restore_value(&duplicated, "/a", "/b", LABEL, variable);
        assert!(matches!(restored, HookEdit::Refused(_)));
        let broken = append_folder("{ not json", &folder, &home, LABEL, variable);
        assert!(matches!(broken, Appended::Refused(_)));
    }

    #[test]
    fn restoring_puts_back_the_old_value_only_while_the_line_is_untouched() {
        let before = "~/.claude/mods/face-band";
        let after = format!("{before}:{FOLDER}");
        let appended = USER_FILE.replace("face-band\"", &format!("face-band:{FOLDER}\""));

        let variable = variable();

        let edit = restore_value(&appended, before, &after, LABEL, variable);

        assert_eq!(edit, HookEdit::Changed(USER_FILE.to_string()));
        let with_slash = appended.replace(FOLDER, &format!("{FOLDER}/"));
        let retyped = appended.replace(&after, &format!("{FOLDER}:{before}"));
        let other = appended.replace("face-band", "other");
        let absent = "{\"env\": {\"OTHER\": \"1\"}}";
        let number = format!("{{\"env\": {{\"{variable}\": 5}}}}");
        for changed in [with_slash.as_str(), &retyped, &other, absent, &number] {
            let edit = restore_value(changed, before, &after, LABEL, variable);
            assert_eq!(edit, HookEdit::Unchanged, "{changed}");
        }
        let broken = restore_value("{ not json", before, &after, LABEL, variable);
        assert!(matches!(broken, HookEdit::Refused(_)));
    }
}
