use std::collections::BTreeMap;

use taco_protocol::{AgentRow, AgentSignal, HarnessId, RowState, TerminalId};

pub type AgentRows = BTreeMap<TerminalId, AgentRow>;

/// One hook signal from the agent in a terminal, and whether that terminal is the one on screen.
pub struct Signalled<'a> {
    pub terminal: &'a TerminalId,
    pub harness: &'a HarnessId,
    pub session_id: &'a str,
    pub signal: AgentSignal,
    pub on_screen: bool,
    pub now_ms: u64,
}

fn state_after(signal: AgentSignal, on_screen: bool) -> RowState {
    match signal {
        // a turn that ends on screen has been seen, so it is not marked done
        AgentSignal::Stopped if on_screen => RowState::Idle,
        AgentSignal::Stopped => RowState::Done,
        AgentSignal::Needs => RowState::Needs,
        _ => RowState::Working,
    }
}

fn set_state(row: &mut AgentRow, state: RowState, now_ms: u64) {
    if row.state != state {
        row.state = state;
        row.state_since_ms = now_ms;
    }
    if state != RowState::Needs {
        row.question = None;
    }
}

/// Applies a signal. Only the terminal's current session moves its row; a start of another session replaces it.
pub fn apply_signal(rows: &mut AgentRows, signalled: Signalled) {
    let current = rows.get(signalled.terminal);
    let is_current = current.is_some_and(|row| {
        row.harness == *signalled.harness && row.session_id == signalled.session_id
    });

    match signalled.signal {
        AgentSignal::Start | AgentSignal::Resume => {
            if is_current {
                return;
            }
            let row = AgentRow {
                harness: signalled.harness.clone(),
                session_id: signalled.session_id.to_string(),
                state: RowState::Idle,
                state_since_ms: signalled.now_ms,
                has_conversation: signalled.signal == AgentSignal::Resume,
                question: None,
            };
            rows.insert(signalled.terminal.clone(), row);
        }
        _ if !is_current => {}
        AgentSignal::End => {
            rows.remove(signalled.terminal);
        }
        signal => {
            let Some(row) = rows.get_mut(signalled.terminal) else {
                return;
            };
            let state = state_after(signal, signalled.on_screen);
            set_state(row, state, signalled.now_ms);
            // every signal left comes from a turn, and a turn begins with a prompt
            row.has_conversation = true;
        }
    }
}

/// A done row is idle once its terminal comes on screen.
pub fn apply_on_screen(rows: &mut AgentRows, on_screen: Option<&TerminalId>, now_ms: u64) {
    let Some(terminal) = on_screen else {
        return;
    };
    let Some(row) = rows.get_mut(terminal) else {
        return;
    };
    if row.state == RowState::Done {
        set_state(row, RowState::Idle, now_ms);
    }
}

/// The fallback for an interrupted turn, which sends no Stop: an idle title ends working and needs.
pub fn apply_idle_title(rows: &mut AgentRows, terminal: &TerminalId, now_ms: u64) {
    let Some(row) = rows.get_mut(terminal) else {
        return;
    };
    if row.state == RowState::Working || row.state == RowState::Needs {
        set_state(row, RowState::Idle, now_ms);
    }
}

/// Keeps the question of the row's current session; it is cleared when the row stops needing you.
pub fn apply_question(
    rows: &mut AgentRows,
    terminal: &TerminalId,
    session_id: &str,
    question: String,
) {
    let Some(row) = rows.get_mut(terminal) else {
        return;
    };
    if row.session_id == session_id {
        row.question = Some(question);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal(name: &str) -> TerminalId {
        TerminalId(name.into())
    }

    fn signal(
        rows: &mut AgentRows,
        name: &str,
        session_id: &str,
        signal: AgentSignal,
        on_screen: bool,
        now_ms: u64,
    ) {
        let terminal = terminal(name);
        let harness = HarnessId("claude".into());
        let signalled = Signalled {
            terminal: &terminal,
            harness: &harness,
            session_id,
            signal,
            on_screen,
            now_ms,
        };
        apply_signal(rows, signalled);
    }

    fn state(rows: &AgentRows, name: &str) -> Option<(RowState, u64, bool)> {
        let row = rows.get(&terminal(name))?;
        Some((row.state, row.state_since_ms, row.has_conversation))
    }

    #[test]
    fn a_row_follows_only_its_current_session_and_is_done_only_off_screen() {
        let mut rows = AgentRows::new();
        signal(&mut rows, "a", "s1", AgentSignal::Start, false, 1);
        assert_eq!(state(&rows, "a"), Some((RowState::Idle, 1, false)));

        signal(&mut rows, "a", "s1", AgentSignal::Working, false, 2);
        signal(&mut rows, "a", "s1", AgentSignal::Working, false, 3);
        assert_eq!(state(&rows, "a"), Some((RowState::Working, 2, true)));
        signal(&mut rows, "a", "other", AgentSignal::Stopped, false, 4);
        assert_eq!(state(&rows, "a"), Some((RowState::Working, 2, true)));

        signal(&mut rows, "a", "s1", AgentSignal::Stopped, false, 5);
        assert_eq!(state(&rows, "a"), Some((RowState::Done, 5, true)));
        apply_on_screen(&mut rows, Some(&terminal("a")), 6);
        assert_eq!(state(&rows, "a"), Some((RowState::Idle, 6, true)));
        signal(&mut rows, "a", "s1", AgentSignal::Stopped, true, 7);
        assert_eq!(state(&rows, "a"), Some((RowState::Idle, 6, true)));

        signal(&mut rows, "a", "s1", AgentSignal::Start, false, 8);
        assert_eq!(state(&rows, "a"), Some((RowState::Idle, 6, true)));
        signal(&mut rows, "a", "s2", AgentSignal::Resume, false, 9);
        assert_eq!(state(&rows, "a"), Some((RowState::Idle, 9, true)));
        signal(&mut rows, "a", "s2", AgentSignal::End, false, 10);
        assert_eq!(state(&rows, "a"), None);
    }

    #[test]
    fn an_idle_title_ends_working_and_needs_and_a_question_lasts_only_while_the_row_needs_you() {
        let mut rows = AgentRows::new();
        signal(&mut rows, "a", "s1", AgentSignal::Start, false, 0);

        // the payload can arrive before the needs signal for the same hook
        apply_question(&mut rows, &terminal("a"), "s1", "Which branch?".into());
        signal(&mut rows, "a", "s1", AgentSignal::Needs, false, 1);
        assert_eq!(rows_question(&rows), Some("Which branch?".into()));
        apply_idle_title(&mut rows, &terminal("a"), 2);
        assert_eq!(state(&rows, "a"), Some((RowState::Idle, 2, true)));
        assert_eq!(rows_question(&rows), None);

        signal(&mut rows, "a", "s1", AgentSignal::Needs, false, 3);
        apply_question(&mut rows, &terminal("a"), "s1", "Allow Bash?".into());
        apply_question(&mut rows, &terminal("a"), "old", "Stale?".into());
        assert_eq!(rows_question(&rows), Some("Allow Bash?".into()));
        signal(&mut rows, "a", "s1", AgentSignal::Working, false, 4);
        assert_eq!(rows_question(&rows), None);

        signal(&mut rows, "a", "s1", AgentSignal::Stopped, false, 5);
        apply_idle_title(&mut rows, &terminal("a"), 6);
        assert_eq!(state(&rows, "a"), Some((RowState::Done, 5, true)));
    }

    fn rows_question(rows: &AgentRows) -> Option<String> {
        let row = rows.get(&terminal("a"))?;
        row.question.clone()
    }
}
