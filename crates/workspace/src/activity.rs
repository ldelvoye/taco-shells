use std::collections::BTreeMap;

use taco_agents::{EntryRead, Found, PullRequestSeen};
use taco_protocol::{
    ActivityEntry, ActivityEvent, ActivityKind, ActivityRequest, Author, Board, SharedThing,
    StartSource, TerminalId,
};

use crate::asking::{context_text, nudge_text, start_source};
use crate::board::{NowLine, Seen, Sharing, TerminalBoard};
use crate::log::{Log, line_of};
use crate::outbound::Outbound;
use crate::storage::Storage;

/// Agent Activity for every terminal: the logs, the boards and the terminal the panel watches.
#[derive(Default)]
pub(crate) struct Activity {
    logs: BTreeMap<TerminalId, Log>,
    boards: BTreeMap<TerminalId, TerminalBoard>,
    /// The terminal Agent Activity shows; it is sent each entry as it is logged.
    watched: Option<TerminalId>,
}

// A terminal's board is read from its state file the first time it is needed.
fn loaded_board<'a>(
    boards: &'a mut BTreeMap<TerminalId, TerminalBoard>,
    storage: &dyn Storage,
    terminal: &TerminalId,
) -> &'a mut TerminalBoard {
    let entry = boards.entry(terminal.clone());
    entry.or_insert_with(|| {
        let text = storage.read_state(terminal);
        TerminalBoard::from_text(text)
    })
}

// A terminal's log is read from storage the first time it is needed.
fn loaded_log<'a>(
    logs: &'a mut BTreeMap<TerminalId, Log>,
    storage: &dyn Storage,
    terminal: &TerminalId,
) -> &'a mut Log {
    let entry = logs.entry(terminal.clone());
    entry.or_insert_with(|| {
        let lines = storage.read_log(terminal);
        Log::from_lines(&lines)
    })
}

fn write_log(
    storage: &mut dyn Storage,
    terminal: &TerminalId,
    log: &Log,
    entry: &ActivityEntry,
    rewrite: bool,
) -> bool {
    if rewrite {
        let lines = log.lines();
        return storage.rewrite_log(terminal, &lines);
    }
    let Some(line) = line_of(entry) else {
        return false;
    };
    storage.append_log(terminal, &line)
}

impl Activity {
    pub(crate) fn session_context(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        session_id: &str,
        source: &str,
        now_ms: u64,
        out: &mut Vec<Outbound>,
    ) -> Option<String> {
        let source = start_source(source);
        let started = ActivityKind::SessionStarted { source };
        self.log_entry(storage, terminal, out, |log| {
            log.append(Author::App, started, now_ms)
        });
        if source != StartSource::Compacted {
            return None;
        }
        let board = loaded_board(&mut self.boards, storage, terminal);
        let shown = board.shown(Some(session_id));
        let log = loaded_log(&mut self.logs, storage, terminal);
        let notes = log.session_notes_newest_first();
        context_text(shown.now.as_deref(), &notes)
    }

    pub(crate) fn stop_nudge(
        &mut self,
        storage: &dyn Storage,
        terminal: &TerminalId,
        directory: Option<&str>,
    ) -> Option<String> {
        let log = loaded_log(&mut self.logs, storage, terminal);
        let steps = log.open_steps();
        nudge_text(&steps, directory)
    }

    pub(crate) fn asked_for_note(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        now_ms: u64,
        out: &mut Vec<Outbound>,
    ) {
        self.log_entry(storage, terminal, out, |log| {
            log.append(Author::App, ActivityKind::AskedForNote, now_ms)
        });
    }

    pub(crate) fn request(
        &mut self,
        storage: &dyn Storage,
        request: ActivityRequest,
        held: bool,
        current_session: Option<&str>,
    ) -> Vec<Outbound> {
        let event = match request {
            ActivityRequest::Watch { terminal } => {
                self.watched = terminal.clone();
                let Some(terminal) = terminal else {
                    return Vec::new();
                };
                if !held {
                    return Vec::new();
                }
                let board = self.shown_board(storage, &terminal, current_session);
                let log = loaded_log(&mut self.logs, storage, &terminal);
                let page = log.newest();
                ActivityEvent::Loaded {
                    terminal,
                    entries: page.entries,
                    complete: page.complete,
                    board,
                }
            }
            ActivityRequest::Older { terminal, before } => {
                if !held {
                    return Vec::new();
                }
                let log = loaded_log(&mut self.logs, storage, &terminal);
                let page = log.older_than(before);
                ActivityEvent::Older {
                    terminal,
                    entries: page.entries,
                    complete: page.complete,
                }
            }
            ActivityRequest::Body { terminal, thing } => {
                if !held {
                    return Vec::new();
                }
                let board = loaded_board(&mut self.boards, storage, &terminal);
                let body = board.body_of(thing);
                ActivityEvent::Body {
                    terminal,
                    thing,
                    body,
                }
            }
        };
        vec![Outbound::Activity(event)]
    }

    pub(crate) fn terminal_exited(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        frozen: bool,
    ) {
        // a frozen shutdown, a quit or the runtime dying, keeps the logs: the next launch reopens these terminals
        if !frozen {
            self.logs.remove(terminal);
            self.boards.remove(terminal);
            storage.delete_log(terminal);
            storage.delete_state(terminal);
        }
        if self.watched.as_ref() == Some(terminal) {
            self.watched = None;
        }
    }

    pub(crate) fn agent_started(
        &mut self,
        storage: &dyn Storage,
        terminal: &TerminalId,
        session_id: &str,
        current_session: Option<&str>,
        shown_before: Option<Board>,
        out: &mut Vec<Outbound>,
    ) {
        // a new session hides the last one's now line
        self.board_changed(storage, terminal, current_session, shown_before, out);
        let log = loaded_log(&mut self.logs, storage, terminal);
        log.session_started(session_id);
    }

    pub(crate) fn read_into_activity(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        session_id: &str,
        current_session: Option<&str>,
        read: EntryRead,
        now_ms: u64,
    ) -> Vec<Outbound> {
        let mut logged = Vec::new();
        let out = &mut logged;
        match read {
            EntryRead::Asked { text, turn } => {
                self.log_entry(storage, terminal, out, |log| {
                    log.append_prompt(text, turn, now_ms)
                });
            }
            EntryRead::MessageIn { from, text, turn } => {
                self.log_entry(storage, terminal, out, |log| {
                    log.append_message_in(from, text, turn, now_ms)
                });
            }
            EntryRead::Step { step, context } => {
                self.log_entry(storage, terminal, out, |log| {
                    log.append_step(step, &context, now_ms)
                });
            }
            EntryRead::MessageOut { to, text, context } => {
                self.log_entry(storage, terminal, out, |log| {
                    log.append_message_out(to, text, &context, now_ms)
                });
            }
            EntryRead::Note { text } => {
                self.log_entry(storage, terminal, out, |log| log.append_note(text, now_ms));
            }
            EntryRead::Now { text } => {
                self.set_now(storage, terminal, session_id, current_session, text, out)
            }
            EntryRead::Share {
                kind,
                title,
                target,
                body,
            } => {
                let sharing = Sharing {
                    by: Author::Agent,
                    kind,
                    title,
                    target,
                    body,
                    at_ms: now_ms,
                };
                self.share(storage, terminal, current_session, sharing, out);
            }
            EntryRead::Found(Found::Thing {
                kind,
                title,
                target,
            }) => {
                let sharing = Sharing {
                    by: Author::App,
                    kind,
                    title,
                    target: Some(target),
                    body: None,
                    at_ms: now_ms,
                };
                self.share(storage, terminal, current_session, sharing, out);
            }
            EntryRead::Found(Found::PullRequest(seen)) => {
                self.pull_request_seen(storage, terminal, current_session, seen, now_ms, out);
            }
        }
        logged
    }

    // The thing goes to the top of Shared and gets a log line of its own; only a note closes the open stretch.
    fn share(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        current_session: Option<&str>,
        sharing: Sharing,
        out: &mut Vec<Outbound>,
    ) {
        let shown_before = self.watched_board(storage, terminal, current_session);
        let at_ms = sharing.at_ms;
        let board = loaded_board(&mut self.boards, storage, terminal);
        let thing = board.share(sharing);
        if let Some(text) = board.text() {
            storage.write_state(terminal, &text);
        }
        self.board_changed(storage, terminal, current_session, shown_before, out);
        self.log_shared(storage, terminal, thing, at_ms, out);
    }

    // A pull request the agent opened joins Shared with a line in the log; one it merged or closed changes in place.
    fn pull_request_seen(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        current_session: Option<&str>,
        seen: PullRequestSeen,
        now_ms: u64,
        out: &mut Vec<Outbound>,
    ) {
        let shown_before = self.watched_board(storage, terminal, current_session);
        let board = loaded_board(&mut self.boards, storage, terminal);
        let outcome = board.pull_request_seen(seen, now_ms);
        if outcome == Seen::Nothing {
            return;
        }
        if let Some(text) = board.text() {
            storage.write_state(terminal, &text);
        }
        self.board_changed(storage, terminal, current_session, shown_before, out);
        if let Seen::Added(thing) = outcome {
            self.log_shared(storage, terminal, thing, now_ms, out);
        }
    }

    fn log_shared(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        thing: SharedThing,
        at_ms: u64,
        out: &mut Vec<Outbound>,
    ) {
        let by = thing.by.clone();
        let kind = ActivityKind::Shared {
            thing: thing.id,
            title: thing.title,
        };
        self.log_entry(storage, terminal, out, |log| log.append(by, kind, at_ms));
    }

    /// The board Agent Activity shows for a terminal: its now line only while the session that wrote it is the terminal's.
    fn shown_board(
        &mut self,
        storage: &dyn Storage,
        terminal: &TerminalId,
        current_session: Option<&str>,
    ) -> Board {
        let board = loaded_board(&mut self.boards, storage, terminal);
        board.shown(current_session)
    }

    // Taken before a change, so `board_changed` can tell whether what the watched terminal shows changed.
    pub(crate) fn watched_board(
        &mut self,
        storage: &dyn Storage,
        terminal: &TerminalId,
        current_session: Option<&str>,
    ) -> Option<Board> {
        if self.watched.as_ref() != Some(terminal) {
            return None;
        }
        Some(self.shown_board(storage, terminal, current_session))
    }

    // The watched terminal's board goes out whole whenever what it shows changes.
    fn board_changed(
        &mut self,
        storage: &dyn Storage,
        terminal: &TerminalId,
        current_session: Option<&str>,
        shown_before: Option<Board>,
        out: &mut Vec<Outbound>,
    ) {
        let Some(shown_before) = shown_before else {
            return;
        };
        let board = self.shown_board(storage, terminal, current_session);
        if board == shown_before {
            return;
        }
        let changed = ActivityEvent::BoardChanged {
            terminal: terminal.clone(),
            board,
        };
        out.push(Outbound::Activity(changed));
    }

    fn set_now(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        session_id: &str,
        current_session: Option<&str>,
        text: String,
        out: &mut Vec<Outbound>,
    ) {
        let shown_before = self.watched_board(storage, terminal, current_session);
        let board = loaded_board(&mut self.boards, storage, terminal);
        let line = NowLine {
            session_id: session_id.to_string(),
            text,
        };
        // the same line again changes nothing, so it costs no write
        if board.now.as_ref() == Some(&line) {
            return;
        }
        board.now = Some(line);
        // a failed write leaves the file behind until the next change, which writes it whole
        if let Some(text) = board.text() {
            storage.write_state(terminal, &text);
        }
        self.board_changed(storage, terminal, current_session, shown_before, out);
    }

    // The log belongs to the terminal, not to one session of its agent, so every session in it appends to the same one.
    fn log_entry(
        &mut self,
        storage: &mut dyn Storage,
        terminal: &TerminalId,
        out: &mut Vec<Outbound>,
        append: impl FnOnce(&mut Log) -> ActivityEntry,
    ) {
        let log = loaded_log(&mut self.logs, storage, terminal);
        let entry = append(log);
        let rewrite = log.needs_rewrite();
        let succeeded = write_log(storage, terminal, log, &entry, rewrite);
        log.file_written(rewrite, succeeded);
        if self.watched.as_ref() == Some(terminal) {
            let appended = ActivityEvent::Appended {
                terminal: terminal.clone(),
                entry,
            };
            out.push(Outbound::Activity(appended));
        }
    }

    pub(crate) fn drop_orphan_logs(&mut self, storage: &mut dyn Storage, held: &[TerminalId]) {
        let stored = storage.activity_terminals();
        for terminal in stored {
            if !held.contains(&terminal) {
                storage.delete_log(&terminal);
                storage.delete_state(&terminal);
            }
        }
    }
}
