//! The parts of the app that touch the user's machine outside a terminal.

mod agent_flow;
mod agent_hooks;
mod agent_setup_state;
mod agent_setups;
#[cfg(test)]
mod build_identity;
mod claude_version;
mod config_files;
mod config_store;
mod config_writer;
mod file_write;
mod hook_listener;
mod identity;
mod mac;
mod plugin_dirs;
mod plugin_folder;
mod sync;
mod updater;

pub use agent_flow::{hooks_directory, stub_hook_scripts};
pub use agent_hooks::HookEdit;
pub use agent_setup_state::{
    AgentMenu, AgentMenuAction, AgentSetupFacts, AgentSetupState, PluginDirs, agent_menu_of,
    agent_setup_state_of, runs_agent,
};
pub use agent_setups::{
    AgentMenuEntry, AgentSetupNotice, AgentSetupNoticeKind, AgentSetupView, AgentSetups,
    AgentSetupsHost, PluginHeard, changed_file_text, done_text, offer_text,
    plugin_dirs_hidden_text, too_old_reason,
};
pub use claude_version::{MINIMUM_VERSION, claude_version, find_claude, is_too_old, parse_version};
pub use config_files::{
    config_file_path, focus_port_path, plugin_directory_path, read_config_texts, seed_config_files,
    unreadable_problems,
};
pub use config_store::ConfigStore;
pub use config_writer::{ConfigChange, ConfigWriter, Refusal};
pub use hook_listener::{
    AnswerPart, FOCUS_PORTS, HookListener, ListenerEvent, OnDelivered, Question, QuestionKind,
};
pub use identity::{
    AGENT_APP_NAME, APPLICATION_NAME, CONFIG_DIRECTORY_NAME, CONFIG_DIRECTORY_VARIABLE, IS_STABLE,
    config_directory,
};
pub use mac::app_version;
pub use plugin_folder::{PROGRAM_PATH, refresh_plugin_folder};
pub use updater::{UpdateEvent, Updater, UpdaterHost};
