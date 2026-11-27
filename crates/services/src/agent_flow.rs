use std::path::{Path, PathBuf};

use taco_agents::{HARNESSES, HarnessSetup};
use taco_protocol::HarnessId;

use crate::agent_hooks::{has_managed_entries, remove_managed_entries};
use crate::agent_setup_state::{
    AgentSetupFacts, AgentSetupState, PluginDirs, agent_setup_state_of,
};
use crate::config_files::plugin_directory_path;
use crate::file_write::{read_text, write_if_unchanged};
use crate::hook_scripts::hooks_directory;
use crate::plugin_dirs::{MANAGED_SETTINGS_PATH, plugin_dirs_of};
use crate::settings_edit::HookEdit;

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

pub(crate) fn read_settings(path: &Path) -> Result<Option<String>, String> {
    let shown = path.display().to_string();
    read_text(path, &shown)
}

pub(crate) fn write_settings(
    path: &Path,
    original: Option<&str>,
    text: &str,
) -> Result<(), String> {
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

pub(crate) fn label_of(agent: &Agent) -> String {
    agent.paths.settings_path.display().to_string()
}

pub(crate) fn plugin_dirs_reading(agent: &Agent, user_text: Option<&str>) -> PluginDirs {
    let managed_read = read_settings(&agent.paths.managed_settings_path);
    let managed_readable = managed_read.ok();
    let managed_text = managed_readable.flatten();
    plugin_dirs_of(
        user_text,
        &agent.paths.settings_path,
        managed_text.as_deref(),
        &agent.paths.plugin_folder,
        &agent.paths.home,
        agent.setup.plugin_variable,
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
        minimum_version: agent.setup.minimum_version,
    };
    agent_setup_state_of(&facts)
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
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

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
