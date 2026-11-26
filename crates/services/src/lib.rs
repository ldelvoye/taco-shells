//! The parts of the app that touch the user's machine outside a terminal.

mod agent_flow;
mod agent_hooks;
mod agent_record;
mod agent_setup_state;
mod agent_setups;
#[cfg(test)]
mod build_identity;
mod config_files;
mod config_store;
mod hook_listener;
mod identity;
mod mac;
mod updater;

pub use agent_flow::{copy_hook_scripts, hooks_directory};
pub use agent_hooks::{
    HookEdit, HookWiring, add_entries, entry_key, hook_wiring_of, remove_hand_entries,
    remove_managed_entries,
};
pub use agent_record::{AgentRecord, RECORD_FILE, read_agent_record, write_agent_record};
pub use agent_setup_state::{
    AgentMenu, AgentMenuAction, AgentSetupFacts, AgentSetupState, agent_menu_of,
    agent_setup_state_of, runs_agent,
};
pub use agent_setups::{
    AgentMenuEntry, AgentSetupNotice, AgentSetupNoticeKind, AgentSetupView, AgentSetups,
    AgentSetupsHost,
};
pub use config_files::{
    config_file_path, focus_port_path, read_config_texts, seed_config_files, unreadable_problems,
};
pub use config_store::ConfigStore;
pub use hook_listener::{FOCUS_PORTS, HookListener, ListenerEvent};
pub use identity::{
    APPLICATION_NAME, CONFIG_DIRECTORY_NAME, CONFIG_DIRECTORY_VARIABLE, IS_STABLE, config_directory,
};
pub use updater::{UpdateEvent, Updater, UpdaterHost};
