//! The terminals in the middle of the window, drawn from Ghostty's screens and following the runtime over the terminal channel.

mod connection;
mod draw;
mod grid;
mod keys;
mod links;
mod shown;
mod terminals;
mod view;
mod views;

pub use terminals::{Terminals, TerminalsEvent};
