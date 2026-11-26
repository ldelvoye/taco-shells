use std::collections::BTreeMap;

use taco_protocol::{HarnessId, TerminalId};

// An agent killed without its end hook leaves its session behind, so the oldest go past this many.
const MAX_SESSIONS_PER_TERMINAL: usize = 16;

/// An agent session a terminal can resume: which agent, its id, and the directory it runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSession {
    pub harness: HarnessId,
    pub session_id: String,
    pub cwd: String,
}

/// The agent sessions each terminal has run, newest last; the newest is the one it resumes or forks.
#[derive(Default)]
pub struct AgentSessions {
    by_terminal: BTreeMap<TerminalId, Vec<AgentSession>>,
    frozen: bool,
}

impl AgentSessions {
    pub fn record_start(&mut self, terminal: &TerminalId, session: AgentSession) {
        if self.frozen {
            return;
        }
        let sessions = self.by_terminal.entry(terminal.clone()).or_default();
        let known = sessions.iter().any(|known| {
            known.harness == session.harness && known.session_id == session.session_id
        });
        if known {
            return;
        }
        sessions.push(session);
        if sessions.len() > MAX_SESSIONS_PER_TERMINAL {
            sessions.remove(0);
        }
    }

    pub fn record_end(&mut self, terminal: &TerminalId, harness: &HarnessId, session_id: &str) {
        if self.frozen {
            return;
        }
        let Some(sessions) = self.by_terminal.get_mut(terminal) else {
            return;
        };
        sessions.retain(|known| !(known.harness == *harness && known.session_id == session_id));
        if sessions.is_empty() {
            self.by_terminal.remove(terminal);
        }
    }

    /// The newest session; an older one may belong to an agent that died without its end hook.
    pub fn session_of(&self, terminal: &TerminalId) -> Option<&AgentSession> {
        let sessions = self.by_terminal.get(terminal)?;
        sessions.last()
    }

    pub fn forget(&mut self, terminal: &TerminalId) {
        if self.frozen {
            return;
        }
        self.by_terminal.remove(terminal);
    }

    /// Nothing changes after this: what is saved has to stay as the terminals were.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> AgentSession {
        AgentSession {
            harness: HarnessId("claude".into()),
            session_id: id.into(),
            cwd: "/tmp".into(),
        }
    }

    fn resumed(sessions: &AgentSessions, terminal: &TerminalId) -> Option<String> {
        let found = sessions.session_of(terminal)?;
        Some(found.session_id.clone())
    }

    #[test]
    fn a_terminal_resumes_its_newest_session_through_clear_and_freeze() {
        let terminal = TerminalId("t".into());
        let claude = HarnessId("claude".into());
        let mut sessions = AgentSessions::default();

        sessions.record_start(&terminal, session("old"));
        sessions.record_start(&terminal, session("new"));
        sessions.record_start(&terminal, session("old"));
        assert_eq!(resumed(&sessions, &terminal).as_deref(), Some("new"));
        sessions.record_end(&terminal, &claude, "new");
        assert_eq!(resumed(&sessions, &terminal).as_deref(), Some("old"));

        // /clear ends one session and starts another, in either order
        sessions.record_start(&terminal, session("cleared"));
        sessions.record_end(&terminal, &claude, "old");
        assert_eq!(resumed(&sessions, &terminal).as_deref(), Some("cleared"));
        sessions.record_end(&terminal, &claude, "cleared");
        sessions.record_start(&terminal, session("after"));
        assert_eq!(resumed(&sessions, &terminal).as_deref(), Some("after"));

        sessions.freeze();
        sessions.forget(&terminal);
        sessions.record_end(&terminal, &claude, "after");
        assert_eq!(resumed(&sessions, &terminal).as_deref(), Some("after"));
    }
}
