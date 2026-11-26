use serde::{Deserialize, Serialize};

use crate::colors::TerminalColors;
use crate::ids::TerminalId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

/// A terminal to open: the workspace has already named it and placed it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerminalStart {
    pub id: TerminalId,
    /// The directory to start in. When none, `directory_of` is tried, then the home directory.
    pub directory: Option<String>,
    /// A terminal whose current directory to start in, looked up when the shell is started.
    pub directory_of: Option<TerminalId>,
    /// Typed into the shell once it has started.
    pub command: Option<String>,
    pub size: TerminalSize,
}

/// What the app sends the runtime on the control connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuntimeCommand {
    /// Returns at once. A shell that cannot start reports an exit like any other.
    Open(TerminalStart),
    Close {
        id: TerminalId,
    },
    Scrollback {
        lines: u32,
    },
    Colors(TerminalColors),
    /// A question, answered by a `RuntimeReport::Answer` carrying the same `ask`.
    Ask {
        ask: u64,
        query: RuntimeQuery,
    },
    /// End every shell and exit.
    Stop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuntimeQuery {
    DirectoryOf {
        id: TerminalId,
    },
    ForegroundCommands {
        id: TerminalId,
    },
    /// The terminal whose shell is one of these process ids.
    TerminalForPids {
        pids: Vec<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuntimeAnswer {
    Directory(Option<String>),
    Commands(Vec<String>),
    Terminal(Option<TerminalId>),
}

/// What the runtime sends the app on the control connection.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RuntimeReport {
    /// The starting directory is the terminal's first directory report.
    Started {
        id: TerminalId,
        directory: String,
    },
    DirectoryChanged {
        id: TerminalId,
        directory: String,
    },
    Exited {
        id: TerminalId,
        code: i32,
    },
    Titled {
        id: TerminalId,
        title: String,
    },
    /// A line was submitted in the terminal.
    Submitted {
        id: TerminalId,
    },
    Answer {
        ask: u64,
        value: RuntimeAnswer,
    },
}
