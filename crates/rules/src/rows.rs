use std::collections::BTreeMap;

use taco_protocol::{AgentLabel, AgentRow, HarnessId, RowState, TerminalId};

const URGENCY: [RowState; 3] = [RowState::Needs, RowState::Working, RowState::Done];

/// The one state a collapsed tab group shows for the terminals in it, the most urgent, or none when all are idle.
pub fn summary_state(
    rows: &BTreeMap<TerminalId, AgentRow>,
    terminals: &[&TerminalId],
) -> Option<RowState> {
    let held = terminals.iter().filter_map(|terminal| rows.get(*terminal));
    let states: Vec<RowState> = held.map(|row| row.state).collect();
    let mut by_urgency = URGENCY.into_iter();
    by_urgency.find(|state| states.contains(state))
}

/// How many agents are waiting on the user, across every terminal.
pub fn waiting_count(rows: &BTreeMap<TerminalId, AgentRow>) -> usize {
    let waiting = rows.values().filter(|row| row.state == RowState::Needs);
    waiting.count()
}

/// Whether a row offers Fork: its agent forks and its session has a conversation to copy.
pub fn can_fork(
    rows: &BTreeMap<TerminalId, AgentRow>,
    agents: &BTreeMap<HarnessId, AgentLabel>,
    terminal: &TerminalId,
) -> bool {
    let Some(row) = rows.get(terminal) else {
        return false;
    };
    if !row.has_conversation {
        return false;
    }
    let label = agents.get(&row.harness);
    label.is_some_and(|label| label.forks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::id;

    fn row(state: RowState, has_conversation: bool) -> AgentRow {
        AgentRow {
            harness: HarnessId("claude".into()),
            session_id: "s".into(),
            state,
            state_since_ms: 0,
            has_conversation,
            question: None,
        }
    }

    #[test]
    fn a_group_shows_its_most_urgent_row_and_fork_needs_a_conversation_and_an_agent_that_forks() {
        let rows = BTreeMap::from([
            (id("a"), row(RowState::Done, true)),
            (id("b"), row(RowState::Needs, false)),
            (id("c"), row(RowState::Working, true)),
            (id("d"), row(RowState::Idle, true)),
        ]);
        let (a, b, c, d) = (id("a"), id("b"), id("c"), id("d"));

        assert_eq!(summary_state(&rows, &[&a, &b, &c]), Some(RowState::Needs));
        assert_eq!(summary_state(&rows, &[&a, &c]), Some(RowState::Working));
        assert_eq!(summary_state(&rows, &[&d, &id("shell")]), None);

        let forking = BTreeMap::from([(
            HarnessId("claude".into()),
            AgentLabel {
                name: "Claude".into(),
                forks: true,
            },
        )]);
        let not_forking = BTreeMap::from([(
            HarnessId("claude".into()),
            AgentLabel {
                name: "Claude".into(),
                forks: false,
            },
        )]);
        assert!(can_fork(&rows, &forking, &a));
        assert!(!can_fork(&rows, &forking, &b));
        assert!(!can_fork(&rows, &not_forking, &a));
        assert!(!can_fork(&rows, &forking, &id("shell")));
    }
}
