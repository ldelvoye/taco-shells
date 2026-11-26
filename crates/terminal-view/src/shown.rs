//! Which terminals keep a view: every one on screen, then the most recently shown others, eight in all.

use std::collections::HashSet;

use taco_protocol::TerminalId;

pub const KEPT_VIEWS: usize = 8;

/// `recency` after `on_screen` was shown: those first, then the rest in their old order. Closed terminals drop out.
pub fn after_showing(
    recency: &[TerminalId],
    on_screen: &[TerminalId],
    open: &HashSet<TerminalId>,
) -> Vec<TerminalId> {
    let mut order: Vec<TerminalId> = on_screen.to_vec();
    for id in recency {
        if !on_screen.contains(id) {
            order.push(id.clone());
        }
    }
    order.retain(|id| open.contains(id));
    order
}

/// The terminals that should have a view, given `recency` from `after_showing`: all on screen however many, then the next most recent up to the limit.
pub fn viewed(
    recency: &[TerminalId],
    on_screen: &[TerminalId],
    open: &HashSet<TerminalId>,
) -> Vec<TerminalId> {
    let mut wanted: Vec<TerminalId> = Vec::new();
    for id in on_screen {
        if open.contains(id) && !wanted.contains(id) {
            wanted.push(id.clone());
        }
    }
    for id in recency {
        if wanted.len() >= KEPT_VIEWS {
            break;
        }
        if open.contains(id) && !wanted.contains(id) {
            wanted.push(id.clone());
        }
    }
    wanted
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

    #[test]
    fn views_go_to_the_terminal_on_screen_then_the_most_recent_up_to_eight() {
        let all = ids(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
        let open: HashSet<TerminalId> = all.iter().cloned().collect();

        // Shown in order a to j: j is on screen, then i back to c fill the other seven; a and b lose theirs.
        let mut recency = Vec::new();
        for id in &all {
            recency = after_showing(&recency, std::slice::from_ref(id), &open);
        }
        let on_screen = ids(&["j"]);
        let kept = viewed(&recency, &on_screen, &open);
        assert_eq!(kept, ids(&["j", "i", "h", "g", "f", "e", "d", "c"]));

        // Showing a again puts it first and drops c, the least recent.
        recency = after_showing(&recency, &ids(&["a"]), &open);
        let kept = viewed(&recency, &ids(&["a"]), &open);
        assert_eq!(kept, ids(&["a", "j", "i", "h", "g", "f", "e", "d"]));

        // A closed terminal never keeps a view, even when it was the most recent.
        let mut fewer = open.clone();
        fewer.remove(&TerminalId("a".to_string()));
        recency = after_showing(&recency, &ids(&["j"]), &fewer);
        let kept = viewed(&recency, &ids(&["j"]), &fewer);
        assert!(!kept.contains(&TerminalId("a".to_string())));
        assert_eq!(kept.len(), KEPT_VIEWS);
    }
}
