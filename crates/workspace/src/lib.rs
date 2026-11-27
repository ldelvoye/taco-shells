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

pub use outbound::Outbound;
pub use storage::{FileStorage, Storage};
pub use workspace::{Options, Workspace};
