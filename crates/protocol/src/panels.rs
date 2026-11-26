use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    Terminals,
    AgentActivity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edge {
    Left,
    Right,
    Bottom,
}

/// Where one panel is docked. `size` is its width, or its height on the bottom edge, in points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelPlace {
    pub edge: Edge,
    pub open: bool,
    pub size: f32,
}

/// At most one open panel per edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Panels {
    pub terminals: PanelPlace,
    pub agent_activity: PanelPlace,
}
