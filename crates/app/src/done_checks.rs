//! Which done checks are drawing themselves: once, for a short while after a row's agent finishes.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use taco_protocol::{DrawnState, RowState, TerminalId};

/// How long a check takes to draw itself: tacoshells.dev's ts-draw, 8% of its 9 s cycle.
pub const DRAW_TIME: Duration = Duration::from_millis(720);

/// When each check that is still drawing itself started.
#[derive(Debug, Default)]
pub struct DoneChecks {
    started: HashMap<TerminalId, Instant>,
}

impl DoneChecks {
    /// Starts a check for every row that has just finished, between one published state and the next.
    pub fn note(&mut self, before: &DrawnState, after: &DrawnState, now: Instant) {
        let finished = newly_done(before, after);
        for terminal in finished {
            self.started.insert(terminal, now);
        }
        self.started
            .retain(|_, started| now.duration_since(*started) < DRAW_TIME);
    }

    /// The rows whose check is still drawing itself at `now`.
    pub fn drawing(&self, now: Instant) -> HashSet<TerminalId> {
        let still = self
            .started
            .iter()
            .filter(|(_, started)| now.duration_since(**started) < DRAW_TIME);
        let terminals = still.map(|(terminal, _)| terminal.clone());
        terminals.collect()
    }
}

/// The rows done in `after` that were working, waiting or idle in `before`; a row that arrives already done, as restored ones do, is not one.
pub fn newly_done(before: &DrawnState, after: &DrawnState) -> Vec<TerminalId> {
    let mut finished = Vec::new();
    for (terminal, row) in &after.agent_rows {
        if row.state != RowState::Done {
            continue;
        }
        let Some(earlier) = before.agent_rows.get(terminal) else {
            continue;
        };
        if earlier.state != RowState::Done {
            finished.push(terminal.clone());
        }
    }
    finished
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::{Duration, Instant};

    use taco_protocol::{AgentRow, DrawnState, HarnessId, RowState, TerminalId};
    use taco_rules::default_panels;

    use super::{DRAW_TIME, DoneChecks, newly_done};

    fn terminal(name: &str) -> TerminalId {
        TerminalId(name.to_string())
    }

    fn drawn(states: &[(&str, RowState)]) -> DrawnState {
        let rows = states.iter().map(|(name, state)| {
            let row = AgentRow {
                harness: HarnessId("claude".to_string()),
                session_id: name.to_string(),
                state: *state,
                state_since_ms: 0,
                has_conversation: true,
                question: None,
            };
            (terminal(name), row)
        });
        DrawnState {
            revision: 1,
            arrangement: None,
            agent_rows: rows.collect(),
            directories: BTreeMap::new(),
            refocus: 0,
            focus_count: 0,
            agents: BTreeMap::new(),
            default_agent_name: String::new(),
            panels: default_panels(),
            problem: None,
            started: BTreeSet::new(),
        }
    }

    #[test]
    fn a_check_draws_once_when_its_row_finishes_and_never_for_a_row_already_done() {
        let restored = drawn(&[("a", RowState::Done), ("b", RowState::Working)]);
        let launch = drawn(&[]);
        assert!(
            newly_done(&launch, &restored).is_empty(),
            "restored rows arrive done"
        );

        let finished = drawn(&[("a", RowState::Done), ("b", RowState::Done)]);
        assert_eq!(newly_done(&restored, &finished), [terminal("b")]);
        assert!(
            newly_done(&finished, &finished).is_empty(),
            "a redraw is not a finish"
        );

        let mut checks = DoneChecks::default();
        let now = Instant::now();
        checks.note(&restored, &finished, now);
        let halfway = now + DRAW_TIME / 2;
        assert_eq!(checks.drawing(halfway).len(), 1);
        let after = now + DRAW_TIME + Duration::from_millis(1);
        assert!(checks.drawing(after).is_empty(), "it stops once drawn");
        checks.note(&finished, &finished, after);
        assert!(checks.drawing(after).is_empty(), "and does not start again");
    }
}
