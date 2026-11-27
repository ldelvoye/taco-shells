use std::path::Path;

use crate::agent_flow::{
    Agent, label_of, plugin_dirs_reading, read_settings, settings_text, write_settings,
};
use crate::agent_setup_state::{PluginDirs, shown_with_tilde};
use crate::plugin_dirs::{
    Appended, PluginDirsRecord, append_folder, read_record, remove_record, restore_value,
    write_record,
};
use crate::settings_edit::HookEdit;

pub(crate) fn hides_plugin_folder(agent: &Agent) -> bool {
    let text = settings_text(&agent.paths.settings_path);
    let reading = plugin_dirs_reading(agent, text.as_deref());
    matches!(reading, PluginDirs::Hides { .. })
}

fn not_added_problem(file: &str, reason: &str) -> String {
    format!("This app's plugin folder could not be added to {file}: {reason}")
}

fn not_taken_out_problem(file: &str, reason: &str) -> String {
    format!("This app's plugin folder could not be taken out of {file}: {reason}")
}

/// The settings file the folder was added to, shown with `~`, or none when nothing needed writing.
pub(crate) fn add_plugin_folder(
    agent: &Agent,
    config_directory: &Path,
) -> Result<Option<String>, String> {
    let file = label_of(agent);
    let read = read_settings(&agent.paths.settings_path);
    let found = read.map_err(|reason| not_added_problem(&file, &reason))?;
    let Some(text) = found else {
        return Ok(None);
    };
    let folder = &agent.paths.plugin_folder;
    let home = &agent.paths.home;
    let variable = agent.setup.plugin_variable;
    let edit = append_folder(&text, folder, home, &file, variable);
    let (changed, before, after) = match edit {
        Appended::Changed {
            text,
            before,
            after,
        } => (text, before, after),
        Appended::Unchanged => return Ok(None),
        Appended::Refused(reason) => return Err(not_added_problem(&file, &reason)),
    };
    let record = PluginDirsRecord {
        settings: agent.paths.settings_path.clone(),
        before,
        after,
    };
    let recorded = write_record(config_directory, &record);
    recorded.map_err(|error| not_added_problem(&file, &error.to_string()))?;
    let original = Some(text.as_str());
    let written = write_settings(&agent.paths.settings_path, original, &changed);
    if let Err(reason) = written {
        let _ = remove_record(config_directory);
        return Err(not_added_problem(&file, &reason));
    }
    let shown = shown_with_tilde(&agent.paths.settings_path, home);
    Ok(Some(shown))
}

pub(crate) fn remove_plugin_folder(config_directory: &Path, variable: &str) -> Result<(), String> {
    let Some(record) = read_record(config_directory) else {
        return Ok(());
    };
    let file = record.settings.display().to_string();
    let read = read_settings(&record.settings);
    let found = read.map_err(|reason| not_taken_out_problem(&file, &reason))?;
    let mut written = Ok(());
    if let Some(text) = found {
        let edit = restore_value(&text, &record.before, &record.after, &file, variable);
        match edit {
            HookEdit::Changed(changed) => {
                written = write_settings(&record.settings, Some(text.as_str()), &changed);
            }
            HookEdit::Unchanged => {}
            HookEdit::Refused(reason) => return Err(not_taken_out_problem(&file, &reason)),
        }
    }
    written.map_err(|reason| not_taken_out_problem(&file, &reason))?;
    let removed = remove_record(config_directory);
    removed.map_err(|error| not_taken_out_problem(&file, &error.to_string()))
}
