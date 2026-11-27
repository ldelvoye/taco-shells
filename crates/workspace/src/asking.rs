//! The text the app hands an agent from what Agent Activity holds: its own notes after a compaction, and the ask for a
//! note before it stops.

use std::collections::HashSet;
use std::path::Path;

use taco_agents::CUT_MARK;
use taco_protocol::{StartSource, Step};

// How much of its own account a compacted agent is handed back, in characters, its last line included.
const MAX_CONTEXT_CHARACTERS: usize = 2_000;
// Room kept for the line saying how many older notes were left out.
const LEFT_OUT_ROOM: usize = 40;
// How long the ask for a note can be, in characters.
const MAX_NUDGE_CHARACTERS: usize = 500;
// How many changed files the ask names before it counts the rest.
const MAX_NUDGE_FILES: usize = 5;
// Real work since the last note: one changed file, or this many other steps.
const STEPS_WORTH_A_NOTE: usize = 3;

const CONTEXT_HEADING: &str = "Your earlier notes from this session, as you wrote them in Taco Shells' Agent Activity, newest first:";
const NOW_PREFIX: &str = "What you were doing: ";
const NUDGE_OPENING: &str = "Before you finish, write a short note for the user with the activity note tool. Since your last note you";

/// How a session started, from Claude Code's `source`; a value it does not know counts as a new session.
pub fn start_source(source: &str) -> StartSource {
    match source {
        "resume" => StartSource::Resumed,
        "compact" => StartSource::Compacted,
        "clear" => StartSource::Cleared,
        _ => StartSource::New,
    }
}

/// The now line and the newest notes that fit, for an agent whose context was just compacted; none when there is
/// nothing to hand back. Notes that do not fit are counted rather than cut.
pub fn context_text(now: Option<&str>, notes_newest_first: &[&str]) -> Option<String> {
    if now.is_none() && notes_newest_first.is_empty() {
        return None;
    }
    let mut text = CONTEXT_HEADING.to_string();
    if let Some(now) = now {
        text.push_str(&format!("\n{NOW_PREFIX}{now}"));
    }
    let budget = MAX_CONTEXT_CHARACTERS - LEFT_OUT_ROOM;
    let mut left_out = 0;
    for (index, note) in notes_newest_first.iter().enumerate() {
        let line = format!("\n- {note}");
        let room = budget.saturating_sub(text.chars().count());
        let fits = line.chars().count() <= room;
        // the newest note is never left out whole: what fits of it is handed back, cut with the mark
        if index == 0 && !fits {
            let kept_length = room.saturating_sub(CUT_MARK.chars().count());
            let kept: String = line.chars().take(kept_length).collect();
            text.push_str(&format!("{kept}{CUT_MARK}"));
            continue;
        }
        // once one note is left out every older one is too, so the notes handed back run unbroken from the newest
        if left_out == 0 && fits {
            text.push_str(&line);
        } else {
            left_out += 1;
        }
    }
    match left_out {
        0 => {}
        1 => text.push_str("\n(1 older note left out.)"),
        _ => text.push_str(&format!("\n({left_out} older notes left out.)")),
    }
    Some(text)
}

/// The ask for a note, when the steps since the last one hold real work; none otherwise. Changed files are named
/// relative to the session's directory.
pub fn nudge_text(steps: &[&Step], directory: Option<&str>) -> Option<String> {
    let mut changed: Vec<String> = Vec::new();
    let mut named = HashSet::new();
    let mut others = 0;
    for step in steps {
        match step {
            Step::Changed { path, .. } => {
                let shown = relative_to(path, directory);
                if named.insert(shown.clone()) {
                    changed.push(shown);
                }
            }
            Step::Did { .. } => others += 1,
        }
    }
    if changed.is_empty() && others < STEPS_WORTH_A_NOTE {
        return None;
    }
    let mut what_was_done = format!("took {others} steps that no note describes.");
    if !changed.is_empty() {
        let files = files_named(&changed);
        what_was_done = format!("changed {files}, and no note says what or why.");
    }
    let text = format!("{NUDGE_OPENING} {what_was_done}");
    Some(bounded(&text, MAX_NUDGE_CHARACTERS))
}

fn relative_to(path: &str, directory: Option<&str>) -> String {
    let Some(directory) = directory else {
        return path.to_string();
    };
    let relative = Path::new(path).strip_prefix(directory);
    let Ok(relative) = relative else {
        return path.to_string();
    };
    let Some(shown) = relative.to_str() else {
        return path.to_string();
    };
    shown.to_string()
}

// "a.rs, b.rs, c.rs, d.rs, e.rs and 2 more" for seven files.
fn files_named(changed: &[String]) -> String {
    let shown: Vec<&str> = changed
        .iter()
        .take(MAX_NUDGE_FILES)
        .map(String::as_str)
        .collect();
    let listed = shown.join(", ");
    let more = changed.len() - shown.len();
    if more == 0 {
        return listed;
    }
    format!("{listed} and {more} more")
}

fn bounded(text: &str, limit: usize) -> String {
    let length = text.chars().count();
    if length <= limit {
        return text.to_string();
    }
    let kept_length = limit - CUT_MARK.chars().count();
    let kept: String = text.chars().take(kept_length).collect();
    format!("{kept}{CUT_MARK}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed(path: &str) -> Step {
        Step::Changed {
            tool: "Edit".into(),
            path: path.into(),
        }
    }

    fn ran() -> Step {
        Step::Did {
            tool: "Bash".into(),
            summary: "ls".into(),
            failed: None,
        }
    }

    #[test]
    fn a_compacted_agent_gets_its_now_line_and_the_newest_notes_that_fit() {
        assert_eq!(context_text(None, &[]), None, "nothing to hand back");
        let text = context_text(Some("Fixing the logo"), &["Found the cause", "Started"]).unwrap();
        assert!(text.contains("What you were doing: Fixing the logo"));
        let newest = text.find("Found the cause").unwrap();
        let oldest = text.find("Started").unwrap();
        assert!(newest < oldest, "newest first");

        let long = "n".repeat(900);
        let many = [long.as_str(), long.as_str(), long.as_str(), "short"];
        let text = context_text(None, &many).unwrap();
        assert!(
            text.chars().count() <= MAX_CONTEXT_CHARACTERS,
            "its last line included"
        );
        assert!(text.ends_with("(2 older notes left out.)"), "{text}");
        let one_over = context_text(None, &[long.as_str(), long.as_str(), long.as_str()]).unwrap();
        assert!(one_over.ends_with("(1 older note left out.)"), "{one_over}");

        let huge = "h".repeat(8_000);
        let first_too_long = context_text(None, &[huge.as_str(), "older"]).unwrap();
        assert!(
            first_too_long.contains(&format!("hhh{CUT_MARK}")),
            "the newest note comes back cut, not left out"
        );
        assert!(
            first_too_long.ends_with("(1 older note left out.)"),
            "{first_too_long}"
        );
        assert!(first_too_long.chars().count() <= MAX_CONTEXT_CHARACTERS);
        assert!(
            !text.contains("short"),
            "an older note never jumps a newer one that did not fit"
        );
    }

    #[test]
    fn the_ask_for_a_note_comes_with_real_work_and_names_five_files_at_most() {
        assert_eq!(
            nudge_text(&[&ran(), &ran()], None),
            None,
            "two commands are not worth a note"
        );
        let three = nudge_text(&[&ran(), &ran(), &ran()], None).unwrap();
        assert!(three.contains("took 3 steps"));

        let one = changed("/work/src/main.rs");
        let again = changed("/work/src/main.rs");
        let outside = changed("/etc/hosts");
        let text = nudge_text(&[&one, &again, &outside], Some("/work")).unwrap();
        assert!(text.contains("changed src/main.rs, /etc/hosts,"), "{text}");

        let paths: Vec<Step> = (0..7)
            .map(|index| changed(&format!("/work/{index}.rs")))
            .collect();
        let steps: Vec<&Step> = paths.iter().collect();
        let text = nudge_text(&steps, Some("/work")).unwrap();
        assert!(
            text.contains("0.rs, 1.rs, 2.rs, 3.rs, 4.rs and 2 more"),
            "{text}"
        );

        let long_name = changed(&format!("/work/{}.rs", "x".repeat(1_000)));
        let text = nudge_text(&[&long_name], Some("/work")).unwrap();
        assert_eq!(text.chars().count(), MAX_NUDGE_CHARACTERS);
        assert!(text.ends_with(CUT_MARK));
    }

    #[test]
    fn a_start_source_claude_code_has_no_name_for_is_a_new_session() {
        assert_eq!(start_source("fork"), StartSource::New);
        assert_eq!(start_source(""), StartSource::New);
    }
}
