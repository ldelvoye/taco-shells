//! The arrangement of terminals and everything that changes it.

mod asking;
mod board;
mod log;
mod rows;
mod save;
mod sessions;
mod start;
mod storage;
mod workspace;

#[cfg(test)]
mod tests;

pub use log::KEPT_ENTRIES;
pub use storage::{
    ACTIVITY_DIRECTORY, ELECTRON_COPY_FILE, FileStorage, Storage, UNREADABLE_COPY_FILE,
};
pub use workspace::{Options, Outbound, SAVE_DEBOUNCE_MS, Workspace};
