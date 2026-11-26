//! The terminals in the middle of the window, drawn from Ghostty's screens and following the runtime over the terminal channel.

mod connection;
mod draw;
mod grid;
mod shown;
mod terminals;
mod view;

pub use terminals::Terminals;
