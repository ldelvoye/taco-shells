use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// The colors the runtime answers a program's color queries from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalColors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub cursor_text: Rgb,
    pub selection: Rgb,
    /// Black, red, green, yellow, blue, magenta, cyan, white, then the bright eight.
    pub ansi: [Rgb; 16],
}
