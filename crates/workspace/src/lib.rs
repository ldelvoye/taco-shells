//! The arrangement of terminals and everything that changes it.

mod rows;
mod save;
mod sessions;
mod start;
mod storage;
mod workspace;

#[cfg(test)]
mod tests;

pub use sessions::PROVISIONAL_MS;
pub use storage::{ELECTRON_COPY_FILE, FileStorage, Storage};
pub use workspace::{Options, Outbound, SAVE_DEBOUNCE_MS, Workspace};
