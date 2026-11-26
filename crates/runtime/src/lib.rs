//! The shells and a screen for each, in a process of their own.

pub mod budget;
pub mod core;
mod platform;
pub mod program;
pub mod pty;
pub mod shells;

#[cfg(test)]
mod tests;
