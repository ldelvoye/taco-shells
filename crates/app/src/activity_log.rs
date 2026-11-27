//! The reading of a terminal's agent log: the entries the window holds, and the lines they make. Nothing here draws,
//! so the same lines can be shown as a list today and some other way later.

use std::collections::{BTreeMap, HashSet};

use taco_protocol::{
    ActivityEntry, ActivityEvent, ActivityKind, Author, Board, Peer, StartSource, Step, TerminalId,
};

const PREVIEW_CHARACTERS: usize = 360;
const PREVIEW_LINES: usize = 6;
const CUT_MARK: char = '…';

/// The log the window keeps for the terminal it watches.
#[derive(Clone, Debug, Default)]
pub struct ActivityLog {
    pub terminal: Option<TerminalId>,
    /// Oldest first, as the workspace sends them.
    pub entries: Vec<ActivityEntry>,
    /// Whether nothing older than the entries held is left to ask for.
    pub complete: bool,
    pub board: Board,
}

/// One line of the log as the panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum LogItem {
    /// The agent's own account, with the steps of the stretch it names.
    Note {
        seq: u64,
        at_ms: u64,
        text: String,
        steps: Option<Stretch>,
    },
    /// Work that ended with no note from the agent, at its last step.
    Unnoted {
        at_ms: u64,
        steps: Stretch,
    },
    /// The stretch still being worked on.
    Working {
        at_ms: u64,
        steps: Stretch,
    },
    Asked {
        seq: u64,
        at_ms: u64,
        text: String,
    },
    MessageIn {
        seq: u64,
        at_ms: u64,
        from: Peer,
        text: String,
    },
    MessageOut {
        seq: u64,
        at_ms: u64,
        to: Peer,
        text: String,
    },
    Shared {
        seq: u64,
        at_ms: u64,
        by: Author,
        thing: u64,
        title: String,
    },
    AskedForNote {
        seq: u64,
        at_ms: u64,
    },
    SessionStarted {
        seq: u64,
        at_ms: u64,
        source: StartSource,
    },
}

/// The steps of one stretch, or of neighbouring stretches with nothing between them. A stretch is numbered by the
/// `seq` of its first step, so a number older than the oldest entry held means its start is not loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct Stretch {
    /// Names the fold: the newest stretch number in it, so loading older steps never moves it.
    pub number: u64,
    /// Oldest first.
    pub steps: Vec<StretchStep>,
    /// Whether the stretch began before the oldest entry held, with older entries left to ask for.
    pub earlier_not_loaded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StretchStep {
    Step(Step),
    SentMessage(Peer),
}

#[derive(Default)]
struct Members {
    steps: Vec<StretchStep>,
    last_seq: u64,
}

impl ActivityLog {
    /// Applies one event; one for another terminal is ignored.
    pub fn apply(&mut self, event: ActivityEvent) {
        match event {
            ActivityEvent::Loaded {
                terminal,
                entries,
                complete,
                board,
            } => {
                if !self.watches(&terminal) {
                    return;
                }
                self.entries = entries;
                self.complete = complete;
                self.board = board;
            }
            ActivityEvent::Older {
                terminal,
                entries,
                complete,
            } => {
                if !self.watches(&terminal) {
                    return;
                }
                // A page asked for twice, or answered after a newer watch, holds entries already here.
                let held_oldest = self.oldest();
                let mut merged = entries;
                merged.retain(|entry| is_before(entry.seq, held_oldest));
                merged.append(&mut self.entries);
                self.entries = merged;
                self.complete = complete;
            }
            ActivityEvent::Appended { terminal, entry } => {
                if !self.watches(&terminal) {
                    return;
                }
                let newest = self.entries.last();
                if let Some(newest) = newest
                    && entry.seq <= newest.seq
                {
                    return;
                }
                self.entries.push(entry);
            }
            ActivityEvent::BoardChanged { terminal, board } => {
                if !self.watches(&terminal) {
                    return;
                }
                self.board = board;
            }
            ActivityEvent::Body { .. } => {}
        }
    }

    /// The `seq` of the oldest entry held, for asking for older ones.
    pub fn oldest(&self) -> Option<u64> {
        let first = self.entries.first()?;
        Some(first.seq)
    }

    fn watches(&self, terminal: &TerminalId) -> bool {
        self.terminal.as_ref() == Some(terminal)
    }
}

/// The log's lines, newest first. While `agent_working`, the newest stretch with no note stays open unless a prompt
/// or session start came after it.
pub fn log_items(log: &ActivityLog, agent_working: bool) -> Vec<LogItem> {
    let stretches = stretch_members(&log.entries);
    let noted = noted_stretches(&log.entries);
    let turn_start = newest_turn_start(&log.entries);
    let mut open = None;
    if agent_working && let Some((newest, members)) = stretches.iter().next_back() {
        let ended_by_a_turn = turn_start.is_some_and(|start| start > members.last_seq);
        if !ended_by_a_turn {
            open = Some(*newest);
        }
    }
    let mut items = Vec::new();
    for entry in &log.entries {
        let seq = entry.seq;
        let at_ms = entry.at_ms;
        match &entry.kind {
            ActivityKind::Note { text, stretch } => {
                let steps = stretch.map(|number| stretch_of(number, &stretches, log));
                let text = text.clone();
                items.push(LogItem::Note {
                    seq,
                    at_ms,
                    text,
                    steps,
                });
            }
            ActivityKind::Step { stretch, .. } => {
                let ends = ends_unnoted(*stretch, seq, &stretches, &noted);
                if ends {
                    let group = unnoted_group(*stretch, at_ms, open, &stretches, log);
                    items.push(group);
                }
            }
            ActivityKind::MessageOut { to, text, stretch } => {
                let ends = ends_unnoted(*stretch, seq, &stretches, &noted);
                if ends {
                    let group = unnoted_group(*stretch, at_ms, open, &stretches, log);
                    items.push(group);
                }
                items.push(LogItem::MessageOut {
                    seq,
                    at_ms,
                    to: to.clone(),
                    text: text.clone(),
                });
            }
            ActivityKind::Asked { text } => {
                let text = text.clone();
                items.push(LogItem::Asked { seq, at_ms, text });
            }
            ActivityKind::MessageIn { text } => {
                let from = peer_of(&entry.author);
                let text = text.clone();
                items.push(LogItem::MessageIn {
                    seq,
                    at_ms,
                    from,
                    text,
                });
            }
            ActivityKind::Shared { thing, title } => {
                let by = entry.author.clone();
                let title = title.clone();
                let thing = *thing;
                items.push(LogItem::Shared {
                    seq,
                    at_ms,
                    by,
                    thing,
                    title,
                });
            }
            ActivityKind::AskedForNote => items.push(LogItem::AskedForNote { seq, at_ms }),
            ActivityKind::SessionStarted { source } => {
                let source = *source;
                items.push(LogItem::SessionStarted { seq, at_ms, source });
            }
        }
    }
    let mut merged = merge_neighbouring_unnoted(items);
    merged.reverse();
    merged
}

/// The start of a long text, cut at a word and marked, or none when the text is short enough to show whole.
pub fn preview(whole: &str) -> Option<String> {
    // Leading blank lines would otherwise leave a preview of nothing but the mark.
    let text = whole.trim_start();
    let by_characters = text.char_indices().nth(PREVIEW_CHARACTERS);
    let character_cut = by_characters.map(|(at, _)| at);
    let mut newlines = text.match_indices('\n');
    let line_cut = newlines.nth(PREVIEW_LINES - 1).map(|(at, _)| at);
    let cuts = [character_cut, line_cut];
    let found = cuts.into_iter().flatten();
    let cut = found.min()?;
    let rest = &text[cut..];
    if rest.trim().is_empty() {
        return None;
    }
    let kept = &text[..cut];
    let inside_a_word = !rest.starts_with(char::is_whitespace);
    let mut shown = kept;
    if inside_a_word && let Some(space) = kept.rfind(char::is_whitespace) {
        shown = &kept[..space];
    }
    let mut cut_text = shown.trim_end().to_string();
    cut_text.push(CUT_MARK);
    Some(cut_text)
}

fn is_before(seq: u64, held_oldest: Option<u64>) -> bool {
    match held_oldest {
        Some(oldest) => seq < oldest,
        None => true,
    }
}

fn stretch_members(entries: &[ActivityEntry]) -> BTreeMap<u64, Members> {
    let mut stretches: BTreeMap<u64, Members> = BTreeMap::new();
    for entry in entries {
        let (number, step) = match &entry.kind {
            ActivityKind::Step { stretch, step } => (*stretch, StretchStep::Step(step.clone())),
            ActivityKind::MessageOut { to, stretch, .. } => {
                (*stretch, StretchStep::SentMessage(to.clone()))
            }
            _ => continue,
        };
        let members = stretches.entry(number).or_default();
        members.steps.push(step);
        members.last_seq = entry.seq;
    }
    stretches
}

fn newest_turn_start(entries: &[ActivityEntry]) -> Option<u64> {
    let mut newest = None;
    for entry in entries {
        let starts_a_turn = matches!(
            entry.kind,
            ActivityKind::Asked { .. } | ActivityKind::SessionStarted { .. }
        );
        if starts_a_turn {
            newest = Some(entry.seq);
        }
    }
    newest
}

fn noted_stretches(entries: &[ActivityEntry]) -> HashSet<u64> {
    let mut noted = HashSet::new();
    for entry in entries {
        if let ActivityKind::Note {
            stretch: Some(number),
            ..
        } = entry.kind
        {
            noted.insert(number);
        }
    }
    noted
}

fn ends_unnoted(
    number: u64,
    seq: u64,
    stretches: &BTreeMap<u64, Members>,
    noted: &HashSet<u64>,
) -> bool {
    if noted.contains(&number) {
        return false;
    }
    let Some(members) = stretches.get(&number) else {
        return false;
    };
    members.last_seq == seq
}

fn stretch_of(number: u64, stretches: &BTreeMap<u64, Members>, log: &ActivityLog) -> Stretch {
    let mut steps = Vec::new();
    if let Some(members) = stretches.get(&number) {
        steps = members.steps.clone();
    }
    let began_before_held = log.oldest().is_some_and(|oldest| number < oldest);
    Stretch {
        number,
        steps,
        earlier_not_loaded: began_before_held && !log.complete,
    }
}

fn unnoted_group(
    number: u64,
    at_ms: u64,
    open: Option<u64>,
    stretches: &BTreeMap<u64, Members>,
    log: &ActivityLog,
) -> LogItem {
    let steps = stretch_of(number, stretches, log);
    if open == Some(number) {
        return LogItem::Working { at_ms, steps };
    }
    LogItem::Unnoted { at_ms, steps }
}

fn peer_of(author: &Author) -> Peer {
    match author {
        Author::OtherAgent(peer) => peer.clone(),
        _ => Peer {
            name: None,
            address: None,
        },
    }
}

// An earlier build's log makes every step a stretch of its own; read side by side, they are one group.
fn merge_neighbouring_unnoted(items: Vec<LogItem>) -> Vec<LogItem> {
    let mut merged: Vec<LogItem> = Vec::new();
    for item in items {
        let previous = merged.last_mut();
        if let Some(LogItem::Unnoted {
            at_ms: earlier_at,
            steps: earlier,
        }) = previous
            && let LogItem::Unnoted { at_ms, steps } = &item
        {
            earlier.steps.extend(steps.steps.iter().cloned());
            earlier.number = steps.number;
            *earlier_at = *at_ms;
            continue;
        }
        merged.push(item);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes entries as the workspace would, keeping one open stretch.
    #[derive(Default)]
    struct Writer {
        entries: Vec<ActivityEntry>,
        open: Option<u64>,
    }

    impl Writer {
        fn push(&mut self, author: Author, kind: ActivityKind) {
            let seq = self.next_seq();
            self.entries.push(ActivityEntry {
                seq,
                at_ms: seq * 1000,
                author,
                kind,
            });
        }

        fn next_seq(&self) -> u64 {
            let held = self.entries.len() as u64;
            held + 1
        }

        fn stretch(&mut self) -> u64 {
            let next = self.next_seq();
            *self.open.get_or_insert(next)
        }

        fn asked(&mut self) -> &mut Self {
            self.open = None;
            let text = "a prompt".to_string();
            self.push(Author::User, ActivityKind::Asked { text });
            self
        }

        fn ran(&mut self) -> &mut Self {
            let stretch = self.stretch();
            let summary = self.next_seq().to_string();
            let tool = "Bash".to_string();
            let step = Step::Did {
                tool,
                summary,
                failed: None,
            };
            self.push(Author::App, ActivityKind::Step { stretch, step });
            self
        }

        fn old_step(&mut self) -> &mut Self {
            let stretch = self.next_seq();
            let summary = stretch.to_string();
            let tool = "Bash".to_string();
            let step = Step::Did {
                tool,
                summary,
                failed: None,
            };
            self.push(Author::App, ActivityKind::Step { stretch, step });
            self
        }

        fn sent(&mut self) -> &mut Self {
            let stretch = self.stretch();
            let to = named("code-83");
            let text = "a message out".to_string();
            self.push(
                Author::Agent,
                ActivityKind::MessageOut { to, text, stretch },
            );
            self
        }

        fn heard(&mut self) -> &mut Self {
            let peer = named("code-83");
            let text = "a message in".to_string();
            self.push(Author::OtherAgent(peer), ActivityKind::MessageIn { text });
            self
        }

        fn noted(&mut self) -> &mut Self {
            let stretch = self.open.take();
            let text = "a note".to_string();
            self.push(Author::Agent, ActivityKind::Note { text, stretch });
            self
        }

        fn shared(&mut self) -> &mut Self {
            let title = "a plan".to_string();
            self.push(Author::Agent, ActivityKind::Shared { thing: 1, title });
            self
        }

        fn log(&self) -> ActivityLog {
            ActivityLog {
                entries: self.entries.clone(),
                complete: true,
                ..ActivityLog::default()
            }
        }
    }

    fn named(name: &str) -> Peer {
        Peer {
            name: Some(name.to_string()),
            address: None,
        }
    }

    // Each line as a short word, a stretch as its steps' seqs, so a whole log reads on one line.
    fn outline(items: &[LogItem]) -> Vec<String> {
        let mut lines = Vec::new();
        for item in items {
            let line = match item {
                LogItem::Note { seq, steps, .. } => {
                    format!("note {seq} {}", stretch_outline(steps.as_ref()))
                }
                LogItem::Unnoted { steps, .. } => {
                    format!("unnoted {}", stretch_outline(Some(steps)))
                }
                LogItem::Working { steps, .. } => {
                    format!("working {}", stretch_outline(Some(steps)))
                }
                LogItem::Asked { seq, .. } => format!("asked {seq}"),
                LogItem::MessageIn { seq, .. } => format!("in {seq}"),
                LogItem::MessageOut { seq, .. } => format!("out {seq}"),
                LogItem::Shared { seq, .. } => format!("shared {seq}"),
                LogItem::AskedForNote { seq, .. } => format!("asked for note {seq}"),
                LogItem::SessionStarted { seq, .. } => format!("session {seq}"),
            };
            lines.push(line);
        }
        lines
    }

    fn stretch_outline(stretch: Option<&Stretch>) -> String {
        let Some(stretch) = stretch else {
            return "-".to_string();
        };
        let mut steps = Vec::new();
        for step in &stretch.steps {
            let shown = match step {
                StretchStep::Step(Step::Did { summary, .. }) => summary.clone(),
                StretchStep::Step(Step::Changed { path, .. }) => path.clone(),
                StretchStep::SentMessage(_) => "sent".to_string(),
            };
            steps.push(shown);
        }
        let mut shown = format!("#{} [{}]", stretch.number, steps.join(","));
        if stretch.earlier_not_loaded {
            shown.push('+');
        }
        shown
    }

    #[test]
    fn steps_fold_under_their_note_and_work_with_no_note_is_one_group_at_its_last_step() {
        let mut writer = Writer::default();
        writer.asked().ran().heard().ran().sent().shared().noted();
        writer.noted();
        writer.asked().ran().ran();
        writer.asked().ran().ran();
        let log = writer.log();

        let working = outline(&log_items(&log, true));
        let expected = [
            "working #13 [13,14]",
            "asked 12",
            "unnoted #10 [10,11]",
            "asked 9",
            "note 8 -",
            "note 7 #2 [2,4,sent]",
            "shared 6",
            "out 5",
            "in 3",
            "asked 1",
        ];
        assert_eq!(working, expected);

        let stopped = outline(&log_items(&log, false));
        assert_eq!(stopped[0], "unnoted #13 [13,14]");

        writer.asked();
        let prompted_again = writer.log();
        let before_its_first_step = outline(&log_items(&prompted_again, true));
        assert_eq!(
            before_its_first_step[..2],
            ["asked 15", "unnoted #13 [13,14]"]
        );

        writer.ran().noted();
        let noted_while_working = outline(&log_items(&writer.log(), true));
        assert_eq!(noted_while_working[0], "note 17 #16 [16]");
    }

    #[test]
    fn the_log_takes_only_entries_it_does_not_hold_for_the_terminal_it_watches() {
        let mut writer = Writer::default();
        writer.ran().ran().ran().ran().ran().ran().ran();
        let entries = writer.entries;
        let watched = TerminalId("watched".to_string());
        let other = TerminalId("other".to_string());
        let mut log = ActivityLog {
            terminal: Some(watched.clone()),
            ..ActivityLog::default()
        };
        log.apply(ActivityEvent::Loaded {
            terminal: watched.clone(),
            entries: entries[4..6].to_vec(),
            complete: false,
            board: Board::default(),
        });
        let older = ActivityEvent::Older {
            terminal: watched.clone(),
            entries: entries[2..4].to_vec(),
            complete: true,
        };
        log.apply(older.clone());
        log.apply(older);
        log.apply(ActivityEvent::Appended {
            terminal: watched,
            entry: entries[5].clone(),
        });
        log.apply(ActivityEvent::Appended {
            terminal: other.clone(),
            entry: entries[6].clone(),
        });
        log.apply(ActivityEvent::Loaded {
            terminal: other,
            entries: entries[..2].to_vec(),
            complete: true,
            board: Board::default(),
        });

        let held: Vec<u64> = log.entries.iter().map(|entry| entry.seq).collect();
        assert_eq!(held, [3, 4, 5, 6]);
        assert!(log.complete);
    }

    #[test]
    fn neighbouring_groups_from_an_earlier_build_read_as_one() {
        let mut writer = Writer::default();
        writer.asked().old_step().old_step().old_step();
        writer.asked().old_step();
        let log = writer.log();

        let items = outline(&log_items(&log, false));
        let expected = ["unnoted #6 [6]", "asked 5", "unnoted #4 [2,3,4]", "asked 1"];
        assert_eq!(items, expected);
    }

    #[test]
    fn a_stretch_cut_by_paging_says_so_and_joins_when_the_older_page_arrives() {
        let mut writer = Writer::default();
        writer.asked().ran().ran().ran().ran().noted();
        let terminal = TerminalId("t".to_string());
        let mut log = ActivityLog {
            terminal: Some(terminal.clone()),
            ..ActivityLog::default()
        };
        let newer = writer.entries[3..].to_vec();
        let older = writer.entries[..3].to_vec();
        log.apply(ActivityEvent::Loaded {
            terminal: terminal.clone(),
            entries: newer.clone(),
            complete: true,
            board: Board::default(),
        });
        let trimmed_for_good = outline(&log_items(&log, false));
        assert_eq!(trimmed_for_good, ["note 6 #2 [4,5]"]);

        log.apply(ActivityEvent::Loaded {
            terminal: terminal.clone(),
            entries: newer,
            complete: false,
            board: Board::default(),
        });
        assert_eq!(outline(&log_items(&log, false)), ["note 6 #2 [4,5]+"]);

        log.apply(ActivityEvent::Older {
            terminal,
            entries: older,
            complete: true,
        });
        let joined = outline(&log_items(&log, false));
        assert_eq!(joined, ["note 6 #2 [2,3,4,5]", "asked 1"]);
    }

    #[test]
    fn a_long_text_is_cut_at_a_word_or_after_six_lines() {
        assert_eq!(preview("short"), None);

        let words = "word ".repeat(100);
        let cut = preview(&words).unwrap();
        assert!(cut.ends_with("word…"), "{cut}");
        assert!(cut.chars().count() <= PREVIEW_CHARACTERS + 1);

        let lines = "line\n".repeat(8);
        assert_eq!(preview(&lines).unwrap(), "line\n".repeat(5) + "line…");

        let six_and_a_trailing_break = "line\n".repeat(6);
        assert_eq!(preview(&six_and_a_trailing_break), None);

        let blank_lines_first = "\n".repeat(7) + &words;
        assert_eq!(preview(&blank_lines_first), Some(cut));
    }
}
