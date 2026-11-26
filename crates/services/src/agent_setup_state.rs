use crate::agent_hooks::HookWiring;

const STATUS_NOT_INSTALLED: &str = "Not installed";
pub(crate) const STATUS_OFF: &str = "Turned off in settings";
const STATUS_BY_HAND: &str = "Set up by hand";
const STATUS_SET_UP: &str = "Set up";
const STATUS_NOT_SET_UP: &str = "Not set up";
const LABEL_SWITCH: &str = "Switch to Managed Setup";
const LABEL_REMOVE: &str = "Remove Setup";
pub(crate) const NOTE_BUILD_MAY_NOT_WRITE: &str = "Development builds leave your settings alone";
const SCRIPT_RUNNER: &str = "node";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSetupState {
    NotInstalled,
    Off,
    ByHand,
    SetUp,
    Declined,
    NotSetUp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSetupFacts {
    pub installed: bool,
    pub managing: bool,
    pub wiring: HookWiring,
    pub declined: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentMenuAction {
    SetUp,
    Remove,
    Switch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMenu {
    pub status: String,
    pub action: AgentMenuAction,
    pub action_label: String,
    pub enabled: bool,
    pub note: Option<String>,
}

pub fn agent_setup_state_of(facts: &AgentSetupFacts) -> AgentSetupState {
    if !facts.installed {
        return AgentSetupState::NotInstalled;
    }
    if !facts.managing {
        return AgentSetupState::Off;
    }
    if facts.wiring == HookWiring::ByHand {
        return AgentSetupState::ByHand;
    }
    if facts.wiring == HookWiring::Managed {
        return AgentSetupState::SetUp;
    }
    if facts.declined {
        return AgentSetupState::Declined;
    }
    AgentSetupState::NotSetUp
}

fn note_of(can_write: bool) -> Option<String> {
    if can_write {
        return None;
    }
    Some(NOTE_BUILD_MAY_NOT_WRITE.to_string())
}

pub fn agent_menu_of(state: AgentSetupState, product_name: &str, can_write: bool) -> AgentMenu {
    let set_up_label = format!("Set Up {product_name}");
    let note = note_of(can_write);
    match state {
        AgentSetupState::NotInstalled => AgentMenu {
            status: STATUS_NOT_INSTALLED.to_string(),
            action: AgentMenuAction::SetUp,
            action_label: set_up_label,
            enabled: false,
            note: None,
        },
        AgentSetupState::Off => AgentMenu {
            status: STATUS_OFF.to_string(),
            action: AgentMenuAction::SetUp,
            action_label: set_up_label,
            enabled: false,
            note: None,
        },
        AgentSetupState::ByHand => AgentMenu {
            status: STATUS_BY_HAND.to_string(),
            action: AgentMenuAction::Switch,
            action_label: LABEL_SWITCH.to_string(),
            enabled: can_write,
            note,
        },
        AgentSetupState::SetUp => AgentMenu {
            status: STATUS_SET_UP.to_string(),
            action: AgentMenuAction::Remove,
            action_label: LABEL_REMOVE.to_string(),
            enabled: can_write,
            note,
        },
        AgentSetupState::Declined | AgentSetupState::NotSetUp => AgentMenu {
            status: STATUS_NOT_SET_UP.to_string(),
            action: AgentMenuAction::SetUp,
            action_label: set_up_label,
            enabled: can_write,
            note,
        },
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

    fn facts(
        installed: bool,
        managing: bool,
        wiring: HookWiring,
        declined: bool,
    ) -> AgentSetupFacts {
        AgentSetupFacts {
            installed,
            managing,
            wiring,
            declined,
        }
    }

    #[test]
    fn each_state_beats_the_ones_after_it() {
        let cases = [
            (
                facts(false, false, HookWiring::ByHand, true),
                AgentSetupState::NotInstalled,
            ),
            (
                facts(true, false, HookWiring::ByHand, true),
                AgentSetupState::Off,
            ),
            (
                facts(true, true, HookWiring::ByHand, true),
                AgentSetupState::ByHand,
            ),
            (
                facts(true, true, HookWiring::Managed, true),
                AgentSetupState::SetUp,
            ),
            (
                facts(true, true, HookWiring::None, true),
                AgentSetupState::Declined,
            ),
            (
                facts(true, true, HookWiring::None, false),
                AgentSetupState::NotSetUp,
            ),
        ];
        for (case, expected) in cases {
            let state = agent_setup_state_of(&case);
            assert_eq!(state, expected);
        }
    }

    #[test]
    fn the_menu_offers_one_action_and_says_why_it_is_off() {
        let by_hand = agent_menu_of(AgentSetupState::ByHand, "Claude Code", true);
        assert_eq!(by_hand.action, AgentMenuAction::Switch);
        assert!(by_hand.enabled);
        assert!(by_hand.note.is_none());

        let set_up = agent_menu_of(AgentSetupState::SetUp, "Claude Code", false);
        assert_eq!(set_up.action, AgentMenuAction::Remove);
        assert!(!set_up.enabled);
        assert!(set_up.note.is_some());

        let declined = agent_menu_of(AgentSetupState::Declined, "Claude Code", true);
        assert_eq!(declined.action, AgentMenuAction::SetUp);
        assert!(declined.enabled);
        assert!(declined.note.is_none());

        let not_installed = agent_menu_of(AgentSetupState::NotInstalled, "Claude Code", true);
        assert!(!not_installed.enabled);
        assert!(not_installed.note.is_none());

        let off = agent_menu_of(AgentSetupState::Off, "Claude Code", true);
        assert!(!off.enabled);
        assert!(off.note.is_none());
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
