//! Which terminals keep a view: the one on screen, then the most recently shown others, eight in all.

use std::collections::HashSet;

use taco_protocol::TerminalId;

const KEPT_VIEWS: usize = 8;

/// Open terminals, most recently shown first, after `on_screen` was shown.
pub fn after_showing(
    recency: &[TerminalId],
    on_screen: Option<&TerminalId>,
    open: &HashSet<TerminalId>,
) -> Vec<TerminalId> {
    let mut order: Vec<TerminalId> = Vec::new();
    if let Some(id) = on_screen {
        order.push(id.clone());
    }
    for id in recency {
        if Some(id) != on_screen {
            order.push(id.clone());
        }
    }
    order.retain(|id| open.contains(id));
    order
}

/// The terminals that keep a view, from an order `after_showing` gave.
pub fn viewed(recency: &[TerminalId]) -> Vec<TerminalId> {
    let kept = recency.iter().take(KEPT_VIEWS);
    kept.cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(names: &[&str]) -> Vec<TerminalId> {
        names
            .iter()
            .map(|name| TerminalId(name.to_string()))
            .collect()
    }

    fn id(name: &str) -> TerminalId {
        TerminalId(name.to_string())
    }

    #[test]
    fn views_go_to_the_terminal_on_screen_then_the_most_recently_shown() {
        let all = ids(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let open: HashSet<TerminalId> = all.iter().cloned().collect();

        // Shown in order a to j: j is on screen, then i back to c; a and b lose their views.
        let mut recency = Vec::new();
        for shown in &all {
            recency = after_showing(&recency, Some(shown), &open);
        }
        assert_eq!(
            viewed(&recency),
            ids(&["j", "i", "h", "g", "f", "e", "d", "c"])
        );

        // Showing a again puts it first and drops c, the least recently shown.
        recency = after_showing(&recency, Some(&id("a")), &open);
        assert_eq!(
            viewed(&recency),
            ids(&["a", "j", "i", "h", "g", "f", "e", "d"])
        );

        // A closed terminal never keeps a view, even the most recently shown one.
        let mut fewer = open.clone();
        fewer.remove(&id("a"));
        recency = after_showing(&recency, Some(&id("j")), &fewer);
        assert_eq!(
            viewed(&recency),
            ids(&["j", "i", "h", "g", "f", "e", "d", "c"])
        );
    }
}
