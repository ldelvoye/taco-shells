//! What the smoke tests send the running app, and what it answers. Built into development builds only.
//!
//! The app listens on the Unix socket named by `TACO_SHELLS_DRIVE_SOCKET`. Each request is one line of JSON,
//! and each gets one reply line, in the order the requests came.

use serde::{Deserialize, Serialize};
use taco_protocol::{DrawnState, TerminalId};

pub const SOCKET_VARIABLE: &str = "TACO_SHELLS_DRIVE_SOCKET";

/// A point in the window, in logical pixels from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Point,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Light,
    Dark,
}

/// Input goes through the window's own input path, so a chord or a drag is handled as a real one is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveRequest {
    /// Keystrokes in gpui's notation, such as `cmd-t`, `e` or `enter`.
    Keys { keys: Vec<String> },
    /// Text as a paste or a composed character arrives.
    Text { text: String },
    /// A press at `from`, moves, and a release at `to`.
    Drag { from: Point, to: Point },
    /// Where a named thing is drawn: `row:<terminal id>`, `panel:<panel>`, `panel:<panel>:title`,
    /// `strip:<panel or settings>`, `middle` or `settings:scrollback`, with panels named `terminals` and `agent_activity`.
    Bounds { target: String },
    /// The state the window last drew, with its own focus move laid over it.
    DrawnState,
    /// The visible lines of a terminal, as its view holds them.
    ScreenText { terminal: TerminalId },
    /// The window drawn to a PNG file at `path`.
    Render {
        path: String,
        appearance: Appearance,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveReply {
    Done,
    /// `None` when the named thing is not drawn, such as a closed panel.
    Bounds(Option<Rect>),
    DrawnState(Box<DrawnState>),
    ScreenText(Vec<String>),
    Failed(String),
}
