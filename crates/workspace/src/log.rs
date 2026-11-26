use std::collections::VecDeque;

use taco_protocol::{ActivityEntry, ActivityKind};

/// How many entries a terminal's log keeps; older ones go as new ones come.
pub const KEPT_ENTRIES: usize = 1_000;
/// How many entries the window is sent at a time.
pub const PAGE: usize = 200;
// The file is rewritten down to the kept entries once it passes this many lines.
const REWRITE_AT_LINES: usize = 2 * KEPT_ENTRIES;

/// One terminal's Agent Activity, oldest first.
#[derive(Debug, Default)]
pub struct Log {
    entries: VecDeque<ActivityEntry>,
    next_seq: u64,
    lines_in_file: usize,
    /// The file holds lines the log does not keep, or lacks one a failed write lost.
    out_of_step: bool,
}

/// A page of entries, oldest first, and whether nothing older is kept.
pub struct Page {
    pub entries: Vec<ActivityEntry>,
    pub complete: bool,
}

pub fn line_of(entry: &ActivityEntry) -> Option<String> {
    serde_json::to_string(entry).ok()
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
        self.entries.push_back(entry);
        if self.entries.len() > KEPT_ENTRIES {
            self.entries.pop_front();
        }
    }

    /// A new entry at the end, numbered after every earlier one.
    pub fn append(&mut self, kind: ActivityKind, at_ms: u64) -> ActivityEntry {
        let entry = ActivityEntry {
            seq: self.next_seq,
            at_ms,
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
            let entry = log.append(asked(&index.to_string()), index as u64);
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
        let entry = read.append(asked("next"), 0);
        assert_eq!(
            entry.seq, appended as u64,
            "numbering carries on after a relaunch"
        );
    }
}
