use std::collections::VecDeque;

use taco_protocol::{ActivityEntry, ActivityKind, Author, Step};

/// How many entries other than steps a terminal's log keeps; older ones go as new ones come.
pub const KEPT_ENTRIES: usize = 1_000;
/// How many steps it keeps, counted apart so a long run of steps never pushes the agent's notes out.
pub const KEPT_STEPS: usize = 2_000;
/// How many entries the window is sent at a time.
pub const PAGE: usize = 200;
/// The file is rewritten down to the kept entries once it passes this many lines, twice what is kept.
pub const REWRITE_AT_LINES: usize = 2 * (KEPT_ENTRIES + KEPT_STEPS);

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
    pub fn append_step(&mut self, step: Step, at_ms: u64) -> ActivityEntry {
        let stretch = match self.open_stretch {
            Some(open) => open,
            None => self.next_seq,
        };
        self.open_stretch = Some(stretch);
        let kind = ActivityKind::Step { stretch, step };
        self.append(Author::App, kind, at_ms)
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
        let same_turn = turn.is_some() && turn == self.turn;
        if !same_turn {
            self.open_stretch = None;
            self.turn = turn;
        }
        let kind = ActivityKind::Asked { text };
        self.append(Author::User, kind, at_ms)
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

    /// The page just before the entry numbered `before`.
    pub fn older_than(&self, before: u64) -> Page {
        let end = self.entries.partition_point(|entry| entry.seq < before);
        self.page_ending_at(end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(text: &str) -> ActivityKind {
        ActivityKind::Asked { text: text.into() }
    }

    #[test]
    fn a_log_keeps_its_newest_entries_pages_back_from_them_and_reads_back_from_its_file() {
        let appended = REWRITE_AT_LINES + 5;
        let mut log = Log::default();
        let mut rewritten_at = Vec::new();
        for index in 0..appended {
            let entry = log.append(Author::User, asked(&index.to_string()), index as u64);
            assert_eq!(entry.seq, index as u64);
            let rewrite = log.needs_rewrite();
            if rewrite {
                rewritten_at.push(index + 1);
            }
            log.file_written(rewrite, true);
        }
        assert_eq!(
            rewritten_at,
            vec![REWRITE_AT_LINES],
            "once the file holds twice the kept entries"
        );

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
        };
        for _ in 0..(KEPT_STEPS + 10) {
            log.append_step(step(), 0);
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
    fn a_file_an_earlier_build_wrote_reads_as_prompts_and_steps_of_their_own() {
        let lines = [
            r#"{"seq":3,"at_ms":10,"kind":{"Asked":{"text":"Fix the tacos"}}}"#,
            r#"{"seq":4,"at_ms":11,"kind":{"Did":{"tool":"Bash","summary":"cargo test"}}}"#,
            r#"{"seq":5,"at_ms":12,"kind":{"Changed":{"tool":"Edit","path":"src/lib.rs"}}}"#,
            r#"{"seq":6,"at_ms":13,"author":"Agent","kind":{"Note":{"text":"Done","stretch":4}}}"#,
            r#"{"seq":7,"at_ms":14,"kind":{"Danced":{}}}"#,
        ];
        let lines: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
        let mut read = Log::from_lines(&lines);

        let entries = read.newest().entries;
        let read_as: Vec<(u64, Author, ActivityKind)> = entries
            .into_iter()
            .map(|entry| (entry.seq, entry.author, entry.kind))
            .collect();
        let did = Step::Did {
            tool: "Bash".into(),
            summary: "cargo test".into(),
        };
        let changed = Step::Changed {
            tool: "Edit".into(),
            path: "src/lib.rs".into(),
        };
        let note = ActivityKind::Note {
            text: "Done".into(),
            stretch: Some(4),
        };
        let expected = vec![
            (3, Author::User, asked("Fix the tacos")),
            (
                4,
                Author::App,
                ActivityKind::Step {
                    stretch: 4,
                    step: did,
                },
            ),
            (
                5,
                Author::App,
                ActivityKind::Step {
                    stretch: 5,
                    step: changed,
                },
            ),
            (6, Author::Agent, note),
        ];
        assert_eq!(read_as, expected);
        assert!(read.needs_rewrite(), "the line that is no entry is dropped");
        let step = Step::Did {
            tool: "Read".into(),
            summary: "README.md".into(),
        };
        let next = read.append_step(step, 15);
        let ActivityKind::Step { stretch, .. } = next.kind else {
            panic!("a step is appended as a step");
        };
        assert_eq!(
            (next.seq, stretch),
            (7, 7),
            "a new step is a stretch of its own"
        );
    }
}
