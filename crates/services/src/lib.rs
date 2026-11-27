//! The parts of the app that touch the user's machine outside a terminal.

mod agent_flow;
mod agent_hooks;
mod agent_setup_state;
mod agent_setups;
mod agent_version;
mod config_files;
mod config_store;
mod config_writer;
mod file_write;
mod hook_listener;
mod mac;
mod plugin_dirs;
mod plugin_folder;
mod sync;
mod updater;

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
pub use config_files::{
    ConfigRead, config_file_path, focus_port_path, plugin_directory_path, read_config_files,
    seed_config_files,
};
pub use config_store::ConfigStore;
pub use config_writer::{ConfigChange, ConfigWriter, Refusal};
pub use hook_listener::{
    AnswerPart, FOCUS_PORTS, HookListener, ListenerEvent, OnDelivered, Question, QuestionKind,
};
pub use mac::app_version;
pub use plugin_folder::{PROGRAM_PATH, refresh_plugin_folder};
pub use updater::{UpdateEvent, Updater, UpdaterHost};
