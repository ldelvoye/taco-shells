//! The arrangement of terminals and everything that changes it.

mod rows;
mod sessions;
mod start;
mod workspace;

#[cfg(test)]
mod tests;

pub use workspace::{Options, Outbound, Workspace};
