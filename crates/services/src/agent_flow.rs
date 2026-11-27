use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use taco_agents::{HARNESSES, HarnessSetup};
use taco_protocol::HarnessId;

use crate::agent_hooks::{HookEdit, has_managed_entries, remove_managed_entries};
use crate::agent_setup_state::{
    AgentSetupFacts, AgentSetupState, PluginDirs, agent_setup_state_of, shown_with_tilde,
};
use crate::config_files::plugin_directory_path;
use crate::file_write::{read_text, write_if_unchanged};
use crate::plugin_dirs::{
    Appended, MANAGED_SETTINGS_PATH, PluginDirsRecord, append_folder, plugin_dirs_of, read_record,
    remove_record, restore_value, write_record,
};

const HOOKS_DIRECTORY_NAME: &str = "hooks";
const SCRIPT_MODE: u32 = 0o755;
const HOOK_SCRIPT_NAMES: [&str; 2] = ["taco-shells-attention.sh", "taco-shells-claude-session.sh"];
const STUB_SCRIPT: &str = "#!/bin/sh\nexit 0\n";

static STUB_WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentPaths {
    pub(crate) hooks_directory: PathBuf,
    pub(crate) agent_directory: PathBuf,
    pub(crate) settings_path: PathBuf,
    pub(crate) managed_settings_path: PathBuf,
    pub(crate) plugin_folder: PathBuf,
    pub(crate) home: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct Agent {
    pub(crate) harness_id: HarnessId,
    pub(crate) setup: &'static HarnessSetup,
    pub(crate) paths: AgentPaths,
}

pub fn hooks_directory(config_directory: &Path) -> PathBuf {
    config_directory.join(HOOKS_DIRECTORY_NAME)
}

fn write_stub(target: &Path, name: &str) -> io::Result<()> {
    let process = std::process::id();
    let counter = STUB_WRITES.fetch_add(1, Ordering::Relaxed);
    let temporary_name = format!(".{name}.{process}.{counter}.taco-tmp");
    let temporary = target.with_file_name(temporary_name);
    let written = fs::write(&temporary, STUB_SCRIPT);
    let mode = fs::Permissions::from_mode(SCRIPT_MODE);
    let marked = written.and_then(|()| fs::set_permissions(&temporary, mode));
    let renamed = marked.and_then(|()| fs::rename(&temporary, target));
    if renamed.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    renamed
}

pub fn stub_hook_scripts(destination: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for name in HOOK_SCRIPT_NAMES {
        let target = destination.join(name);
        let stubbed = write_stub(&target, name);
        if let Err(error) = stubbed {
            let shown = target.display();
            problems.push(format!("{shown} could not be replaced: {error}"));
        }
    }
    problems
}

fn agent_paths_of(
    setup: &HarnessSetup,
    config_directory: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
) -> AgentPaths {
    let override_value = env(setup.config_directory_variable);
    let agent_directory = match override_value {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => home.join(setup.default_config_directory),
    };
    let settings_path = agent_directory.join(setup.settings_file);
    AgentPaths {
        hooks_directory: hooks_directory(config_directory),
        agent_directory,
        settings_path,
        managed_settings_path: PathBuf::from(MANAGED_SETTINGS_PATH),
        plugin_folder: plugin_directory_path(config_directory),
        home: home.to_path_buf(),
    }
}

pub(crate) fn agents_of(
    config_directory: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    home: &Path,
) -> Vec<Agent> {
    let mut agents = Vec::new();
    for harness in HARNESSES {
        let Some(setup) = harness.setup() else {
            continue;
        };
        let paths = agent_paths_of(setup, config_directory, env, home);
        agents.push(Agent {
            harness_id: harness.id(),
            setup,
            paths,
        });
    }
    agents
}

fn read_settings(path: &Path) -> Result<Option<String>, String> {
    let shown = path.display().to_string();
    read_text(path, &shown)
}

fn write_settings(path: &Path, original: Option<&str>, text: &str) -> Result<(), String> {
    let shown = path.display().to_string();
    write_if_unchanged(path, &shown, original, text)
}

fn write_edit(agent: &Agent, original: Option<&str>, edit: HookEdit) -> Result<(), String> {
    match edit {
        HookEdit::Changed(text) => write_settings(&agent.paths.settings_path, original, &text),
        HookEdit::Unchanged => Ok(()),
        HookEdit::Refused(problem) => Err(problem),
    }
}

fn label_of(agent: &Agent) -> String {
    agent.paths.settings_path.display().to_string()
}

fn plugin_dirs_reading(agent: &Agent, user_text: Option<&str>) -> PluginDirs {
    let managed_read = read_settings(&agent.paths.managed_settings_path);
    let managed_readable = managed_read.ok();
    let managed_text = managed_readable.flatten();
    plugin_dirs_of(
        user_text,
        &agent.paths.settings_path,
        managed_text.as_deref(),
        &agent.paths.plugin_folder,
        &agent.paths.home,
    )
}

pub(crate) fn read_state(
    agent: &Agent,
    activity_on: bool,
    version: Option<(u32, u32, u32)>,
) -> AgentSetupState {
    let installed = agent.paths.agent_directory.exists();
    let read = read_settings(&agent.paths.settings_path);
    let readable = read.ok();
    let text = readable.flatten();
    let has_old_entries = has_managed_entries(text.as_deref(), &agent.paths.hooks_directory);
    let plugin_dirs = plugin_dirs_reading(agent, text.as_deref());
    let facts = AgentSetupFacts {
        installed,
        activity_on,
        has_old_entries,
        plugin_dirs,
        home: agent.paths.home.clone(),
        version,
    };
    agent_setup_state_of(&facts)
}

pub(crate) fn hides_plugin_folder(agent: &Agent) -> bool {
    let read = read_settings(&agent.paths.settings_path);
    let readable = read.ok();
    let text = readable.flatten();
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
    let edit = append_folder(&text, folder, home, &file);
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

pub(crate) fn remove_plugin_folder(config_directory: &Path) -> Result<(), String> {
    let Some(record) = read_record(config_directory) else {
        return Ok(());
    };
    let file = record.settings.display().to_string();
    let read = read_settings(&record.settings);
    let found = read.map_err(|reason| not_taken_out_problem(&file, &reason))?;
    let mut written = Ok(());
    if let Some(text) = found {
        let edit = restore_value(&text, &record.before, &record.after, &file);
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

pub(crate) fn remove_old_setup(agent: &Agent) -> Result<(), String> {
    let read = read_settings(&agent.paths.settings_path)?;
    let Some(text) = read else {
        return Ok(());
    };
    let label = label_of(agent);
    let removed = remove_managed_entries(&text, &agent.paths.hooks_directory, &label);
    let original = Some(text.as_str());
    write_edit(agent, original, removed)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use serde_json::Value;
    use tempfile::TempDir;

    use crate::agent_hooks::electron_fixture::electron_settings;

    use super::*;

    struct Fixture {
        temp: TempDir,
        agent: Agent,
    }

    fn claude_setup() -> &'static HarnessSetup {
        taco_agents::CLAUDE.setup().unwrap()
    }

    fn fixture() -> Fixture {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("config");
        let home = temp.path().join("home");
        let agent_directory = temp.path().join("claude");
        fs::create_dir_all(&agent_directory).unwrap();
        let variable_name = claude_setup().config_directory_variable;
        let variable = agent_directory.display().to_string();
        let env = move |name: &str| {
            if name != variable_name {
                return None;
            }
            Some(variable.clone())
        };
        let agents = agents_of(&config, &env, &home);
        let agent = agents.into_iter().next().unwrap();
        Fixture { temp, agent }
    }

    fn file_names_in(directory: &Path) -> Vec<String> {
        let mut names = Vec::new();
        for entry in fs::read_dir(directory).unwrap() {
            let name = entry.unwrap().file_name();
            names.push(name.to_string_lossy().into_owned());
        }
        names.sort();
        names
    }

    #[test]
    fn a_file_changed_since_it_was_read_is_left_alone() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("settings.json");
        let edited = "{\"edited\": \"by the user\"}";
        fs::write(&path, edited).unwrap();

        let problem = write_settings(&path, Some("{}"), "{\"ours\": true}");

        assert!(problem.is_err());
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(after, edited);
        let unchanged = write_settings(&path, Some(edited), "{}");
        assert_eq!(unchanged, Ok(()));
    }

    #[test]
    fn a_rewritten_file_keeps_its_permissions_and_a_failed_write_names_the_path() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, "{}").unwrap();
        let private = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, private).unwrap();

        let rewritten = write_settings(&path, Some("{}"), "{\"ours\": 1}");
        assert_eq!(rewritten, Ok(()));

        let metadata = fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let names = file_names_in(temp.path());
        assert_eq!(names, vec!["settings.json".to_string()]);

        let missing_directory = temp.path().join("missing");
        let unwritable = missing_directory.join("settings.json");
        let problem = write_settings(&unwritable, None, "{}").unwrap_err();
        let shown = unwritable.display().to_string();
        assert!(problem.contains(&shown), "{problem}");
        assert!(!missing_directory.exists());
    }

    #[test]
    fn a_symlinked_settings_file_stays_a_symlink() {
        let fixture = fixture();
        let agent = &fixture.agent;
        let dotfiles = fixture.temp.path().join("dotfiles");
        fs::create_dir_all(&dotfiles).unwrap();
        let target = dotfiles.join("settings.json");
        let hooks_directory = &agent.paths.hooks_directory;
        let managed = electron_settings(hooks_directory, "{}");
        fs::write(&target, managed).unwrap();
        let middle = dotfiles.join("middle.json");
        symlink("settings.json", &middle).unwrap();
        symlink(&middle, &agent.paths.settings_path).unwrap();

        assert_eq!(remove_old_setup(agent), Ok(()));

        let metadata = fs::symlink_metadata(&agent.paths.settings_path).unwrap();
        assert!(metadata.file_type().is_symlink());
        let middle_metadata = fs::symlink_metadata(&middle).unwrap();
        assert!(middle_metadata.file_type().is_symlink());
        let text = fs::read_to_string(&target).unwrap();
        assert!(!has_managed_entries(Some(&text), hooks_directory));
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert!(parsed.get("hooks").is_none());
        let beside_target = file_names_in(&dotfiles);
        let beside_link = file_names_in(&agent.paths.agent_directory);
        for name in beside_target.iter().chain(beside_link.iter()) {
            assert!(!name.ends_with("taco-tmp"), "{name}");
        }
    }
}
