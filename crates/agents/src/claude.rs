use serde_json::Value;
use taco_protocol::{ActivityKind, AgentSignal};

use crate::harness::{AgentTitle, Harness, HarnessSetup, HookEntry};

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
// An entry is a line in a narrow panel, kept for a thousand entries, so a prompt or a command is cut to this.
const MAX_ENTRY_CHARACTERS: usize = 1000;

const CHANGING_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];
const CHANGED_PATHS: &[&str] = &["/tool_input/file_path", "/tool_input/notebook_path"];
/// The input field that says, in one line, what each tool did.
const SUMMARY_FIELDS: &[(&str, &str)] = &[
    ("Bash", "/tool_input/command"),
    ("Read", "/tool_input/file_path"),
    ("Grep", "/tool_input/pattern"),
    ("Glob", "/tool_input/pattern"),
    ("WebFetch", "/tool_input/url"),
    ("WebSearch", "/tool_input/query"),
    ("Task", "/tool_input/description"),
];

const UUID_LENGTH: usize = 36;
const UUID_HYPHENS: [usize; 4] = [8, 13, 18, 23];

const ATTENTION_SCRIPT: &str = "taco-shells-attention.sh";
const SESSION_SCRIPT: &str = "taco-shells-claude-session.sh";

static SETUP: HarnessSetup = HarnessSetup {
    product_name: "Claude Code",
    commands: &["claude"],
    config_directory_variable: "CLAUDE_CONFIG_DIR",
    default_config_directory: ".claude",
    settings_file: "settings.json",
    entries: &[
        entry("Notification", ATTENTION_SCRIPT, false),
        entry("Stop", ATTENTION_SCRIPT, false),
        entry("SessionStart", SESSION_SCRIPT, false),
        entry("SessionEnd", SESSION_SCRIPT, false),
        entry("UserPromptSubmit", SESSION_SCRIPT, true),
        entry("PreToolUse", SESSION_SCRIPT, true),
        entry("PostToolUse", SESSION_SCRIPT, true),
        entry("Notification", SESSION_SCRIPT, true),
        entry("Stop", SESSION_SCRIPT, true),
        entry("StopFailure", SESSION_SCRIPT, true),
    ],
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
    entry_reader: Some(entry_of),
    setup: Some(&SETUP),
};

const fn entry(event: &'static str, script: &'static str, is_async: bool) -> HookEntry {
    HookEntry {
        event,
        script,
        is_async,
    }
}

/// Whether a value is a Claude Code session id: a UUID, in either case.
fn is_session_id(value: &str) -> bool {
    if value.len() != UUID_LENGTH {
        return false;
    }
    let mut bytes = value.bytes().enumerate();
    bytes.all(|(index, byte)| {
        if UUID_HYPHENS.contains(&index) {
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn signal_for(event: &str, detail: Option<&str>) -> Option<AgentSignal> {
    match event {
        "UserPromptSubmit" | "PostToolUse" => Some(AgentSignal::Working),
        "PreToolUse" => {
            let asks = detail.is_some_and(|tool| NEEDS_TOOLS.contains(&tool));
            if asks {
                Some(AgentSignal::Needs)
            } else {
                Some(AgentSignal::Working)
            }
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

fn cut_to(text: &str, limit: usize) -> String {
    let kept = text.trim().chars().take(limit);
    kept.collect()
}

fn text_at(parsed: &Value, pointer: &str) -> Option<String> {
    let text = parsed.pointer(pointer)?.as_str()?;
    let cut = cut_to(text, MAX_ENTRY_CHARACTERS);
    if cut.is_empty() {
        return None;
    }
    Some(cut)
}

// A summary is one line, so a multi-line command shows its first.
fn summary_of(tool: &str, parsed: &Value) -> String {
    let mut fields = SUMMARY_FIELDS.iter();
    let Some((_, pointer)) = fields.find(|(name, _)| *name == tool) else {
        return String::new();
    };
    let Some(text) = text_at(parsed, pointer) else {
        return String::new();
    };
    let mut lines = text.lines();
    let first = lines.next();
    match first {
        Some(line) => line.to_string(),
        None => String::new(),
    }
}

// What you asked, from a submitted prompt, and what the agent did or changed, from a finished tool call.
fn entry_of(event: &str, payload: &str) -> Option<ActivityKind> {
    let parsed: Value = serde_json::from_str(payload).ok()?;
    match event {
        "UserPromptSubmit" => {
            let text = text_at(&parsed, "/prompt")?;
            Some(ActivityKind::Asked { text })
        }
        // logged once a tool has run, so one the user refused never shows as done
        "PostToolUse" => {
            let tool = text_at(&parsed, "/tool_name")?;
            let mut paths = CHANGED_PATHS.iter();
            let path = paths.find_map(|pointer| text_at(&parsed, pointer));
            if let (true, Some(path)) = (CHANGING_TOOLS.contains(&tool.as_str()), path) {
                return Some(ActivityKind::Changed { tool, path });
            }
            let summary = summary_of(&tool, &parsed);
            Some(ActivityKind::Did { tool, summary })
        }
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION_ID: &str = "123e4567-e89b-12d3-a456-426614174000";
    const INJECTED: &str = "$(rm -rf /)";

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
            signal_for("UserPromptSubmit", None),
            Some(AgentSignal::Working)
        );
        assert_eq!(
            signal_for("PreToolUse", Some("Bash")),
            Some(AgentSignal::Working)
        );
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
        assert_eq!(question_of("PostToolUse", asked), None);
        assert_eq!(question_of("PreToolUse", "not json"), None);
        let cut = question_of("PreToolUse", &long).unwrap();
        assert_eq!(cut.chars().count(), MAX_QUESTION_CHARACTERS);
    }

    #[test]
    fn a_prompt_a_finished_tool_and_a_changed_file_each_make_an_entry_and_nothing_else_does() {
        let entry = |event: &str, payload: &str| entry_of(event, payload);
        let did = |tool: &str, summary: &str| ActivityKind::Did {
            tool: tool.into(),
            summary: summary.into(),
        };
        let changed = |tool: &str, path: &str| ActivityKind::Changed {
            tool: tool.into(),
            path: path.into(),
        };

        let prompt =
            r#"{"hook_event_name":"UserPromptSubmit","prompt":"  Fix the tacos\nplease  "}"#;
        let asked = ActivityKind::Asked {
            text: "Fix the tacos\nplease".into(),
        };
        assert_eq!(entry("UserPromptSubmit", prompt), Some(asked));
        assert_eq!(entry("UserPromptSubmit", r#"{"prompt":"  "}"#), None);

        let bash = r#"{"tool_name":"Bash","tool_input":{"command":"cargo test\n--locked"}}"#;
        assert_eq!(entry("PostToolUse", bash), Some(did("Bash", "cargo test")));
        assert_eq!(entry("PreToolUse", bash), None, "only a tool that ran");
        let edit =
            r#"{"tool_name":"Edit","tool_input":{"file_path":"/w/src/main.rs","old_string":"a"}}"#;
        assert_eq!(
            entry("PostToolUse", edit),
            Some(changed("Edit", "/w/src/main.rs"))
        );
        let notebook =
            r#"{"tool_name":"NotebookEdit","tool_input":{"notebook_path":"/w/a.ipynb"}}"#;
        assert_eq!(
            entry("PostToolUse", notebook),
            Some(changed("NotebookEdit", "/w/a.ipynb"))
        );
        let unknown = r#"{"tool_name":"mcp__linear__get_issue","tool_input":{"id":"X-1"}}"#;
        assert_eq!(
            entry("PostToolUse", unknown),
            Some(did("mcp__linear__get_issue", ""))
        );
        assert_eq!(entry("Stop", prompt), None);
        assert_eq!(entry("PostToolUse", "{"), None);

        let huge = format!(
            r#"{{"tool_name":"Write","tool_input":{{"file_path":"/w/big","content":"{}"}}}}"#,
            "x".repeat(100_000)
        );
        assert_eq!(
            entry("PostToolUse", &huge),
            Some(changed("Write", "/w/big"))
        );
        let long_command = format!(
            r#"{{"tool_name":"Bash","tool_input":{{"command":"{}"}}}}"#,
            "y".repeat(5_000)
        );
        let Some(ActivityKind::Did { summary, .. }) = entry("PostToolUse", &long_command) else {
            panic!("a command makes an entry");
        };
        assert_eq!(summary.chars().count(), MAX_ENTRY_CHARACTERS);
    }
}
