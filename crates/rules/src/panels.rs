use taco_protocol::{Edge, Panel, Panels};

pub fn default_panels() -> Panels {
    Panels::default()
}

/// The panel open on this edge, or none when the edge shows none.
pub fn open_on(panels: &Panels, edge: Edge) -> Option<Panel> {
    let mut all = Panel::ALL.into_iter();
    all.find(|panel| {
        let place = panels.place(*panel);
        place.open && place.edge == edge
    })
}

// One open panel per edge: the panel just opened or moved there closes any other.
fn close_others_on_its_edge(panels: &mut Panels, panel: Panel) {
    let edge = panels.place(panel).edge;
    for other in Panel::ALL {
        let place = panels.place_mut(other);
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

/// The panel opened on the edge it was last on, closing any other panel open there.
pub fn open_panel(panels: &Panels, panel: Panel) -> Option<Panels> {
    let mut after = panels.clone();
    after.place_mut(panel).open = true;
    close_others_on_its_edge(&mut after, panel);
    changed(panels, after)
}

pub fn close_panel(panels: &Panels, panel: Panel) -> Option<Panels> {
    let mut after = panels.clone();
    after.place_mut(panel).open = false;
    changed(panels, after)
}

/// The panel docked on another edge, open or closed as it was; remembered for the next time it opens.
pub fn move_panel(panels: &Panels, panel: Panel, edge: Edge) -> Option<Panels> {
    let mut after = panels.clone();
    let place = after.place_mut(panel);
    place.edge = edge;
    if place.open {
        close_others_on_its_edge(&mut after, panel);
    }
    changed(panels, after)
}

/// A size that is not a positive number is refused; the window clamps the size to its own bounds before sending it.
pub fn resize_panel(panels: &Panels, panel: Panel, size: f32) -> Option<Panels> {
    if !size.is_finite() || size <= 0.0 {
        return None;
    }
    let mut after = panels.clone();
    after.place_mut(panel).size = size;
    changed(panels, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_open_panel_per_edge_and_each_remembers_its_edge_while_closed() {
        let start = default_panels();
        let open = |panels: &Panels, panel: Panel| panels.place(panel).open;

        let moved = move_panel(&start, Panel::AgentActivity, Edge::Left).unwrap();
        assert_eq!(
            (
                open(&moved, Panel::AgentActivity),
                open(&moved, Panel::Terminals)
            ),
            (true, false)
        );
        assert_eq!(open_on(&moved, Edge::Left), Some(Panel::AgentActivity));
        assert_eq!(open_on(&moved, Edge::Right), None);

        let closed = close_panel(&moved, Panel::AgentActivity).unwrap();
        let reopened = open_panel(&closed, Panel::AgentActivity).unwrap();
        assert_eq!(reopened.place(Panel::AgentActivity).edge, Edge::Left);

        let terminals_back = open_panel(&reopened, Panel::Terminals).unwrap();
        assert_eq!(
            (
                open(&terminals_back, Panel::Terminals),
                open(&terminals_back, Panel::AgentActivity)
            ),
            (true, false)
        );

        let moved_closed = move_panel(&terminals_back, Panel::AgentActivity, Edge::Bottom).unwrap();
        let moved_back_closed =
            move_panel(&moved_closed, Panel::AgentActivity, Edge::Left).unwrap();
        assert!(open(&moved_back_closed, Panel::Terminals));

        assert_eq!(resize_panel(&start, Panel::Terminals, f32::NAN), None);
        assert_eq!(resize_panel(&start, Panel::Terminals, 0.0), None);
        assert_eq!(open_panel(&start, Panel::Terminals), None);
    }
}
