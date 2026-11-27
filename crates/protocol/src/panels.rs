use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const TERMINALS_SIZE: f32 = 210.0;
const AGENT_ACTIVITY_SIZE: f32 = 340.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

/// Where every panel is, at most one open per edge. Stored as a map by panel, and a panel the map does not name is
/// in its default place, so a panel added later leaves the others where they were.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<Panel, PanelPlace>",
    into = "BTreeMap<Panel, PanelPlace>"
)]
pub struct Panels {
    places: [PanelPlace; Panel::ALL.len()],
}

impl Panel {
    /// Every panel, in the order the window draws them.
    pub const ALL: [Panel; 2] = [Panel::Terminals, Panel::AgentActivity];

    /// Where the panel is before the user moves it.
    pub fn default_place(self) -> PanelPlace {
        match self {
            Panel::Terminals => PanelPlace {
                edge: Edge::Left,
                open: true,
                size: TERMINALS_SIZE,
            },
            Panel::AgentActivity => PanelPlace {
                edge: Edge::Right,
                open: true,
                size: AGENT_ACTIVITY_SIZE,
            },
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

impl Panels {
    pub fn place(&self, panel: Panel) -> &PanelPlace {
        &self.places[panel.index()]
    }

    pub fn place_mut(&mut self, panel: Panel) -> &mut PanelPlace {
        &mut self.places[panel.index()]
    }
}

impl Default for Panels {
    fn default() -> Self {
        let places = Panel::ALL.map(Panel::default_place);
        Panels { places }
    }
}

impl From<BTreeMap<Panel, PanelPlace>> for Panels {
    fn from(stored: BTreeMap<Panel, PanelPlace>) -> Self {
        let mut panels = Panels::default();
        for (panel, place) in stored {
            *panels.place_mut(panel) = place;
        }
        panels
    }
}

impl From<Panels> for BTreeMap<Panel, PanelPlace> {
    fn from(panels: Panels) -> Self {
        let places = Panel::ALL.into_iter().zip(panels.places);
        places.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panel_is_stored_by_its_name_and_one_a_file_leaves_out_is_in_its_default_place() {
        for (index, panel) in Panel::ALL.into_iter().enumerate() {
            assert_eq!(panel.index(), index, "ALL lists the panels as declared");
        }
        let stored = r#"{"AgentActivity":{"edge":"Bottom","open":false,"size":120.0}}"#;
        let panels: Panels = serde_json::from_str(stored).unwrap();
        assert_eq!(panels.place(Panel::AgentActivity).edge, Edge::Bottom);
        assert_eq!(
            *panels.place(Panel::Terminals),
            Panel::Terminals.default_place()
        );
    }
}
