use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::arrangement::{Arrangement, Dragged, DropPlace};
use crate::ids::{HarnessId, TabGroupId, TerminalId};
use crate::panels::{Edge, Panel, Panels};
use crate::rows::{AgentLabel, AgentRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigFile {
    Settings,
    Keybindings,
}

/// What the user asked for. The workspace applies it or drops it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WorkspaceRequest {
    /// A new terminal, opening `file` in the user's editor when one is given.
    OpenTerminal {
        file: Option<ConfigFile>,
    },
    OpenTerminalIn {
        tab_group: Option<TabGroupId>,
    },
    ForkTerminal {
        terminal: TerminalId,
    },
    CloseTerminal {
        terminal: TerminalId,
    },
    /// The window moved keyboard focus itself. `count` numbers its moves.
    FocusReported {
        terminal: TerminalId,
        count: u64,
    },
    Dropped {
        dragged: Dragged,
        place: DropPlace,
    },
    TabGroupCreated {
        terminal: TerminalId,
    },
    TabGroupRenamed {
        id: TabGroupId,
        name: String,
    },
    TabGroupUngrouped {
        id: TabGroupId,
    },
    TabGroupToggled {
        id: TabGroupId,
    },
    PanelOpened {
        panel: Panel,
    },
    PanelClosed {
        panel: Panel,
    },
    PanelMoved {
        panel: Panel,
        edge: Edge,
    },
    PanelResized {
        panel: Panel,
        size: f32,
    },
}

/// `ok` is false when the request was not applied, such as a fork of a terminal that cannot fork.
/// `revision` is the state that holds the request's effect, which the window waits for before acting on the reply.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestReply {
    pub ok: bool,
    pub tab_group: Option<TabGroupId>,
    pub revision: u64,
}

/// Everything the window draws, published whole each time it changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrawnState {
    pub revision: u64,
    pub arrangement: Option<Arrangement>,
    pub agent_rows: BTreeMap<TerminalId, AgentRow>,
    pub directories: BTreeMap<TerminalId, String>,
    /// A counter that goes up each time the window should hand the keyboard back to the selected terminal.
    pub refocus: u64,
    /// How many of the window's own focus moves the workspace has applied: the highest `count` it was sent.
    pub focus_count: u64,
    pub agents: BTreeMap<HarnessId, AgentLabel>,
    pub default_agent_name: String,
    pub panels: Panels,
    pub problem: Option<String>,
}
