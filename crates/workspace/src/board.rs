use serde::{Deserialize, Serialize};
use taco_protocol::Board;

/// The agent's line on what it is doing now, kept with the session that wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NowLine {
    pub session_id: String,
    pub text: String,
}

/// What a terminal's state file holds, beside its log.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TerminalBoard {
    pub now: Option<NowLine>,
}

impl TerminalBoard {
    /// The board a state file holds; an unreadable file is an empty board.
    pub fn from_text(text: Option<String>) -> TerminalBoard {
        let Some(text) = text else {
            return TerminalBoard::default();
        };
        let parsed: Result<TerminalBoard, _> = serde_json::from_str(&text);
        let Ok(board) = parsed else {
            return TerminalBoard::default();
        };
        board
    }

    pub fn text(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// The board the panel shows while `session` is the terminal's agent session: a now line from another session is hidden.
    pub fn shown(&self, session: Option<&str>) -> Board {
        let mut now = None;
        if let Some(line) = &self.now
            && session == Some(line.session_id.as_str())
        {
            now = Some(line.text.clone());
        }
        Board {
            now,
            shared: Vec::new(),
        }
    }
}
