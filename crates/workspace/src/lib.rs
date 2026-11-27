//! The terminals' arrangement, their agents, their save, and Agent Activity.

mod activity;
mod asking;
mod board;
mod log;
mod outbound;
mod rows;
mod save;
mod sessions;
mod start;
mod storage;
mod workspace;

#[cfg(test)]
mod tests;

pub use log::KEPT_ENTRIES;
pub use outbound::Outbound;
pub use save::SAVE_DEBOUNCE_MS;
pub use storage::{
    ACTIVITY_DIRECTORY, ELECTRON_COPY_FILE, FileStorage, Storage, UNREADABLE_COPY_FILE,
};
pub use workspace::{Options, Workspace};
