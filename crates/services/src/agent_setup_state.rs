use std::path::{Path, PathBuf};

use crate::agent_hooks::HookWiring;
use crate::claude_version::is_too_old;

const STATUS_NOT_INSTALLED: &str = "Not installed";
const STATUS_OFF: &str = "Off";
const STATUS_OLD_SETUP: &str = "Set up the old way";
const STATUS_ON: &str = "On";
const STATUS_PLUGIN_DIRS_HIDDEN: &str = "Its settings hide the plugin";
const STATUS_PLUGIN_DIRS_MANAGED: &str = "Blocked by managed settings";
const SCRIPT_RUNNER: &str = "node";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginDirs {
    Unset,
    Lists,
    Hides { file: PathBuf },
    ManagedHides,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSetupState {
    NotInstalled,
    Off,
    OldSetup,
    PluginDirsHidden { file: String },
    PluginDirsManaged,
    TooOld { version: String },
    On,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSetupFacts {
    pub installed: bool,
    pub activity_on: bool,
    pub wiring: HookWiring,
    pub plugin_dirs: PluginDirs,
    pub home: PathBuf,
    pub version: Option<(u32, u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentMenuAction {
    SetUp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMenu {
    pub status: String,
    pub action: AgentMenuAction,
    pub action_label: String,
    pub enabled: bool,
}

fn version_text(version: (u32, u32, u32)) -> String {
    let (major, minor, patch) = version;
    format!("{major}.{minor}.{patch}")
}

fn shown_with_tilde(file: &Path, home: &Path) -> String {
    let Ok(rest) = file.strip_prefix(home) else {
        return file.display().to_string();
    };
    let shown_rest = rest.display();
    format!("~/{shown_rest}")
}

pub fn agent_setup_state_of(facts: &AgentSetupFacts) -> AgentSetupState {
    if !facts.installed {
        return AgentSetupState::NotInstalled;
    }
    if facts.plugin_dirs == PluginDirs::ManagedHides {
        return AgentSetupState::PluginDirsManaged;
    }
    if facts.wiring == HookWiring::Managed {
        return AgentSetupState::OldSetup;
    }
    if let PluginDirs::Hides { file } = &facts.plugin_dirs {
        let shown = shown_with_tilde(file, &facts.home);
        return AgentSetupState::PluginDirsHidden { file: shown };
    }
    if !facts.activity_on {
        return AgentSetupState::Off;
    }
    if let Some(version) = facts.version
        && is_too_old(version)
    {
        let shown = version_text(version);
        return AgentSetupState::TooOld { version: shown };
    }
    AgentSetupState::On
}

pub fn agent_menu_of(state: &AgentSetupState, product_name: &str) -> AgentMenu {
    let action_label = format!("Set Up {product_name}");
    let (status, enabled) = match state {
        AgentSetupState::NotInstalled => (STATUS_NOT_INSTALLED.to_string(), false),
        AgentSetupState::Off => (STATUS_OFF.to_string(), true),
        AgentSetupState::OldSetup => (STATUS_OLD_SETUP.to_string(), true),
        AgentSetupState::PluginDirsHidden { .. } => (STATUS_PLUGIN_DIRS_HIDDEN.to_string(), true),
        AgentSetupState::PluginDirsManaged => (STATUS_PLUGIN_DIRS_MANAGED.to_string(), false),
        AgentSetupState::TooOld { .. } => (format!("Needs a newer {product_name}"), false),
        AgentSetupState::On => (STATUS_ON.to_string(), false),
    };
    AgentMenu {
        status,
        action: AgentMenuAction::SetUp,
        action_label,
        enabled,
    }
}

fn base_name_of(word: &str) -> &str {
    let slash = word.rfind('/');
    match slash {
        Some(index) => &word[index + 1..],
        None => word,
    }
}

fn runs_command(command_line: &str, commands: &[&str]) -> bool {
    let words: Vec<&str> = command_line.split_whitespace().collect();
    let Some(first) = words.first() else {
        return false;
    };
    let program = base_name_of(first);
    if commands.contains(&program) {
        return true;
    }
    // A CLI installed through npm shows in the process list as "node <path>/<name>".
    if program != SCRIPT_RUNNER {
        return false;
    }
    let Some(second) = words.get(1) else {
        return false;
    };
    let script = base_name_of(second);
    commands.contains(&script)
}

pub fn runs_agent(command_lines: &[String], commands: &[&str]) -> bool {
    for command_line in command_lines {
        if runs_command(command_line, commands) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: Option<(u32, u32, u32)> = Some((2, 1, 279));
    const NEW: Option<(u32, u32, u32)> = Some((2, 1, 280));

    fn facts(
        installed: bool,
        wiring: HookWiring,
        activity_on: bool,
        version: Option<(u32, u32, u32)>,
    ) -> AgentSetupFacts {
        AgentSetupFacts {
            installed,
            activity_on,
            wiring,
            plugin_dirs: PluginDirs::Unset,
            home: PathBuf::from("/h"),
            version,
        }
    }

    fn with_plugin_dirs(facts: AgentSetupFacts, plugin_dirs: PluginDirs) -> AgentSetupFacts {
        AgentSetupFacts {
            plugin_dirs,
            ..facts
        }
    }

    #[test]
    fn each_state_beats_the_ones_after_it() {
        let too_old = AgentSetupState::TooOld {
            version: "2.1.279".to_string(),
        };
        let hides = PluginDirs::Hides {
            file: PathBuf::from("/h/.claude/settings.json"),
        };
        let hidden = AgentSetupState::PluginDirsHidden {
            file: "~/.claude/settings.json".to_string(),
        };
        let managed_over_old = with_plugin_dirs(
            facts(true, HookWiring::Managed, true, OLD),
            PluginDirs::ManagedHides,
        );
        let old_over_hidden =
            with_plugin_dirs(facts(true, HookWiring::Managed, true, OLD), hides.clone());
        let hidden_while_off =
            with_plugin_dirs(facts(true, HookWiring::ByHand, false, OLD), hides.clone());
        let hidden_while_on = with_plugin_dirs(facts(true, HookWiring::ByHand, true, NEW), hides);
        let cases = [
            (managed_over_old, AgentSetupState::PluginDirsManaged),
            (old_over_hidden, AgentSetupState::OldSetup),
            (hidden_while_off, hidden.clone()),
            (hidden_while_on, hidden),
            (
                facts(false, HookWiring::Managed, false, OLD),
                AgentSetupState::NotInstalled,
            ),
            (
                facts(true, HookWiring::Managed, true, OLD),
                AgentSetupState::OldSetup,
            ),
            (
                facts(true, HookWiring::ByHand, false, OLD),
                AgentSetupState::Off,
            ),
            (facts(true, HookWiring::ByHand, true, OLD), too_old),
            (
                facts(true, HookWiring::ByHand, true, NEW),
                AgentSetupState::On,
            ),
            (
                facts(true, HookWiring::None, true, None),
                AgentSetupState::On,
            ),
        ];
        for (case, expected) in cases {
            let state = agent_setup_state_of(&case);
            assert_eq!(state, expected, "{case:?}");
        }
    }

    #[test]
    fn set_up_is_offered_only_while_off_or_set_up_the_old_way() {
        let too_old = AgentSetupState::TooOld {
            version: "2.1.279".to_string(),
        };
        let cases = [
            (AgentSetupState::NotInstalled, false),
            (AgentSetupState::Off, true),
            (AgentSetupState::OldSetup, true),
            (too_old, false),
            (AgentSetupState::On, false),
        ];
        for (state, enabled) in cases {
            let menu = agent_menu_of(&state, "Claude Code");
            assert_eq!(menu.enabled, enabled, "{state:?}");
            assert_eq!(menu.action, AgentMenuAction::SetUp);
        }
    }

    #[test]
    fn an_agent_runs_as_its_program_or_as_node_running_its_script() {
        let commands = ["claude"];
        let runs = [
            "claude --resume x",
            "node /usr/lib/node_modules/@anthropic-ai/claude-code/claude",
            "/opt/bin/claude",
            "node /x/claude",
        ];
        for line in runs {
            let lines = [line.to_string()];
            assert!(runs_agent(&lines, &commands), "{line}");
        }
        let does_not_run = ["vim claude.md", "node", "", "node /x/cli.js"];
        for line in does_not_run {
            let lines = [line.to_string()];
            assert!(!runs_agent(&lines, &commands), "{line}");
        }
    }
}
