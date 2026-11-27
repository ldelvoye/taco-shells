use serde::{Deserialize, Serialize};

use crate::ids::TerminalId;

/// Who wrote an entry or shared a thing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Author {
    Agent,
    User,
    OtherAgent(Peer),
    /// The app's own record of what happened, which the agent did not write: its steps, a thing the app found to share.
    App,
}

/// Another agent session a message was with: the name it gave itself and its address, either of which can be missing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub name: Option<String>,
    pub address: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Step {
    Did { tool: String, summary: String },
    Changed { tool: String, path: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartSource {
    New,
    Resumed,
    Compacted,
    Cleared,
}

/// What an entry says. A stretch is numbered by the `seq` of its first step; a note names the stretch whose steps fold under it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActivityKind {
    Asked {
        text: String,
    },
    MessageIn {
        text: String,
    },
    MessageOut {
        to: Peer,
        text: String,
        stretch: u64,
    },
    Note {
        text: String,
        stretch: Option<u64>,
    },
    /// `title` is the thing's title when it was shared, so the line still reads once the thing has left Shared.
    Shared {
        thing: u64,
        title: String,
    },
    Step {
        stretch: u64,
        step: Step,
    },
    /// The app asked the agent for a note before it finished.
    AskedForNote,
    SessionStarted {
        source: StartSource,
    },
}

/// One line of a terminal's agent log. `seq` orders the entries of one log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "EntryLine")]
pub struct ActivityEntry {
    pub seq: u64,
    pub at_ms: u64,
    pub author: Author,
    pub kind: ActivityKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

/// What opening a shared thing shows. A text's body is not carried here; the window asks for it with `ActivityRequest::Body`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SharedTarget {
    Text {
        characters: usize,
    },
    File {
        path: String,
    },
    Link {
        url: String,
    },
    PullRequest {
        url: Option<String>,
        number: u64,
        state: PrState,
        checked_ms: Option<u64>,
    },
}

/// One thing in Shared. `kind` is only a label, the agent's ("Plan") or the app's; `target` decides how it opens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SharedThing {
    pub id: u64,
    pub at_ms: u64,
    pub by: Author,
    pub kind: String,
    pub title: String,
    pub target: SharedTarget,
}

/// What the panel shows above the log: the agent's line on what it is doing now, and Shared newest first.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub now: Option<String>,
    pub shared: Vec<SharedThing>,
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
    /// The body of a shared text, when the user opens it.
    Body {
        terminal: TerminalId,
        thing: u64,
    },
}

/// What the watched log sends. Entries are oldest first, and `complete` is true when nothing older is kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ActivityEvent {
    Loaded {
        terminal: TerminalId,
        entries: Vec<ActivityEntry>,
        complete: bool,
        board: Board,
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
    /// Sent whole on any change to the board.
    BoardChanged { terminal: TerminalId, board: Board },
    /// `body` is none when the thing is gone or is not a text.
    Body {
        terminal: TerminalId,
        thing: u64,
        body: Option<String>,
    },
}

// A line as an earlier build wrote it has no author and a kind of Asked, Did or Changed.
// Untagged tries the arms in order, so `Current` stays first: a line with an author is read as written.
#[derive(Deserialize)]
#[serde(untagged)]
enum EntryLine {
    Current {
        seq: u64,
        at_ms: u64,
        author: Author,
        kind: ActivityKind,
    },
    Earlier {
        seq: u64,
        at_ms: u64,
        kind: EarlierKind,
    },
}

#[derive(Deserialize)]
enum EarlierKind {
    Asked { text: String },
    Did { tool: String, summary: String },
    Changed { tool: String, path: String },
}

impl From<EntryLine> for ActivityEntry {
    fn from(line: EntryLine) -> ActivityEntry {
        match line {
            EntryLine::Current {
                seq,
                at_ms,
                author,
                kind,
            } => ActivityEntry {
                seq,
                at_ms,
                author,
                kind,
            },
            EntryLine::Earlier { seq, at_ms, kind } => earlier_entry(seq, at_ms, kind),
        }
    }
}

// An earlier step is a stretch of its own, numbered by its own seq.
fn earlier_entry(seq: u64, at_ms: u64, kind: EarlierKind) -> ActivityEntry {
    let step = match kind {
        EarlierKind::Asked { text } => {
            return ActivityEntry {
                seq,
                at_ms,
                author: Author::User,
                kind: ActivityKind::Asked { text },
            };
        }
        EarlierKind::Did { tool, summary } => Step::Did { tool, summary },
        EarlierKind::Changed { tool, path } => Step::Changed { tool, path },
    };
    ActivityEntry {
        seq,
        at_ms,
        author: Author::App,
        kind: ActivityKind::Step { stretch: seq, step },
    }
}
