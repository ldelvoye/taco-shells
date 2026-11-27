use taco_protocol::{AgentSignal, StartSource};

use crate::harness::{AgentTitle, Harness, HarnessSetup, HookFields};

mod entries;
mod found;
mod payload;
mod shared;
mod text;

pub use entries::{
    ACTIVITY_SERVER, ACTIVITY_TOOLS, NOTE_TOOL, NOW_TOOL, SHARE_KINDS, SHARE_TOOL,
    activity_tool_of, activity_tool_prefix,
};
pub use payload::reduced_payload;

const ASKING_TOOL: &str = "AskUserQuestion";
// ExitPlanMode needs you too, but asks no question in words: the window says a plan waits.
const NEEDS_TOOLS: &[&str] = &[ASKING_TOOL, "ExitPlanMode"];
const NEEDS_NOTIFICATIONS: &[&str] = &[
    "permission_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
    "agent_needs_input",
];

// Claude Code starts its terminal title with a status glyph; this one means idle, any other means working.
const IDLE_GLYPH: char = '✳';

// A line typed before the shell's prompt is up is cut at 1024 bytes, losing the
// closing quote. 200 characters stay under that at four bytes each.
const MAX_NAME_CHARACTERS: usize = 200;

// A question is pinned above the log, so a longer one is cut rather than carried whole in every published state.
const MAX_QUESTION_CHARACTERS: usize = 1000;

const UUID_LENGTH: usize = 36;
const UUID_HYPHENS: [usize; 4] = [8, 13, 18, 23];

static SETUP: HarnessSetup = HarnessSetup {
    product_name: "Claude Code",
    commands: &["claude"],
    config_directory_variable: "CLAUDE_CONFIG_DIR",
    default_config_directory: ".claude",
    settings_file: "settings.json",
    plugin_variable: "CLAUDE_CODE_PLUGIN_DIRS",
    version_flag: "--version",
    minimum_version: (2, 1, 280),
};

pub static CLAUDE: Harness = Harness {
    id: "claude",
    name: "Claude",
    session_id_check: is_session_id,
    signal_reader: signal_for,
    title_reader: Some(title_of),
    resume_line: resume_command,
    fork_line: Some(fork_command),
    question_reader: Some(question_of),
    entry_reader: Some(entries::read_entries),
    version_reader: Some(parse_version),
    hook_reader: Some(hook_fields_of),
    start_source_reader: Some(start_source_of),
    setup: Some(&SETUP),
};

/// Whether a value is a Claude Code session id: a UUID, in either case.
fn is_session_id(value: &str) -> bool {
    if value.len() != UUID_LENGTH {
        return false;
    }
    let mut bytes = value.bytes().enumerate();
    bytes.all(|(index, byte)| {
        if UUID_HYPHENS.contains(&index) {
            return byte == b'-';
        }
        byte.is_ascii_hexdigit()
    })
}

fn signal_for(event: &str, detail: Option<&str>) -> Option<AgentSignal> {
    match event {
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" => Some(AgentSignal::Working),
        "PreToolUse" => {
            let asks = detail.is_some_and(|tool| NEEDS_TOOLS.contains(&tool));
            if asks {
                return Some(AgentSignal::Needs);
            }
            Some(AgentSignal::Working)
        }
        "Notification" => {
            let asks = detail.is_some_and(|kind| NEEDS_NOTIFICATIONS.contains(&kind));
            if !asks {
                return None;
            }
            Some(AgentSignal::Needs)
        }
        "Stop" | "StopFailure" => Some(AgentSignal::Stopped),
        _ => None,
    }
}

// An emoji presentation selector or a combining mark can follow the glyph before its space.
fn is_glyph_modifier(character: char) -> bool {
    let variation_selector = ('\u{FE00}'..='\u{FE0F}').contains(&character);
    let combining_mark = ('\u{0300}'..='\u{036F}').contains(&character);
    let symbol_mark = ('\u{20D0}'..='\u{20FF}').contains(&character);
    variation_selector || combining_mark || symbol_mark
}

fn title_of(title: &str) -> AgentTitle {
    let untouched = AgentTitle {
        name: title.to_string(),
        idle: false,
    };

    let mut characters = title.chars();
    let Some(glyph) = characters.next() else {
        return untouched;
    };
    if glyph.is_alphanumeric() || glyph.is_whitespace() {
        return untouched;
    }

    let mut rest = characters.as_str();
    let mut after_glyph = rest.chars();
    let next = after_glyph.next();
    if next.is_some_and(is_glyph_modifier) {
        rest = after_glyph.as_str();
    }

    let Some(name) = rest.strip_prefix(' ') else {
        return untouched;
    };
    AgentTitle {
        name: name.to_string(),
        idle: glyph == IDLE_GLYPH,
    }
}

// The text of a permission prompt or of the first question the agent asks with AskUserQuestion.
fn question_of(event: &str, payload: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(payload).ok()?;
    let pointer = match event {
        "Notification" => {
            let kind = parsed.pointer("/notification_type")?.as_str()?;
            if !NEEDS_NOTIFICATIONS.contains(&kind) {
                return None;
            }
            "/message"
        }
        "PreToolUse" => {
            let tool = parsed.pointer("/tool_name")?.as_str()?;
            if tool != ASKING_TOOL {
                return None;
            }
            "/tool_input/questions/0/question"
        }
        _ => return None,
    };

    let text = parsed.pointer(pointer)?.as_str()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let kept = trimmed.chars().take(MAX_QUESTION_CHARACTERS);
    Some(kept.collect())
}

fn resume_command(session_id: &str) -> Option<String> {
    if !is_session_id(session_id) {
        return None;
    }
    Some(format!("claude --resume {session_id}\n"))
}

fn is_typeable(character: &char) -> bool {
    let line_or_paragraph_separator = *character == '\u{2028}' || *character == '\u{2029}';
    !character.is_control() && !line_or_paragraph_separator
}

fn fork_command(session_id: &str, name: &str) -> Option<String> {
    if !is_session_id(session_id) {
        return None;
    }
    let base = format!("claude --resume {session_id} --fork-session");

    let typeable = name.chars().filter(is_typeable);
    let kept: String = typeable.take(MAX_NAME_CHARACTERS).collect();
    if kept.is_empty() {
        return Some(format!("{base}\n"));
    }

    let escaped = kept.replace('\'', "'\\''");
    Some(format!("{base} --name '{escaped}'\n"))
}

fn parse_version(output: &str) -> Option<(u32, u32, u32)> {
    let first_line = output.lines().next()?;
    let first_word = first_line.split_whitespace().next()?;
    let mut numbers = first_word.split('.');
    let major_text = numbers.next()?;
    let minor_text = numbers.next()?;
    let patch_text = numbers.next()?;
    let major = major_text.parse().ok()?;
    let minor = minor_text.parse().ok()?;
    let patch_digits = leading_digits(patch_text);
    let patch = patch_digits.parse().ok()?;
    Some((major, minor, patch))
}

fn leading_digits(text: &str) -> &str {
    let digit_count = text.chars().take_while(char::is_ascii_digit).count();
    &text[..digit_count]
}

fn text_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    let value = object.get(key)?;
    value.as_str()
}

fn hook_fields_of(payload: &str) -> Option<HookFields> {
    let parsed: serde_json::Value = serde_json::from_str(payload).ok()?;
    let object = parsed.as_object()?;
    let session_id = text_field(object, "session_id")?;
    let event = text_field(object, "hook_event_name")?;
    let source = text_field(object, "source");
    let tool_name = text_field(object, "tool_name");
    Some(HookFields {
        session_id: session_id.to_string(),
        event: event.to_string(),
        source: source.map(str::to_string),
        tool_name: tool_name.map(str::to_string),
    })
}

/// How a session started, from Claude Code's hook source; a value it does not know counts as a new session.
fn start_source_of(source: &str) -> StartSource {
    match source {
        "resume" => StartSource::Resumed,
        "compact" => StartSource::Compacted,
        "clear" => StartSource::Cleared,
        _ => StartSource::New,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
    const INJECTED: &str = "$(rm -rf /)";

    #[test]
    fn reads_the_leading_version() {
        assert_eq!(parse_version("2.1.332 (Claude Code)"), Some((2, 1, 332)));
        assert_eq!(parse_version("10.0.1\n"), Some((10, 0, 1)));
        assert_eq!(parse_version("Claude Code"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("2.1.332-beta.1"), Some((2, 1, 332)));
    }

    #[test]
    fn each_claude_start_source_maps_to_its_kind_and_an_unknown_one_is_new() {
        let cases = [
            ("resume", StartSource::Resumed),
            ("compact", StartSource::Compacted),
            ("clear", StartSource::Cleared),
            ("fork", StartSource::New),
            ("", StartSource::New),
        ];
        for (source, expected) in cases {
            assert_eq!(CLAUDE.start_source(source), expected, "{source:?}");
        }
    }

    #[test]
    fn a_hook_payload_gives_its_session_event_source_and_tool() {
        let full = format!(
            r#"{{"session_id":"{SESSION_ID}","hook_event_name":"SessionStart","source":"resume","tool_name":"Bash"}}"#
        );
        let bare = format!(r#"{{"session_id":"{SESSION_ID}","hook_event_name":"Stop"}}"#);
        let no_session = r#"{"hook_event_name":"Stop"}"#;

        let all = CLAUDE.hook_fields(&full).unwrap();
        assert_eq!(all.session_id, SESSION_ID);
        assert_eq!(all.event, "SessionStart");
        assert_eq!(all.source.as_deref(), Some("resume"));
        assert_eq!(all.tool_name.as_deref(), Some("Bash"));

        let without = CLAUDE.hook_fields(&bare).unwrap();
        assert_eq!(without.source, None);
        assert_eq!(without.tool_name, None);

        assert_eq!(CLAUDE.hook_fields(no_session), None);
        assert_eq!(CLAUDE.hook_fields("[1]"), None);
    }

    #[test]
    fn a_session_id_is_a_uuid_in_either_case_and_nothing_else() {
        let upper = SESSION_ID.to_uppercase();
        let with_newline = format!("{SESSION_ID}\n");

        assert!(is_session_id(SESSION_ID));
        assert!(is_session_id(&upper));
        assert!(!is_session_id(&with_newline));
        assert!(!is_session_id("--dangerously-skip-permissions"));
        assert!(!is_session_id("123e4567-e89b-12d3-a456_426614174000"));
        assert!(!is_session_id("123e4567-e89b-12d3-a456-42661417400g"));
    }

    #[test]
    fn resume_and_fork_lines_quote_the_name_and_refuse_a_bad_id() {
        let resume = format!("claude --resume {SESSION_ID} --fork-session");

        assert_eq!(
            resume_command(SESSION_ID).as_deref(),
            Some("claude --resume 123e4567-e89b-12d3-a456-426614174000\n")
        );
        assert_eq!(resume_command(INJECTED), None);
        assert_eq!(fork_command(INJECTED, "x"), None);

        let named = fork_command(SESSION_ID, "⑂ Tacos fact");
        assert_eq!(named, Some(format!("{resume} --name '⑂ Tacos fact'\n")));

        let quoted = fork_command(SESSION_ID, "it's $(rm -rf /)");
        assert_eq!(
            quoted,
            Some(format!("{resume} --name 'it'\\''s $(rm -rf /)'\n"))
        );

        // a control character would reach the shell's line editor as a key
        let controls = fork_command(SESSION_ID, "a\u{1b}[2Jb\nc\u{7}\u{2028}");
        assert_eq!(controls, Some(format!("{resume} --name 'a[2Jbc'\n")));

        let longest = fork_command(SESSION_ID, &"😀'".repeat(1000)).unwrap();
        assert!(longest.len() < 1024);
        assert!(longest.ends_with("'\n"));

        assert_eq!(fork_command(SESSION_ID, ""), Some(format!("{resume}\n")));
    }

    #[test]
    fn hook_events_map_to_signals_ignoring_the_idle_reminder_and_subagents() {
        assert_eq!(
            signal_for("PreToolUse", Some("AskUserQuestion")),
            Some(AgentSignal::Needs)
        );
        assert_eq!(
            signal_for("Notification", Some("permission_prompt")),
            Some(AgentSignal::Needs)
        );
        assert_eq!(signal_for("Notification", Some("idle_prompt")), None);
        assert_eq!(signal_for("Notification", None), None);
        assert_eq!(signal_for("StopFailure", None), Some(AgentSignal::Stopped));
        assert_eq!(signal_for("SubagentStop", None), None);
        assert_eq!(
            signal_for("PostToolUseFailure", Some("Bash")),
            Some(AgentSignal::Working),
            "a tool that failed is the agent still at work"
        );
    }

    #[test]
    fn a_title_loses_its_leading_glyph_and_says_whether_it_was_idle() {
        let read = |title: &str| {
            let read = title_of(title);
            (read.name, read.idle)
        };

        assert_eq!(read("✳ INFRENG-813"), ("INFRENG-813".to_string(), true));
        assert_eq!(
            read("✳\u{FE0F} INFRENG-813"),
            ("INFRENG-813".to_string(), true)
        );
        assert_eq!(read("◑ INFRENG-823"), ("INFRENG-823".to_string(), false));
        assert_eq!(read("INFRENG-714"), ("INFRENG-714".to_string(), false));
        assert_eq!(read("✳INFRENG-714"), ("✳INFRENG-714".to_string(), false));
        assert_eq!(read("✳ "), (String::new(), true));
    }

    #[test]
    fn a_question_comes_from_a_permission_prompt_or_ask_user_question_and_nothing_else() {
        let permission = r#"{"hook_event_name":"Notification","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}"#;
        let idle =
            r#"{"notification_type":"idle_prompt","message":"Claude is waiting for your input"}"#;
        let asked = r#"{"tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"  Which branch?\n","options":[]}]}}"#;
        let blank =
            r#"{"tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":" \n "}]}}"#;
        let other_tool =
            r#"{"tool_name":"Task","tool_input":{"questions":[{"question":"Not for you"}]}}"#;
        let long = format!(
            r#"{{"tool_name":"AskUserQuestion","tool_input":{{"questions":[{{"question":"{}"}}]}}}}"#,
            "a".repeat(5000)
        );

        assert_eq!(
            question_of("Notification", permission).as_deref(),
            Some("Claude needs your permission to use Bash")
        );
        assert_eq!(question_of("Notification", idle), None);
        assert_eq!(
            question_of("PreToolUse", asked).as_deref(),
            Some("Which branch?")
        );
        assert_eq!(question_of("PreToolUse", blank), None);
        assert_eq!(question_of("PreToolUse", other_tool), None);
        let cut = question_of("PreToolUse", &long).unwrap();
        assert_eq!(cut.chars().count(), MAX_QUESTION_CHARACTERS);
    }
}
