use std::collections::VecDeque;

use taco_protocol::{ActivityEntry, ActivityKind};

/// How many entries a terminal's log keeps; older ones go as new ones come.
pub const KEPT_ENTRIES: usize = 1_000;
/// How many entries the window is sent at a time.
pub const PAGE: usize = 200;
// The file is appended to and rewritten to the kept entries once it holds twice as many lines, so it stays small.
const REWRITE_AT_LINES: usize = 2 * KEPT_ENTRIES;

/// One terminal's Agent Activity, oldest first.
#[derive(Debug, Default)]
pub struct Log {
    entries: VecDeque<ActivityEntry>,
    next_seq: u64,
    lines_in_file: usize,
}

/// A page of entries, oldest first, and whether nothing older is kept.
pub struct Page {
    pub entries: Vec<ActivityEntry>,
    pub complete: bool,
}

pub fn line_of(entry: &ActivityEntry) -> Option<String> {
    serde_json::to_string(entry).ok()
}

fn page_of(kept: &[ActivityEntry]) -> Page {
    let start = kept.len().saturating_sub(PAGE);
    let entries = kept[start..].to_vec();
    Page {
        entries,
        complete: start == 0,
    }
}

impl Log {
    /// The log a file's lines hold, skipping any line that is not an entry, and whether the file needs rewriting.
    pub fn from_lines(lines: &[String]) -> (Log, bool) {
        let mut log = Log::default();
        for line in lines {
            let Ok(entry) = serde_json::from_str::<ActivityEntry>(line) else {
                continue;
            };
            log.next_seq = log.next_seq.max(entry.seq + 1);
            log.keep(entry);
        }
        log.lines_in_file = lines.len();
        let needs_rewrite = log.lines_in_file != log.entries.len();
        (log, needs_rewrite)
    }

    fn keep(&mut self, entry: ActivityEntry) {
        self.entries.push_back(entry);
        if self.entries.len() > KEPT_ENTRIES {
            self.entries.pop_front();
        }
    }

    /// A new entry at the end, numbered after every earlier one, and whether the file should now be rewritten.
    pub fn append(&mut self, kind: ActivityKind, at_ms: u64) -> (ActivityEntry, bool) {
        let entry = ActivityEntry {
            seq: self.next_seq,
            at_ms,
            kind,
        };
        self.next_seq += 1;
        self.keep(entry.clone());
        self.lines_in_file += 1;
        let needs_rewrite = self.lines_in_file >= REWRITE_AT_LINES;
        (entry, needs_rewrite)
    }

    /// The kept entries as file lines, which the file now holds.
    pub fn rewritten(&mut self) -> Vec<String> {
        self.lines_in_file = self.entries.len();
        let lines = self.entries.iter().filter_map(line_of);
        lines.collect()
    }

    pub fn newest(&self) -> Page {
        let kept: Vec<ActivityEntry> = self.entries.iter().cloned().collect();
        page_of(&kept)
    }

    /// The page just before the entry numbered `before`.
    pub fn older_than(&self, before: u64) -> Page {
        let earlier = self.entries.iter().filter(|entry| entry.seq < before);
        let kept: Vec<ActivityEntry> = earlier.cloned().collect();
        page_of(&kept)
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
        let mut log = Log::default();
        let mut rewrites = 0;
        for index in 0..(REWRITE_AT_LINES + 5) {
            let (entry, needs_rewrite) = log.append(asked(&index.to_string()), index as u64);
            assert_eq!(entry.seq, index as u64);
            if needs_rewrite {
                rewrites += 1;
                let lines = log.rewritten();
                assert_eq!(lines.len(), KEPT_ENTRIES);
            }
        }
        assert_eq!(
            rewrites, 1,
            "rewritten once the file holds twice the kept entries"
        );

        let newest = log.newest();
        let last = newest.entries.last().unwrap().seq;
        assert_eq!(
            (newest.entries.len(), newest.complete, last),
            (PAGE, false, 2_004)
        );
        let first_kept = 2_005 - KEPT_ENTRIES as u64;
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

        let mut lines = log.rewritten();
        lines.push("not an entry".into());
        let (mut read, needs_rewrite) = Log::from_lines(&lines);
        assert!(needs_rewrite);
        let (entry, _) = read.append(asked("next"), 0);
        assert_eq!(entry.seq, 2_005, "numbering carries on after a relaunch");
    }
}
