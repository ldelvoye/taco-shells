use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use taco_agents::{HARNESSES, HarnessSetup, HookEntry};
use taco_protocol::HarnessId;

use crate::agent_hooks::{
    HookEdit, add_entries, entry_key, hook_wiring_of, remove_hand_entries, remove_managed_entries,
};
use crate::agent_record::{AgentRecord, RECORD_FILE, read_agent_record, write_agent_record};
use crate::agent_setup_state::{AgentSetupFacts, AgentSetupState, agent_setup_state_of};
use crate::file_write::{read_text, write_if_unchanged};
use crate::identity::CONFIG_DIRECTORY_VARIABLE;

const HOOKS_DIRECTORY_NAME: &str = "hooks";
const SCRIPT_MODE: u32 = 0o755;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentPaths {
    pub(crate) config_directory: PathBuf,
    pub(crate) hooks_directory: PathBuf,
    pub(crate) agent_directory: PathBuf,
    pub(crate) settings_path: PathBuf,
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

fn copy_scripts(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    let entries = fs::read_dir(source)?;
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_file() {
            continue;
        }
        let from = entry.path();
        let name = entry.file_name();
        let to = destination.join(name);
        fs::copy(&from, &to)?;
        let mode = fs::Permissions::from_mode(SCRIPT_MODE);
        fs::set_permissions(&to, mode)?;
    }
    Ok(())
}

pub fn copy_hook_scripts(source: &Path, destination: &Path) -> Vec<String> {
    let copied = copy_scripts(source, destination);
    match copied {
        Ok(()) => Vec::new(),
        Err(error) => {
            let shown = destination.display();
            vec![format!(
                "The hook scripts could not be copied to {shown}: {error}"
            )]
        }
    }
}

fn is_set(value: Option<String>) -> bool {
    match value {
        Some(text) => !text.is_empty(),
        None => false,
    }
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
        config_directory: config_directory.to_path_buf(),
        hooks_directory: hooks_directory(config_directory),
        agent_directory,
        settings_path,
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

pub(crate) fn can_write_for(
    setup: &HarnessSetup,
    is_stable: bool,
    env: &dyn Fn(&str) -> Option<String>,
) -> bool {
    if is_stable {
        return true;
    }
    // A development build writes only where both directories were pointed elsewhere, so it can never rewrite an installed agent's real settings.
    let has_app_override = is_set(env(CONFIG_DIRECTORY_VARIABLE));
    let has_agent_override = is_set(env(setup.config_directory_variable));
    has_app_override && has_agent_override
}

fn read_settings(path: &Path) -> Result<Option<String>, String> {
    let shown = path.display().to_string();
    read_text(path, &shown)
}

fn write_settings(path: &Path, original: Option<&str>, text: &str) -> Result<(), String> {
    let shown = path.display().to_string();
    write_if_unchanged(path, &shown, original, text)
}

fn save_record(agent: &Agent, record: &AgentRecord) -> Result<(), String> {
    let config_directory = &agent.paths.config_directory;
    let saved = write_agent_record(config_directory, &agent.harness_id.0, record);
    match saved {
        Ok(()) => Ok(()),
        Err(error) => {
            let path = config_directory.join(RECORD_FILE);
            let shown = path.display();
            Err(format!("{shown} could not be written: {error}"))
        }
    }
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

fn save_set_up_record(agent: &Agent) -> Result<(), String> {
    let written = agent.setup.entries.iter().map(entry_key).collect();
    let record = AgentRecord {
        declined: false,
        written,
    };
    save_record(agent, &record)
}

pub(crate) fn read_state(agent: &Agent, managing: bool) -> AgentSetupState {
    let installed = agent.paths.agent_directory.exists();
    let read = read_settings(&agent.paths.settings_path);
    let readable = read.ok();
    let text = readable.flatten();
    let wiring = hook_wiring_of(text.as_deref(), agent.setup, &agent.paths.hooks_directory);
    let record = read_agent_record(&agent.paths.config_directory, &agent.harness_id.0);
    let facts = AgentSetupFacts {
        installed,
        managing,
        wiring,
        declined: record.declined,
    };
    agent_setup_state_of(&facts)
}

pub(crate) fn set_up(agent: &Agent) -> Result<(), String> {
    let text = read_settings(&agent.paths.settings_path)?;
    let label = label_of(agent);
    let added = add_entries(
        text.as_deref(),
        agent.setup.entries,
        &agent.paths.hooks_directory,
        &label,
    );
    write_edit(agent, text.as_deref(), added)?;
    save_set_up_record(agent)
}

pub(crate) fn remove_setup(agent: &Agent) -> Result<(), String> {
    let text = read_settings(&agent.paths.settings_path)?;
    if let Some(text) = text {
        let label = label_of(agent);
        let removed = remove_managed_entries(&text, &agent.paths.hooks_directory, &label);
        let original = Some(text.as_str());
        write_edit(agent, original, removed)?;
    }
    let record = AgentRecord {
        declined: true,
        written: Vec::new(),
    };
    save_record(agent, &record)
}

pub(crate) fn switch_to_managed(agent: &Agent) -> Result<(), String> {
    let original = read_settings(&agent.paths.settings_path)?;
    let label = label_of(agent);
    let mut text = original.clone();
    if let Some(current) = &original {
        let cleared =
            remove_hand_entries(current, agent.setup, &agent.paths.hooks_directory, &label);
        match cleared {
            HookEdit::Changed(next) => text = Some(next),
            HookEdit::Unchanged => {}
            HookEdit::Refused(problem) => return Err(problem),
        }
    }
    let added = add_entries(
        text.as_deref(),
        agent.setup.entries,
        &agent.paths.hooks_directory,
        &label,
    );
    match added {
        HookEdit::Changed(next) => text = Some(next),
        HookEdit::Unchanged => {}
        HookEdit::Refused(problem) => return Err(problem),
    }
    if let Some(next) = &text
        && text != original
    {
        let started_from = original.as_deref();
        write_settings(&agent.paths.settings_path, started_from, next)?;
    }
    save_set_up_record(agent)
}

pub(crate) fn keep_current(agent: &Agent) -> Result<(), String> {
    let record = read_agent_record(&agent.paths.config_directory, &agent.harness_id.0);
    let mut missing: Vec<HookEntry> = Vec::new();
    // Only entries the record never wrote are added: one it wrote and no longer finds was deleted by the user, and stays deleted.
    for entry in agent.setup.entries {
        let key = entry_key(entry);
        if !record.written.contains(&key) {
            missing.push(*entry);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let text = read_settings(&agent.paths.settings_path)?;
    let label = label_of(agent);
    let added = add_entries(
        text.as_deref(),
        &missing,
        &agent.paths.hooks_directory,
        &label,
    );
    write_edit(agent, text.as_deref(), added)?;
    let mut written = record.written;
    for entry in &missing {
        written.push(entry_key(entry));
    }
    let next = AgentRecord {
        declined: record.declined,
        written,
    };
    save_record(agent, &next)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::os::unix::fs::symlink;

    use serde_json::{Map, Value};
    use tempfile::TempDir;

    use crate::agent_hooks::HookWiring;

    use super::*;

    const BY_HAND_SETTINGS: &str = r#"{"hooks": {"Stop": [
        {"hooks": [{"type": "command", "command": "~/bin/taco-shells-attention.sh"}]}
    ]}}"#;
    const BY_HAND_COMMAND: &str = "~/bin/taco-shells-attention.sh";

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
            if name == variable_name {
                Some(variable.clone())
            } else {
                None
            }
        };
        let agents = agents_of(&config, &env, &home);
        let agent = agents.into_iter().next().unwrap();
        Fixture { temp, agent }
    }

    fn settings_json(agent: &Agent) -> Map<String, Value> {
        let text = fs::read_to_string(&agent.paths.settings_path).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        let Value::Object(root) = parsed else {
            panic!("settings is not an object");
        };
        root
    }

    fn hooks_of(agent: &Agent) -> Map<String, Value> {
        let mut root = settings_json(agent);
        let Some(Value::Object(hooks)) = root.remove("hooks") else {
            panic!("settings has no hooks");
        };
        hooks
    }

    fn commands_under(hooks: &Map<String, Value>, event: &str) -> Vec<String> {
        let mut commands = Vec::new();
        let Some(Value::Array(groups)) = hooks.get(event) else {
            return commands;
        };
        for group in groups {
            let Some(Value::Array(entries)) = group.get("hooks") else {
                continue;
            };
            for entry in entries {
                let command = entry.get("command").and_then(Value::as_str);
                if let Some(command) = command {
                    commands.push(command.to_string());
                }
            }
        }
        commands
    }

    fn wiring_of(agent: &Agent) -> HookWiring {
        let text = fs::read_to_string(&agent.paths.settings_path).unwrap();
        hook_wiring_of(Some(&text), agent.setup, &agent.paths.hooks_directory)
    }

    fn record_of(agent: &Agent) -> AgentRecord {
        read_agent_record(&agent.paths.config_directory, &agent.harness_id.0)
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
    fn keeping_current_adds_only_entries_the_record_never_wrote() {
        let fixture = fixture();
        let agent = &fixture.agent;
        assert_eq!(set_up(agent), Ok(()));
        let entries = agent.setup.entries;
        let kept = entries.first().unwrap();
        let reintroduced = entries.last().unwrap();
        let reintroduced_key = entry_key(reintroduced);

        let mut root = settings_json(agent);
        let Some(Value::Object(hooks)) = root.get_mut("hooks") else {
            panic!("settings has no hooks");
        };
        hooks.remove(kept.event);
        hooks.remove(reintroduced.event);
        let pruned = serde_json::to_string_pretty(&root).unwrap();
        fs::write(&agent.paths.settings_path, pruned).unwrap();

        let id = &agent.harness_id.0;
        let record = record_of(agent);
        let mut written = record.written.clone();
        written.retain(|key| *key != reintroduced_key);
        let older = AgentRecord {
            declined: false,
            written,
        };
        write_agent_record(&agent.paths.config_directory, id, &older).unwrap();

        assert_eq!(keep_current(agent), Ok(()));

        let hooks = hooks_of(agent);
        assert!(hooks.contains_key(reintroduced.event));
        assert!(!hooks.contains_key(kept.event));
        let after = record_of(agent);
        assert!(after.written.contains(&reintroduced_key));
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

        assert_eq!(write_settings(&path, Some("{}"), "{\"ours\": 1}"), Ok(()));

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
        fs::write(&target, "{}\n").unwrap();
        let middle = dotfiles.join("middle.json");
        symlink("settings.json", &middle).unwrap();
        symlink(&middle, &agent.paths.settings_path).unwrap();

        assert_eq!(set_up(agent), Ok(()));

        let metadata = fs::symlink_metadata(&agent.paths.settings_path).unwrap();
        assert!(metadata.file_type().is_symlink());
        let middle_metadata = fs::symlink_metadata(&middle).unwrap();
        assert!(middle_metadata.file_type().is_symlink());
        let text = fs::read_to_string(&target).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        let hooks = parsed.get("hooks").unwrap();
        for entry in agent.setup.entries {
            assert!(hooks.get(entry.event).is_some(), "{}", entry.event);
        }
        let beside_target = file_names_in(&dotfiles);
        let beside_link = file_names_in(&agent.paths.agent_directory);
        for name in beside_target.iter().chain(beside_link.iter()) {
            assert!(!name.ends_with("taco-tmp"), "{name}");
        }
    }

    #[test]
    fn a_dangling_symlink_is_created_at_its_target() {
        let fixture = fixture();
        let agent = &fixture.agent;
        let dotfiles = fixture.temp.path().join("dotfiles");
        fs::create_dir_all(&dotfiles).unwrap();
        let target = dotfiles.join("settings.json");
        symlink(&target, &agent.paths.settings_path).unwrap();

        assert_eq!(set_up(agent), Ok(()));

        let metadata = fs::symlink_metadata(&agent.paths.settings_path).unwrap();
        assert!(metadata.file_type().is_symlink());
        assert!(target.exists());
        let names = file_names_in(&dotfiles);
        assert_eq!(names, vec!["settings.json".to_string()]);
    }

    #[test]
    fn switching_removing_and_setting_up_again_move_the_entries_and_the_record() {
        let fixture = fixture();
        let agent = &fixture.agent;
        let hooks_directory = &agent.paths.hooks_directory;
        fs::write(&agent.paths.settings_path, BY_HAND_SETTINGS).unwrap();
        assert_eq!(wiring_of(agent), HookWiring::ByHand);

        assert_eq!(switch_to_managed(agent), Ok(()));

        assert_eq!(wiring_of(agent), HookWiring::Managed);
        let hooks = hooks_of(agent);
        let stop_commands = commands_under(&hooks, "Stop");
        assert!(
            !stop_commands
                .iter()
                .any(|command| command == BY_HAND_COMMAND)
        );
        for entry in agent.setup.entries {
            let script = hooks_directory.join(entry.script);
            let command = script.display().to_string();
            let commands = commands_under(&hooks, entry.event);
            assert!(commands.contains(&command), "{}", entry.event);
        }
        assert!(!record_of(agent).declined);

        assert_eq!(remove_setup(agent), Ok(()));

        assert_eq!(wiring_of(agent), HookWiring::None);
        let record = record_of(agent);
        assert!(record.declined);
        assert!(record.written.is_empty());

        assert_eq!(set_up(agent), Ok(()));

        assert_eq!(wiring_of(agent), HookWiring::Managed);
        let record = record_of(agent);
        assert!(!record.declined);
        assert_eq!(record.written.len(), agent.setup.entries.len());
    }

    #[test]
    fn copying_scripts_makes_files_executable_skips_folders_and_overwrites() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join("first.sh"), "first").unwrap();
        fs::write(source.join("second.sh"), "second").unwrap();
        fs::write(source.join("nested").join("third.sh"), "third").unwrap();
        let stale = destination.join("first.sh");
        fs::write(&stale, "stale").unwrap();
        let readable = fs::Permissions::from_mode(0o644);
        fs::set_permissions(&stale, readable).unwrap();

        let problems = copy_hook_scripts(&source, &destination);

        assert!(problems.is_empty(), "{problems:?}");
        for name in ["first.sh", "second.sh"] {
            let copied = destination.join(name);
            let metadata = fs::metadata(&copied).unwrap();
            let mode = metadata.permissions().mode() & 0o777;
            assert_eq!(mode, 0o755, "{name}");
        }
        let first = fs::read_to_string(destination.join("first.sh")).unwrap();
        assert_eq!(first, "first");
        assert!(!destination.join("nested").exists());

        let absent = temp.path().join("absent");
        let missing_source = copy_hook_scripts(&absent, &destination);
        assert_eq!(missing_source.len(), 1);
    }

    #[test]
    fn a_development_build_writes_only_where_both_directories_are_moved() {
        let setup = claude_setup();
        let app = CONFIG_DIRECTORY_VARIABLE;
        let agent = setup.config_directory_variable;
        let both = HashMap::from([(app, "/tmp/app"), (agent, "/tmp/agent")]);
        let only_app = HashMap::from([(app, "/tmp/app")]);
        let only_agent = HashMap::from([(agent, "/tmp/agent")]);
        let empty_agent = HashMap::from([(app, "/tmp/app"), (agent, "")]);
        let nothing: HashMap<&str, &str> = HashMap::new();
        let env_of = |vars: HashMap<&'static str, &'static str>| {
            move |name: &str| vars.get(name).map(|value| value.to_string())
        };

        assert!(can_write_for(setup, true, &env_of(nothing.clone())));
        assert!(can_write_for(setup, false, &env_of(both)));
        assert!(!can_write_for(setup, false, &env_of(only_app)));
        assert!(!can_write_for(setup, false, &env_of(only_agent)));
        assert!(!can_write_for(setup, false, &env_of(empty_agent)));
        assert!(!can_write_for(setup, false, &env_of(nothing)));
    }
}
