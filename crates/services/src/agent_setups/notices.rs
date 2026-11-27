use std::collections::BTreeSet;

use taco_protocol::{HarnessId, TerminalId};

use super::Reading;
use crate::agent_setup_state::{AgentMenu, AgentMenuAction, AgentSetupState};

pub(super) const NOT_LOADED_REASON: &str = "It may have been started before Agent Activity was on, or with its own plugin settings. Start it again in a new terminal.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSetupNoticeKind {
    Offer { action: AgentMenuAction },
    Unavailable { reason: String },
    Done,
    Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSetupNotice {
    pub harness: HarnessId,
    pub kind: AgentSetupNoticeKind,
    pub text: String,
    /// A quieter second line: only a Done whose click changed the agent's settings file has one.
    pub detail: Option<String>,
    pub terminals: Vec<TerminalId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMenuEntry {
    pub harness: HarnessId,
    pub product_name: String,
    pub menu: AgentMenu,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AgentSetupView {
    pub menus: Vec<AgentMenuEntry>,
    pub notices: Vec<AgentSetupNotice>,
}

#[derive(Clone)]
pub(super) enum Recorded {
    Problem(String),
    Done { changed_file: Option<String> },
}

pub fn plugin_dirs_hidden_text(product_name: &str, variable: &str, file: &str) -> String {
    format!(
        "{product_name} can't load this app's plugin, because {file} sets {variable}. Set up adds this app's folder to it."
    )
}

fn plugin_dirs_managed_reason(product_name: &str, variable: &str) -> String {
    format!("{product_name}'s managed settings set {variable}, so this app's plugin can't load.")
}

fn plugin_dirs_managed_text(product_name: &str) -> String {
    format!("{product_name} is running here but can't load this app's plugin.")
}

pub(super) fn not_loaded_text(product_name: &str) -> String {
    format!("{product_name} here didn't load this app's plugin.")
}

pub fn offer_text(product_name: &str) -> String {
    format!("{product_name} is running here but isn't set up, so its activity can't show.")
}

fn old_setup_text(product_name: &str) -> String {
    format!(
        "{product_name} still reports through this app's old hooks. Set up moves it to the plugin."
    )
}

pub fn too_old_reason(product_name: &str, minimum: (u32, u32, u32), version: &str) -> String {
    let (major, minor, patch) = minimum;
    format!(
        "Agent Activity needs {product_name} {major}.{minor}.{patch} or later; this Mac has {version}."
    )
}

pub fn done_text(product_name: &str) -> String {
    format!("Set up. Start {product_name} in a new terminal to see it here.")
}

pub fn changed_file_text(file: &str) -> String {
    format!("One line changed in {file}.")
}

pub(super) fn recorded_of(problems: Vec<String>, changed_file: Option<String>) -> Recorded {
    if problems.is_empty() {
        return Recorded::Done { changed_file };
    }
    let text = problems.join(" ");
    Recorded::Problem(text)
}

pub(super) fn notice_of(
    reading: &Reading,
    recorded: Option<&Recorded>,
    seen: Option<&BTreeSet<TerminalId>>,
) -> Option<AgentSetupNotice> {
    let seen = seen?;
    if seen.is_empty() {
        return None;
    }
    let product_name = reading.agent.setup.product_name;
    let variable = reading.agent.setup.plugin_variable;
    let minimum = reading.agent.setup.minimum_version;
    let harness = reading.agent.harness_id.clone();
    let terminals: Vec<TerminalId> = seen.iter().cloned().collect();
    let notice_with = |kind: AgentSetupNoticeKind, text: String, detail: Option<String>| {
        Some(AgentSetupNotice {
            harness: harness.clone(),
            kind,
            text,
            detail,
            terminals: terminals.clone(),
        })
    };
    let notice = |kind: AgentSetupNoticeKind, text: String| notice_with(kind, text, None);

    if let Some(Recorded::Problem(problem)) = recorded {
        return notice(AgentSetupNoticeKind::Problem, problem.clone());
    }
    let hides_done = matches!(
        reading.state,
        AgentSetupState::NotInstalled
            | AgentSetupState::TooOld { .. }
            | AgentSetupState::PluginDirsManaged
    );
    if let Some(Recorded::Done { changed_file }) = recorded
        && !hides_done
    {
        let text = done_text(product_name);
        let file = changed_file.as_deref();
        let detail = file.map(changed_file_text);
        return notice_with(AgentSetupNoticeKind::Done, text, detail);
    }
    match &reading.state {
        AgentSetupState::Off => {
            let kind = AgentSetupNoticeKind::Offer {
                action: AgentMenuAction::SetUp,
            };
            notice(kind, offer_text(product_name))
        }
        AgentSetupState::OldSetup => {
            let kind = AgentSetupNoticeKind::Offer {
                action: AgentMenuAction::SetUp,
            };
            notice(kind, old_setup_text(product_name))
        }
        AgentSetupState::PluginDirsHidden { file } => {
            let kind = AgentSetupNoticeKind::Offer {
                action: AgentMenuAction::SetUp,
            };
            let text = plugin_dirs_hidden_text(product_name, variable, file);
            notice(kind, text)
        }
        AgentSetupState::PluginDirsManaged => {
            let reason = plugin_dirs_managed_reason(product_name, variable);
            let kind = AgentSetupNoticeKind::Unavailable { reason };
            notice(kind, plugin_dirs_managed_text(product_name))
        }
        AgentSetupState::TooOld { version } => {
            let reason = too_old_reason(product_name, minimum, version);
            let kind = AgentSetupNoticeKind::Unavailable { reason };
            notice(kind, offer_text(product_name))
        }
        _ => None,
    }
}

pub(super) fn not_loaded_notice_of(
    reading: &Reading,
    recorded: Option<&Recorded>,
    unloaded: Option<&BTreeSet<TerminalId>>,
) -> Option<AgentSetupNotice> {
    if reading.state != AgentSetupState::On {
        return None;
    }
    let unloaded = unloaded?;
    if unloaded.is_empty() {
        return None;
    }
    if let Some(Recorded::Done { .. }) = recorded {
        return None;
    }
    let product_name = reading.agent.setup.product_name;
    let text = not_loaded_text(product_name);
    let reason = NOT_LOADED_REASON.to_string();
    let terminals = unloaded.iter().cloned().collect();
    Some(AgentSetupNotice {
        harness: reading.agent.harness_id.clone(),
        kind: AgentSetupNoticeKind::Unavailable { reason },
        text,
        detail: None,
        terminals,
    })
}
