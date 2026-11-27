use std::collections::VecDeque;

use taco_agents::StepContext;
use taco_protocol::{ActivityEntry, ActivityKind, Author, Peer, StartSource, Step};

/// How many entries other than steps a terminal's log keeps; older ones go as new ones come.
pub const KEPT_ENTRIES: usize = 1_000;
/// How many steps it keeps, counted apart so a long run of steps never pushes the agent's notes out.
pub const KEPT_STEPS: usize = 2_000;
/// How many entries the window is sent at a time.
pub const PAGE: usize = 200;
/// The file is rewritten down to the kept entries once it passes this many lines, twice what is kept.
pub const REWRITE_AT_LINES: usize = 2 * (KEPT_ENTRIES + KEPT_STEPS);
// How many turns' last stretches a subagent's late step can still find.
const TURNS_REMEMBERED: usize = 8;

/// One terminal's Agent Activity, oldest first.
#[derive(Debug, Default)]
pub struct Log {
    entries: VecDeque<ActivityEntry>,
    steps_kept: usize,
    next_seq: u64,
    lines_in_file: usize,
    /// The file holds lines the log does not keep, or lacks one a failed write lost.
    out_of_step: bool,
    /// The stretch the next step joins, numbered by the seq of its first step: the steps since the last note or the start of a turn.
    open_stretch: Option<u64>,
    /// The `prompt_id` of the agent's current turn; a message that arrives during the turn carries the same one.
    turn: Option<String>,
    /// The agent session the open stretch and the turn belong to.
    session: Option<String>,
    /// The last stretch of each of the newest turns, oldest first, for a subagent's step that arrives late.
    turn_stretches: VecDeque<(String, u64)>,
}

/// A page of entries, oldest first, and whether nothing older is kept.
pub struct Page {
    pub entries: Vec<ActivityEntry>,
    pub complete: bool,
}

pub fn line_of(entry: &ActivityEntry) -> Option<String> {
    serde_json::to_string(entry).ok()
}

fn is_step(entry: &ActivityEntry) -> bool {
    matches!(entry.kind, ActivityKind::Step { .. })
}

impl Log {
    /// The log a file's lines hold; a line that is not an entry is skipped and leaves the file to be rewritten.
    pub fn from_lines(lines: &[String]) -> Log {
        let mut log = Log::default();
        for line in lines {
            let Ok(entry) = serde_json::from_str::<ActivityEntry>(line) else {
                continue;
            };
            log.next_seq = log.next_seq.max(entry.seq + 1);
            log.keep(entry);
        }
        log.lines_in_file = lines.len();
        log.out_of_step = log.lines_in_file != log.entries.len();
        log
    }

    fn keep(&mut self, entry: ActivityEntry) {
        let adds_a_step = is_step(&entry);
        self.entries.push_back(entry);
        if adds_a_step {
            self.steps_kept += 1;
        }
        let written_kept = self.entries.len() - self.steps_kept;
        if self.steps_kept > KEPT_STEPS {
            self.drop_oldest_step();
        } else if written_kept > KEPT_ENTRIES {
            self.drop_oldest_written();
        }
    }

    fn drop_oldest_step(&mut self) {
        let mut entries = self.entries.iter();
        let Some(index) = entries.position(is_step) else {
            return;
        };
        self.entries.remove(index);
        self.steps_kept -= 1;
    }

    fn drop_oldest_written(&mut self) {
        let mut entries = self.entries.iter();
        let Some(index) = entries.position(|entry| !is_step(entry)) else {
            return;
        };
        self.entries.remove(index);
    }

    /// A step at the end, joining the open stretch or opening one.
    pub fn append_step(&mut self, step: Step, context: &StepContext, at_ms: u64) -> ActivityEntry {
        let stretch = self.stretch_for(context);
        let kind = ActivityKind::Step { stretch, step };
        self.append(Author::App, kind, at_ms)
    }

    /// A message the agent sent another session, which is one of its steps as well as a line of its own.
    pub fn append_message_out(
        &mut self,
        to: Peer,
        text: String,
        context: &StepContext,
        at_ms: u64,
    ) -> ActivityEntry {
        let stretch = self.stretch_for(context);
        let kind = ActivityKind::MessageOut { to, text, stretch };
        self.append(Author::Agent, kind, at_ms)
    }

    // A step joins the open stretch, or opens one. The exception is a subagent's step that arrives once its turn's
    // stretch is closed or another turn has begun: it joins its own turn's last stretch, so it folds under that note.
    fn stretch_for(&mut self, context: &StepContext) -> u64 {
        if context.subagent
            && let Some(turn) = &context.turn
        {
            let its_turn_is_open = self.open_stretch.is_some() && self.turn.as_ref() == Some(turn);
            if !its_turn_is_open && let Some(stretch) = self.last_stretch_of(turn) {
                return stretch;
            }
        }
        let stretch = match self.open_stretch {
            Some(open) => open,
            None => self.next_seq,
        };
        self.open_stretch = Some(stretch);
        let mut turn = context.turn.clone();
        if turn.is_none() {
            turn = self.turn.clone();
        }
        if let Some(turn) = turn {
            self.remember_stretch(turn, stretch);
        }
        stretch
    }

    fn last_stretch_of(&self, turn: &str) -> Option<u64> {
        let mut turns = self.turn_stretches.iter();
        let (_, stretch) = turns.find(|(kept, _)| kept == turn)?;
        Some(*stretch)
    }

    fn remember_stretch(&mut self, turn: String, stretch: u64) {
        self.turn_stretches.retain(|(kept, _)| *kept != turn);
        self.turn_stretches.push_back((turn, stretch));
        if self.turn_stretches.len() > TURNS_REMEMBERED {
            self.turn_stretches.pop_front();
        }
    }

    /// The agent's note, which takes the open stretch's steps and closes it.
    pub fn append_note(&mut self, text: String, at_ms: u64) -> ActivityEntry {
        let stretch = self.open_stretch.take();
        let kind = ActivityKind::Note { text, stretch };
        self.append(Author::Agent, kind, at_ms)
    }

    /// The user's prompt. One that starts a new turn closes the open stretch with no note, and one with no turn id always starts one; one within the turn leaves it open.
    pub fn append_prompt(
        &mut self,
        text: String,
        turn: Option<String>,
        at_ms: u64,
    ) -> ActivityEntry {
        self.start_or_join_turn(turn);
        let kind = ActivityKind::Asked { text };
        self.append(Author::User, kind, at_ms)
    }

    /// A message from another session, which reaches the agent as a prompt and starts or joins a turn the same way.
    pub fn append_message_in(
        &mut self,
        from: Peer,
        text: String,
        turn: Option<String>,
        at_ms: u64,
    ) -> ActivityEntry {
        self.start_or_join_turn(turn);
        let kind = ActivityKind::MessageIn { text };
        self.append(Author::OtherAgent(from), kind, at_ms)
    }

    // A prompt from a new turn, or with no turn id, closes the open stretch with no note.
    fn start_or_join_turn(&mut self, turn: Option<String>) {
        let same_turn = turn.is_some() && turn == self.turn;
        if !same_turn {
            self.open_stretch = None;
            self.turn = turn;
        }
    }

    /// An agent session starting in the terminal. A new session ends the open stretch with no note, and the turn with it;
    /// the same session starting again, as after a compaction, carries on with both.
    pub fn session_started(&mut self, session_id: &str) {
        let same_session = self.session.as_deref() == Some(session_id);
        if same_session {
            return;
        }
        self.session = Some(session_id.to_string());
        self.open_stretch = None;
        self.turn = None;
        self.turn_stretches.clear();
    }

    /// A new entry at the end, numbered after every earlier one.
    pub fn append(&mut self, author: Author, kind: ActivityKind, at_ms: u64) -> ActivityEntry {
        let entry = ActivityEntry {
            seq: self.next_seq,
            at_ms,
            author,
            kind,
        };
        self.next_seq += 1;
        self.keep(entry.clone());
        self.lines_in_file += 1;
        entry
    }

    /// Whether the file should be replaced by the kept entries rather than appended to.
    pub fn needs_rewrite(&self) -> bool {
        self.out_of_step || self.lines_in_file >= REWRITE_AT_LINES
    }

    /// The kept entries as the lines a rewritten file holds.
    pub fn lines(&self) -> Vec<String> {
        let lines = self.entries.iter().filter_map(line_of);
        lines.collect()
    }

    /// Records what happened to the file: rewritten to `lines()`, or a write that failed and left it behind.
    pub fn file_written(&mut self, rewritten: bool, succeeded: bool) {
        if !succeeded {
            self.out_of_step = true;
            return;
        }
        if rewritten {
            self.lines_in_file = self.entries.len();
            self.out_of_step = false;
        }
    }

    fn page_ending_at(&self, end: usize) -> Page {
        let start = end.saturating_sub(PAGE);
        let entries = self.entries.range(start..end).cloned();
        Page {
            entries: entries.collect(),
            complete: start == 0,
        }
    }

    pub fn newest(&self) -> Page {
        self.page_ending_at(self.entries.len())
    }

    /// The steps of the open stretch, oldest first: the work since the agent's last note in this turn.
    pub fn open_steps(&self) -> Vec<&Step> {
        let Some(open) = self.open_stretch else {
            return Vec::new();
        };
        let steps = self.entries.iter().filter_map(|entry| match &entry.kind {
            ActivityKind::Step { stretch, step } if *stretch == open => Some(step),
            _ => None,
        });
        steps.collect()
    }

    /// The notes of the session now running, newest first: those since a new or cleared start. A resume or a
    /// compaction carries the same conversation on, so it does not end them.
    pub fn session_notes_newest_first(&self) -> Vec<&str> {
        let mut notes = Vec::new();
        for entry in self.entries.iter().rev() {
            match &entry.kind {
                ActivityKind::Note { text, .. } => notes.push(text.as_str()),
                ActivityKind::SessionStarted {
                    source: StartSource::New | StartSource::Cleared,
                } => break,
                _ => {}
            }
        }
        notes
    }

    /// The page just before the entry numbered `before`.
    pub fn older_than(&self, before: u64) -> Page {
        let end = self.entries.partition_point(|entry| entry.seq < before);
        self.page_ending_at(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::stretch_line;

    fn asked(text: &str) -> ActivityKind {
        ActivityKind::Asked { text: text.into() }
    }

    fn ran(command: &str) -> Step {
        Step::Did {
            tool: "Bash".into(),
            summary: command.into(),
            failed: None,
        }
    }

    fn peer(name: &str) -> Peer {
        Peer {
            name: Some(name.into()),
            address: None,
        }
    }

    fn turn(id: &str) -> Option<String> {
        Some(id.to_string())
    }

    fn in_turn(id: &str, subagent: bool) -> StepContext {
        StepContext {
            turn: turn(id),
            subagent,
        }
    }

    fn stretches(log: &Log) -> Vec<String> {
        let entries = log.entries.iter().cloned();
        let lines = entries.map(stretch_line);
        lines.collect()
    }

    // A log as this build writes it, with every kind of entry and every author. Renaming a variant or a field of
    // what a line holds would drop a user's history without a word, so the file must read back whole and unchanged.
    const STORED_LOG: &[&str] = &[
        r#"{"seq":0,"at_ms":1,"author":"App","kind":{"SessionStarted":{"source":"New"}}}"#,
        r#"{"seq":1,"at_ms":2,"author":"User","kind":{"Asked":{"text":"Fix the logo"}}}"#,
        r##"{"seq":2,"at_ms":3,"author":{"OtherAgent":{"name":"code-83","address":"uds:/tmp/a.sock"}},"kind":{"MessageIn":{"text":"Look at #12"}}}"##,
        r#"{"seq":3,"at_ms":4,"author":"App","kind":{"Step":{"stretch":3,"step":{"Did":{"tool":"Bash","summary":"cargo test","failed":"Exit code 1"}}}}}"#,
        r#"{"seq":4,"at_ms":5,"author":"App","kind":{"Step":{"stretch":3,"step":{"Did":{"tool":"Read","summary":"/w/a.rs","failed":null}}}}}"#,
        r#"{"seq":5,"at_ms":6,"author":"App","kind":{"Step":{"stretch":3,"step":{"Changed":{"tool":"Edit","path":"/w/src/logo.rs"}}}}}"#,
        r#"{"seq":6,"at_ms":7,"author":"Agent","kind":{"MessageOut":{"to":{"name":null,"address":"uds:/tmp/a.sock"},"text":"On it","stretch":3}}}"#,
        r#"{"seq":7,"at_ms":8,"author":"Agent","kind":{"Shared":{"thing":0,"title":"Plan"}}}"#,
        r#"{"seq":8,"at_ms":9,"author":"App","kind":"AskedForNote"}"#,
        r#"{"seq":9,"at_ms":10,"author":"Agent","kind":{"Note":{"text":"Found it","stretch":3}}}"#,
        r#"{"seq":10,"at_ms":11,"author":"Agent","kind":{"Note":{"text":"Nothing more","stretch":null}}}"#,
        r#"{"seq":11,"at_ms":12,"author":"App","kind":{"SessionStarted":{"source":"Resumed"}}}"#,
        r#"{"seq":12,"at_ms":13,"author":"App","kind":{"SessionStarted":{"source":"Compacted"}}}"#,
        r#"{"seq":13,"at_ms":14,"author":"App","kind":{"SessionStarted":{"source":"Cleared"}}}"#,
    ];

    #[test]
    fn a_log_this_build_wrote_reads_back_whole_and_writes_the_same_lines() {
        let lines: Vec<String> = STORED_LOG.iter().map(|line| line.to_string()).collect();
        let log = Log::from_lines(&lines);
        assert!(!log.needs_rewrite(), "every line is read as an entry");
        assert_eq!(log.lines(), lines);
    }

    #[test]
    fn a_log_keeps_its_newest_entries_pages_back_from_them_and_reads_back_from_its_file() {
        let appended = REWRITE_AT_LINES + 5;
        let mut log = Log::default();
        for index in 0..appended {
            let entry = log.append(Author::User, asked(&index.to_string()), index as u64);
            assert_eq!(entry.seq, index as u64);
            let rewrite = log.needs_rewrite();
            log.file_written(rewrite, true);
        }

        let last = appended as u64 - 1;
        let first_kept = appended as u64 - KEPT_ENTRIES as u64;
        let newest = log.newest();
        let newest_last = newest.entries.last().unwrap().seq;
        assert_eq!(
            (newest.entries.len(), newest.complete, newest_last),
            (PAGE, false, last)
        );
        let mut before = newest.entries[0].seq;
        let mut pages = 1;
        loop {
            let older = log.older_than(before);
            pages += 1;
            before = older.entries[0].seq;
            if older.complete {
                break;
            }
        }
        assert_eq!((pages, before), (KEPT_ENTRIES / PAGE, first_kept));
        let one_past_a_page = log.older_than(first_kept + PAGE as u64 + 1);
        assert!(!one_past_a_page.complete);

        // a failed write leaves the file behind, so the next one replaces it whole
        log.file_written(false, false);
        assert!(log.needs_rewrite());

        let mut lines = log.lines();
        lines.push("not an entry".into());
        let mut read = Log::from_lines(&lines);
        assert!(read.needs_rewrite());
        let entry = read.append(Author::User, asked("next"), 0);
        assert_eq!(
            entry.seq, appended as u64,
            "numbering carries on after a relaunch"
        );
    }

    #[test]
    fn steps_and_written_entries_are_kept_up_to_their_own_counts() {
        let mut log = Log::default();
        for index in 0..KEPT_ENTRIES {
            log.append_note(index.to_string(), 0);
        }
        let step = || Step::Did {
            tool: "Bash".into(),
            summary: "ls".into(),
            failed: None,
        };
        for _ in 0..(KEPT_STEPS + 10) {
            log.append_step(step(), &StepContext::default(), 0);
        }
        let counts = |log: &Log| {
            let steps = log.entries.iter().filter(|entry| is_step(entry)).count();
            (log.entries.len() - steps, steps)
        };
        assert_eq!(
            counts(&log),
            (KEPT_ENTRIES, KEPT_STEPS),
            "a long run of steps leaves every note"
        );
        let oldest_step = log.entries.iter().find(|entry| is_step(entry)).unwrap();
        assert_eq!(oldest_step.seq, (KEPT_ENTRIES + 10) as u64);

        log.append_prompt("next".into(), None, 0);
        assert_eq!(counts(&log), (KEPT_ENTRIES, KEPT_STEPS));
        let ActivityKind::Note { text, .. } = &log.entries[0].kind else {
            panic!("the oldest entry kept is a note");
        };
        assert_eq!(text, "1", "the oldest note went first");
    }

    #[test]
    fn messages_are_lines_of_their_own_and_a_subagents_late_step_keeps_to_its_turn() {
        let mut log = Log::default();
        log.append_message_in(
            peer("code-83"),
            "Look into the logo.".into(),
            turn("turn-a"),
            0,
        );
        log.append_message_in(
            peer("code-f1"),
            "The route is up.".into(),
            turn("turn-a"),
            0,
        );
        log.append_step(ran("cargo test"), &in_turn("turn-a", false), 0);
        log.append_message_out(
            peer("code-83"),
            "On it".into(),
            &in_turn("turn-a", false),
            0,
        );
        log.append_note("Found it".into(), 0);
        // a background helper finishing after the note
        log.append_step(ran("grep logo"), &in_turn("turn-a", true), 0);
        log.append_prompt("Next".into(), turn("turn-b"), 0);
        log.append_step(ran("ls"), &in_turn("turn-b", false), 0);
        log.append_step(ran("grep again"), &in_turn("turn-a", true), 0);
        log.append_step(ran("pwd"), &in_turn("turn-b", true), 0);
        // only a subagent keeps to an older turn: the agent's own step joins what is open
        log.append_step(ran("date"), &in_turn("turn-a", false), 0);
        // a message that starts a new turn closes the stretch, as a prompt does
        log.append_message_in(peer("code-83"), "Merged.".into(), turn("turn-c"), 0);
        log.append_step(ran("git pull"), &in_turn("turn-c", false), 0);

        let expected = [
            "message from code-83",
            "message from code-f1",
            "step 2 in 2",
            "sent 3 in 2",
            "note in 2",
            "step 5 in 2",
            "asked",
            "step 7 in 7",
            "step 8 in 2",
            "step 9 in 7",
            "step 10 in 7",
            "message from code-83",
            "step 12 in 12",
        ];
        assert_eq!(stretches(&log), expected);
    }

    #[test]
    fn a_note_takes_the_steps_since_the_last_one_and_a_new_turn_or_session_leaves_them_unnoted() {
        let mut log = Log::default();
        let own = StepContext::default();
        log.session_started("s1");
        log.append_prompt("Find the bug".into(), turn("turn-a"), 0);
        log.append_step(ran("cargo test"), &own, 0);
        // a message from another agent mid-turn carries the turn's id and leaves the stretch open
        log.append_prompt("Also check the logo".into(), turn("turn-a"), 0);
        log.append_step(ran("cargo clippy"), &own, 0);
        // a compaction starts the same session again, mid-turn
        log.session_started("s1");
        log.append_step(ran("cargo build"), &own, 0);
        log.append_note("Found it".into(), 0);
        log.append_note("Nothing more to do".into(), 0);
        log.append_step(ran("git status"), &own, 0);
        log.append_prompt("Next".into(), turn("turn-b"), 0);
        log.append_step(ran("ls"), &own, 0);
        log.session_started("s2");
        log.append_step(ran("pwd"), &own, 0);
        // a new session starts its own turns, even one whose id repeats the last session's
        log.append_prompt("Again".into(), turn("turn-b"), 0);
        log.append_step(ran("date"), &own, 0);
        log.append_prompt("No turn id".into(), None, 0);
        log.append_step(ran("whoami"), &own, 0);
        log.append_prompt("Still no turn id".into(), None, 0);
        log.append_step(ran("uptime"), &own, 0);

        let expected = [
            "asked",
            "step 1 in 1",
            "asked",
            "step 3 in 1",
            "step 4 in 1",
            "note in 1",
            "note alone",
            "step 7 in 7",
            "asked",
            "step 9 in 9",
            "step 10 in 10",
            "asked",
            "step 12 in 12",
            "asked",
            "step 14 in 14",
            "asked",
            "step 16 in 16",
        ];
        assert_eq!(stretches(&log), expected);
    }
}
