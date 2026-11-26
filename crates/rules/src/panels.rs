use taco_protocol::{Edge, Panel, PanelPlace, Panels};

const ALL_PANELS: [Panel; 2] = [Panel::Terminals, Panel::AgentActivity];
const TERMINALS_SIZE: f32 = 210.0;
const AGENT_ACTIVITY_SIZE: f32 = 340.0;

pub fn default_panels() -> Panels {
    Panels {
        terminals: PanelPlace {
            edge: Edge::Left,
            open: true,
            size: TERMINALS_SIZE,
        },
        agent_activity: PanelPlace {
            edge: Edge::Right,
            open: true,
            size: AGENT_ACTIVITY_SIZE,
        },
    }
}

fn place_of(panels: &mut Panels, panel: Panel) -> &mut PanelPlace {
    match panel {
        Panel::Terminals => &mut panels.terminals,
        Panel::AgentActivity => &mut panels.agent_activity,
    }
}

// One open panel per edge: the panel just opened or moved there closes any other.
fn close_others_on_its_edge(panels: &mut Panels, panel: Panel) {
    let edge = place_of(panels, panel).edge;
    for other in ALL_PANELS {
        let place = place_of(panels, other);
        if other != panel && place.edge == edge {
            place.open = false;
        }
    }
}

fn changed(before: &Panels, after: Panels) -> Option<Panels> {
    if after == *before {
        return None;
    }
    Some(after)
}

/// The panel opened on the edge it was last on.
pub fn open_panel(panels: &Panels, panel: Panel) -> Option<Panels> {
    let mut after = panels.clone();
    place_of(&mut after, panel).open = true;
    close_others_on_its_edge(&mut after, panel);
    changed(panels, after)
}

pub fn close_panel(panels: &Panels, panel: Panel) -> Option<Panels> {
    let mut after = panels.clone();
    place_of(&mut after, panel).open = false;
    changed(panels, after)
}

/// The panel docked on another edge, open or closed as it was; remembered for the next time it opens.
pub fn move_panel(panels: &Panels, panel: Panel, edge: Edge) -> Option<Panels> {
    let mut after = panels.clone();
    let place = place_of(&mut after, panel);
    place.edge = edge;
    if place.open {
        close_others_on_its_edge(&mut after, panel);
    }
    changed(panels, after)
}

/// A size that is not a positive number is refused.
pub fn resize_panel(panels: &Panels, panel: Panel, size: f32) -> Option<Panels> {
    if !size.is_finite() || size <= 0.0 {
        return None;
    }
    let mut after = panels.clone();
    place_of(&mut after, panel).size = size;
    changed(panels, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_open_panel_per_edge_and_each_remembers_its_edge_while_closed() {
        let start = default_panels();

        let moved = move_panel(&start, Panel::AgentActivity, Edge::Left).unwrap();
        assert_eq!(
            (moved.agent_activity.open, moved.terminals.open),
            (true, false)
        );

        let closed = close_panel(&moved, Panel::AgentActivity).unwrap();
        let reopened = open_panel(&closed, Panel::AgentActivity).unwrap();
        assert_eq!(reopened.agent_activity.edge, Edge::Left);

        let terminals_back = open_panel(&reopened, Panel::Terminals).unwrap();
        assert_eq!(
            (
                terminals_back.terminals.open,
                terminals_back.agent_activity.open
            ),
            (true, false)
        );

        let moved_closed = move_panel(&terminals_back, Panel::AgentActivity, Edge::Bottom).unwrap();
        let moved_back_closed =
            move_panel(&moved_closed, Panel::AgentActivity, Edge::Left).unwrap();
        assert!(moved_back_closed.terminals.open);

        assert_eq!(resize_panel(&start, Panel::Terminals, f32::NAN), None);
        assert_eq!(open_panel(&start, Panel::Terminals), None);
    }
}
