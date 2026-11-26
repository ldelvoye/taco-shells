use std::collections::BTreeMap;

use taco_protocol::{HarnessId, TerminalId};

/// How long a restored session waits for its agent's start before it is forgotten.
pub const PROVISIONAL_MS: u64 = 60_000;

// An agent killed without its end hook leaves its session behind, so the oldest go past this many.
const MAX_SESSIONS_PER_TERMINAL: usize = 16;

/// An agent session a terminal can resume: which agent, its id, and the directory it runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSession {
    pub harness: HarnessId,
    pub session_id: String,
    pub cwd: String,
}

#[derive(Clone, Debug)]
struct Entry {
    session: AgentSession,
    /// When a restored session was seeded, until its start confirms it.
    provisional_since: Option<u64>,
}

/// The agent sessions each terminal has run, newest last; the newest is the one it resumes or forks.
#[derive(Default)]
pub struct AgentSessions {
    by_terminal: BTreeMap<TerminalId, Vec<Entry>>,
    frozen: bool,
}

impl AgentSessions {
    pub fn record_start(&mut self, terminal: &TerminalId, session: AgentSession) {
        if self.frozen {
            return;
        }
        let entries = self.by_terminal.entry(terminal.clone()).or_default();
        let mut known = entries.iter_mut();
        let existing = known.find(|entry| {
            entry.session.harness == session.harness
                && entry.session.session_id == session.session_id
        });
        if let Some(entry) = existing {
            entry.provisional_since = None;
            return;
        }
        let entry = Entry {
            session,
            provisional_since: None,
        };
        push_capped(entries, entry);
    }

    pub fn record_end(&mut self, terminal: &TerminalId, harness: &HarnessId, session_id: &str) {
        if self.frozen {
            return;
        }
        let Some(entries) = self.by_terminal.get_mut(terminal) else {
            return;
        };
        entries.retain(|entry| {
            !(entry.session.harness == *harness && entry.session.session_id == session_id)
        });
        if entries.is_empty() {
            self.by_terminal.remove(terminal);
        }
    }

    /// A restored session, kept only until its start confirms it or `PROVISIONAL_MS` passes.
    pub fn seed(&mut self, terminal: &TerminalId, session: AgentSession, now_ms: u64) {
        if self.frozen {
            return;
        }
        let entries = self.by_terminal.entry(terminal.clone()).or_default();
        let entry = Entry {
            session,
            provisional_since: Some(now_ms),
        };
        push_capped(entries, entry);
    }

    /// The newest session; an older one may belong to an agent that died without its end hook.
    pub fn session_of(&self, terminal: &TerminalId) -> Option<&AgentSession> {
        let entries = self.by_terminal.get(terminal)?;
        let newest = entries.last()?;
        Some(&newest.session)
    }

    pub fn forget(&mut self, terminal: &TerminalId) {
        if self.frozen {
            return;
        }
        self.by_terminal.remove(terminal);
    }

    /// When the first restored session still waiting for its start is due to be forgotten.
    pub fn next_expiry(&self) -> Option<u64> {
        let entries = self.by_terminal.values().flatten();
        let seeded = entries.filter_map(|entry| entry.provisional_since);
        let earliest = seeded.min()?;
        Some(earliest + PROVISIONAL_MS + 1)
    }

    /// Forgets restored sessions whose agent never started again. Says whether it forgot any.
    pub fn drop_expired(&mut self, now_ms: u64) -> bool {
        if self.frozen {
            return false;
        }
        let before = self.by_terminal.values().map(Vec::len).sum::<usize>();
        for entries in self.by_terminal.values_mut() {
            entries.retain(|entry| match entry.provisional_since {
                Some(since) => now_ms.saturating_sub(since) <= PROVISIONAL_MS,
                None => true,
            });
        }
        self.by_terminal.retain(|_, entries| !entries.is_empty());
        let after = self.by_terminal.values().map(Vec::len).sum::<usize>();
        after != before
    }

    /// Nothing changes after this: what is saved has to stay as the terminals were.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

fn push_capped(entries: &mut Vec<Entry>, entry: Entry) {
    entries.push(entry);
    if entries.len() > MAX_SESSIONS_PER_TERMINAL {
        entries.remove(0);
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
    fn a_terminal_resumes_its_newest_session_through_clear_expiry_and_freeze() {
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

        let restored = TerminalId("r".into());
        sessions.seed(&restored, session("kept"), 0);
        sessions.record_start(&restored, session("kept"));
        let never_started = TerminalId("n".into());
        sessions.seed(&never_started, session("lost"), 0);
        sessions.drop_expired(PROVISIONAL_MS);
        assert_eq!(resumed(&sessions, &never_started).as_deref(), Some("lost"));
        sessions.drop_expired(PROVISIONAL_MS + 1);
        assert_eq!(resumed(&sessions, &restored).as_deref(), Some("kept"));
        assert_eq!(resumed(&sessions, &never_started), None);

        let seeded_late = TerminalId("late".into());
        sessions.seed(&seeded_late, session("waiting"), PROVISIONAL_MS);
        assert_eq!(sessions.next_expiry(), Some(2 * PROVISIONAL_MS + 1));
        sessions.freeze();
        sessions.drop_expired(10 * PROVISIONAL_MS);
        assert_eq!(resumed(&sessions, &seeded_late).as_deref(), Some("waiting"));
        sessions.forget(&terminal);
        sessions.record_end(&restored, &claude, "kept");
        assert_eq!(resumed(&sessions, &terminal).as_deref(), Some("after"));
        assert_eq!(resumed(&sessions, &restored).as_deref(), Some("kept"));
    }
}
