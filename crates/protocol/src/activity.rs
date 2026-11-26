use serde::{Deserialize, Serialize};

use crate::ids::TerminalId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActivityKind {
    Asked { text: String },
    Did { tool: String, summary: String },
    Changed { tool: String, path: String },
}

/// One line of a terminal's agent log. `seq` orders the entries of one log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActivityEntry {
    pub seq: u64,
    pub at_ms: u64,
    pub kind: ActivityKind,
}

/// What the window asks of the log. It watches one terminal at a time, or none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActivityRequest {
    Watch {
        terminal: Option<TerminalId>,
    },
    /// The entries before the one numbered `before`.
    Older {
        terminal: TerminalId,
        before: u64,
    },
}

/// What the watched log sends. Entries are oldest first, and `complete` is true when nothing older is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActivityEvent {
    Loaded {
        terminal: TerminalId,
        entries: Vec<ActivityEntry>,
        complete: bool,
    },
    Older {
        terminal: TerminalId,
        entries: Vec<ActivityEntry>,
        complete: bool,
    },
    Appended {
        terminal: TerminalId,
        entry: ActivityEntry,
    },
}
