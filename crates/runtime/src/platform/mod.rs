//! The calls only macOS has. The runtime builds for macOS alone; another platform adds its module here.

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{directory_of, group_commands};
