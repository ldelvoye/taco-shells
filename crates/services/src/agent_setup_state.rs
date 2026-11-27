use std::path::{Path, PathBuf};

use taco_agents::Version;

const STATUS_NOT_INSTALLED: &str = "Not installed";
const STATUS_OFF: &str = "Off";
const STATUS_OLD_SETUP: &str = "Set up the old way";
const STATUS_ON: &str = "On";
const STATUS_PLUGIN_DIRS_HIDDEN: &str = "Its settings hide the plugin";
const STATUS_PLUGIN_DIRS_MANAGED: &str = "Blocked by managed settings";

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
    pub has_old_entries: bool,
    pub plugin_dirs: PluginDirs,
    pub home: PathBuf,
    pub version: Option<Version>,
    pub minimum_version: Version,
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

fn version_text(version: Version) -> String {
    let (major, minor, patch) = version;
    format!("{major}.{minor}.{patch}")
}

pub(crate) fn shown_with_tilde(file: &Path, home: &Path) -> String {
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
    if facts.has_old_entries {
        return AgentSetupState::OldSetup;
    }
    if facts.activity_on
        && let Some(version) = facts.version
    {
        let too_old = version < facts.minimum_version;
        if too_old {
            let shown = version_text(version);
            return AgentSetupState::TooOld { version: shown };
        }
    }
    if let PluginDirs::Hides { file } = &facts.plugin_dirs {
        let shown = shown_with_tilde(file, &facts.home);
        return AgentSetupState::PluginDirsHidden { file: shown };
    }
    if !facts.activity_on {
        return AgentSetupState::Off;
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

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: Option<Version> = Some((2, 1, 279));
    const NEW: Option<Version> = Some((2, 1, 280));

    fn facts(
        installed: bool,
        has_old_entries: bool,
        activity_on: bool,
        version: Option<Version>,
    ) -> AgentSetupFacts {
        AgentSetupFacts {
            installed,
            activity_on,
            has_old_entries,
            plugin_dirs: PluginDirs::Unset,
            home: PathBuf::from("/h"),
            version,
            minimum_version: (2, 1, 280),
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
        let managed_over_old =
            with_plugin_dirs(facts(true, true, true, OLD), PluginDirs::ManagedHides);
        let old_over_hidden = with_plugin_dirs(facts(true, true, true, OLD), hides.clone());
        let hidden_while_off = with_plugin_dirs(facts(true, false, false, OLD), hides.clone());
        let hidden_while_on = with_plugin_dirs(facts(true, false, true, NEW), hides.clone());
        let too_old_over_hidden = with_plugin_dirs(facts(true, false, true, OLD), hides);
        let cases = [
            (too_old_over_hidden, too_old.clone()),
            (managed_over_old, AgentSetupState::PluginDirsManaged),
            (old_over_hidden, AgentSetupState::OldSetup),
            (hidden_while_off, hidden.clone()),
            (hidden_while_on, hidden),
            (
                facts(false, true, false, OLD),
                AgentSetupState::NotInstalled,
            ),
            (facts(true, true, true, OLD), AgentSetupState::OldSetup),
            (facts(true, false, false, OLD), AgentSetupState::Off),
            (facts(true, false, true, OLD), too_old),
            (facts(true, false, true, NEW), AgentSetupState::On),
            (facts(true, false, true, None), AgentSetupState::On),
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
}
