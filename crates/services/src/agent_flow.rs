use std::path::{Path, PathBuf};

use taco_agents::{HARNESSES, HarnessSetup, Version};
use taco_protocol::HarnessId;

use crate::agent_setup_state::{
    AgentSetupFacts, AgentSetupState, PluginDirs, agent_setup_state_of,
};
use crate::config_files::plugin_directory_path;
use crate::file_write::{read_text, write_if_unchanged};
use crate::plugin_dirs::{MANAGED_SETTINGS_PATH, plugin_dirs_of};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentPaths {
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

/// A settings file's text, or none when it is missing or cannot be read.
pub(crate) fn settings_text(path: &Path) -> Option<String> {
    let read = read_settings(path);
    let readable = read.ok();
    readable.flatten()
}

pub(crate) fn write_settings(
    path: &Path,
    original: Option<&str>,
    text: &str,
) -> Result<(), String> {
    let shown = path.display().to_string();
    write_if_unchanged(path, &shown, original, text)
}

pub(crate) fn label_of(agent: &Agent) -> String {
    agent.paths.settings_path.display().to_string()
}

pub(crate) fn plugin_dirs_reading(agent: &Agent, user_text: Option<&str>) -> PluginDirs {
    let managed_text = settings_text(&agent.paths.managed_settings_path);
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
    version: Option<Version>,
) -> AgentSetupState {
    let installed = agent.paths.agent_directory.exists();
    let text = settings_text(&agent.paths.settings_path);
    let plugin_dirs = plugin_dirs_reading(agent, text.as_deref());
    let facts = AgentSetupFacts {
        installed,
        activity_on,
        plugin_dirs,
        home: agent.paths.home.clone(),
        version,
        minimum_version: agent.setup.minimum_version,
    };
    agent_setup_state_of(&facts)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

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
}
