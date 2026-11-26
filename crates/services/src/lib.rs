//! The parts of the app that touch the user's machine outside a terminal.

mod config_files;
mod config_store;
mod identity;

pub use config_files::{
    config_file_path, focus_port_path, read_config_texts, seed_config_files, unreadable_problems,
};
pub use config_store::ConfigStore;
pub use identity::{
    APPLICATION_NAME, CONFIG_DIRECTORY_NAME, CONFIG_DIRECTORY_VARIABLE, IS_STABLE, config_directory,
};
