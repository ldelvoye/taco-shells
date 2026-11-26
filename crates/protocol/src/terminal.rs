use serde::{Deserialize, Serialize};

use crate::ids::TerminalId;
use crate::runtime::TerminalSize;

/// What a window asks of a terminal over the terminal channel. `Write` and `Resize` work with no view attached.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TerminalRequest {
    /// `attachment` numbers this attach and comes back with everything sent for it.
    Attach {
        id: TerminalId,
        attachment: u64,
        size: Option<TerminalSize>,
    },
    Detach {
        id: TerminalId,
    },
    Write {
        id: TerminalId,
        data: Vec<u8>,
    },
    Resize {
        id: TerminalId,
        size: TerminalSize,
    },
    Clear {
        id: TerminalId,
    },
}

/// What an attached view is sent, in the order it has to apply it. Nothing reaches a view before its `Attached`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TerminalEvent {
    /// `screen` is the snapshot's first part: enough to draw at once.
    Attached {
        id: TerminalId,
        attachment: u64,
        size: TerminalSize,
        screen: Vec<u8>,
    },
    /// Whole snapshot records, newest history first. The part marked `last` ends the snapshot.
    History {
        id: TerminalId,
        attachment: u64,
        part: Vec<u8>,
        last: bool,
    },
    Output {
        id: TerminalId,
        data: Vec<u8>,
    },
    Resized {
        id: TerminalId,
        size: TerminalSize,
    },
    Cleared {
        id: TerminalId,
    },
    End {
        id: TerminalId,
    },
}
