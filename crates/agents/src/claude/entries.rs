use std::path::Path;

use serde_json::Value;
use taco_protocol::{Peer, PrState, SharedTarget, Step};

use super::found::found_by_app;
use super::shared::{
    ARTIFACT_KIND, DOCUMENT_KIND, IMAGE_KIND, LINK_PREFIX, MAX_BODY_CHARACTERS,
    MAX_TARGET_CHARACTERS, MAX_TITLE_CHARACTERS, NOTE_KIND, PULL_REQUEST_KIND, pull_request_of,
};
use super::text::{text_at, whole_text_at, written, written_at};
use crate::harness::{EntryRead, StepContext};

// A prompt or a note keeps this many characters, then CUT_MARK.
const MAX_WRITTEN_CHARACTERS: usize = 8_000;
// The agent's line on what it is doing now is shown above the log, so it stays short.
const MAX_NOW_CHARACTERS: usize = 300;
// A failed step shows the first line of its error, this long at most.
const MAX_FAILURE_CHARACTERS: usize = 200;
const FAILED_WITHOUT_ERROR: &str = "Failed";
// How a failed command's error starts, before its own message on the next line.
const EXIT_CODE_PREFIX: &str = "Exit code ";
// The share tool's kinds, as the Shared list labels them; any other kind is labelled as a note.
const SHARE_KINDS: &[(&str, &str)] = &[
    ("document", DOCUMENT_KIND),
    ("image", IMAGE_KIND),
    ("pull_request", PULL_REQUEST_KIND),
    ("artifact", ARTIFACT_KIND),
    ("other", NOTE_KIND),
];
// Another session's message, as Claude Code hands it to the agent in a prompt.
const MESSAGE_OPEN: &str = "<cross-session-message";
const MESSAGE_CLOSE: &str = "</cross-session-message>";
// The tools that send another session a message; Claude Code has used both names.
const MESSAGE_TOOLS: &[&str] = &["SendMessage", "CrossSessionMessage"];
// How a session's address starts, as opposed to a name.
const SESSION_ADDRESS_PREFIX: &str = "uds:";
// The tool-name prefixes of the app's activity server, for the stable app and for Dev.
const ACTIVITY_SERVERS: &[&str] = &[
    "mcp__plugin_taco-shells_activity__",
    "mcp__plugin_taco-shells-dev_activity__",
];

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
    ("Agent", "/tool_input/description"),
    ("Skill", "/tool_input/skill"),
    ("ToolSearch", "/tool_input/query"),
];

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

// "note" from "mcp__plugin_taco-shells_activity__note"; none for any other tool.
fn activity_tool(tool: &str) -> Option<&str> {
    let mut servers = ACTIVITY_SERVERS.iter();
    servers.find_map(|server| tool.strip_prefix(server))
}

// A call the server refused never gets here: Claude Code sends it as PostToolUseFailure.
fn activity_entry(name: &str, parsed: &Value) -> Option<EntryRead> {
    match name {
        "note" => {
            let text = written_at(parsed, "/tool_input/text", MAX_WRITTEN_CHARACTERS)?;
            Some(EntryRead::Note { text })
        }
        "now" => {
            let text = written_at(parsed, "/tool_input/text", MAX_NOW_CHARACTERS)?;
            Some(EntryRead::Now { text })
        }
        "share" => share_entry(parsed),
        _ => None,
    }
}

fn share_entry(parsed: &Value) -> Option<EntryRead> {
    let title = written_at(parsed, "/tool_input/title", MAX_TITLE_CHARACTERS)?;
    let named = text_at(parsed, "/tool_input/kind");
    let mut kinds = SHARE_KINDS.iter();
    let label = kinds.find(|(name, _)| Some(*name) == named.as_deref());
    let kind = match label {
        Some((_, label)) => label.to_string(),
        None => NOTE_KIND.to_string(),
    };
    let mut target = None;
    if let Some(given) = whole_text_at(parsed, "/tool_input/target", MAX_TARGET_CHARACTERS) {
        let cwd = text_at(parsed, "/cwd");
        target = target_of(&given, cwd.as_deref());
    }
    let body = written_at(parsed, "/tool_input/body", MAX_BODY_CHARACTERS);
    if target.is_none() && body.is_none() {
        return None;
    }
    Some(EntryRead::Share {
        kind,
        title,
        target,
        body,
    })
}

// What a share's target opens: an https link, a pull request on GitHub, or a file, its path made absolute against
// the agent's working directory. Anything else, such as another scheme or a home-relative path, opens nothing.
fn target_of(target: &str, cwd: Option<&str>) -> Option<SharedTarget> {
    if target.starts_with(LINK_PREFIX) {
        if let Some((url, number)) = pull_request_of(target) {
            // open until something checks it, which checked_ms records
            return Some(SharedTarget::PullRequest {
                url: Some(url),
                number,
                state: PrState::Open,
                checked_ms: None,
            });
        }
        let url = target.to_string();
        return Some(SharedTarget::Link { url });
    }
    let has_a_scheme = target.contains("://");
    if has_a_scheme || target.starts_with('~') {
        return None;
    }
    let path = Path::new(target);
    if path.is_absolute() {
        let path = target.to_string();
        return Some(SharedTarget::File { path });
    }
    let directory = Path::new(cwd?);
    if !directory.is_absolute() {
        return None;
    }
    let joined = directory.join(path);
    let absolute = joined.to_str()?;
    Some(SharedTarget::File {
        path: absolute.to_string(),
    })
}

// The value of `name` among a tag's attributes: `code-83` for `from-name` in ` from="…" from-name="code-83"`.
fn attribute(attributes: &str, name: &str) -> Option<String> {
    let opening = format!(" {name}=\"");
    let found = attributes.find(&opening)?;
    let start = found + opening.len();
    let rest = &attributes[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn asked(text: &str, turn: &Option<String>) -> Option<EntryRead> {
    let text = written(text, MAX_WRITTEN_CHARACTERS)?;
    let turn = turn.clone();
    Some(EntryRead::Asked { text, turn })
}

// A prompt as its parts, in order: each message another session sent, and what the user typed around them.
// A tag that never closes is left as the user's own text.
fn prompt_entries(prompt: &str, turn: Option<String>) -> Vec<EntryRead> {
    let mut entries = Vec::new();
    let mut rest = prompt;
    while let Some(start) = rest.find(MESSAGE_OPEN) {
        let before = &rest[..start];
        let tag = &rest[start + MESSAGE_OPEN.len()..];
        let Some(attributes_end) = tag.find('>') else {
            break;
        };
        let attributes = &tag[..attributes_end];
        let body = &tag[attributes_end + 1..];
        let Some(body_end) = body.find(MESSAGE_CLOSE) else {
            break;
        };
        entries.extend(asked(before, &turn));
        let from = Peer {
            name: attribute(attributes, "from-name"),
            address: attribute(attributes, "from"),
        };
        if let Some(text) = written(&body[..body_end], MAX_WRITTEN_CHARACTERS) {
            let turn = turn.clone();
            entries.push(EntryRead::MessageIn { from, text, turn });
        }
        rest = &body[body_end + MESSAGE_CLOSE.len()..];
    }
    entries.extend(asked(rest, &turn));
    entries
}

fn step_context(parsed: &Value) -> StepContext {
    let turn = text_at(parsed, "/prompt_id");
    let agent = text_at(parsed, "/agent_id");
    StepContext {
        turn,
        subagent: agent.is_some(),
    }
}

// A message the agent sent another session, once it went; none for one that did not, which stays a plain step.
fn message_out(parsed: &Value, context: StepContext) -> Option<EntryRead> {
    let sent = parsed.pointer("/tool_response/success");
    if sent != Some(&Value::Bool(true)) {
        return None;
    }
    let to = text_at(parsed, "/tool_input/to")?;
    let text = written_at(parsed, "/tool_input/message", MAX_WRITTEN_CHARACTERS)?;
    let is_address = to.starts_with(SESSION_ADDRESS_PREFIX);
    let mut peer = Peer {
        name: None,
        address: None,
    };
    if is_address {
        peer.address = Some(to);
    } else {
        peer.name = Some(to);
    }
    Some(EntryRead::MessageOut {
        to: peer,
        text,
        context,
    })
}

fn tool_entry(parsed: &Value) -> Option<EntryRead> {
    let tool = text_at(parsed, "/tool_name")?;
    // the agent's own activity calls are what it wrote, never steps
    if let Some(name) = activity_tool(&tool) {
        return activity_entry(name, parsed);
    }
    let context = step_context(parsed);
    if MESSAGE_TOOLS.contains(&tool.as_str())
        && let Some(sent) = message_out(parsed, context.clone())
    {
        return Some(sent);
    }
    let mut paths = CHANGED_PATHS.iter();
    let path = paths.find_map(|pointer| text_at(parsed, pointer));
    let changes_files = CHANGING_TOOLS.contains(&tool.as_str());
    if changes_files && let Some(path) = path {
        let step = Step::Changed { tool, path };
        return Some(EntryRead::Step { step, context });
    }
    let summary = summary_of(&tool, parsed);
    let step = Step::Did {
        tool,
        summary,
        failed: None,
    };
    Some(EntryRead::Step { step, context })
}

// The error's first line, and the next one too when the first only says the exit code: "Exit code 1: ls: no such file".
fn first_error_line(parsed: &Value) -> Option<String> {
    let error = text_at(parsed, "/error")?;
    let mut lines = error.lines();
    let first = lines.next()?;
    let mut shown = first.to_string();
    if is_exit_code(first)
        && let Some(next) = lines.next()
        && !next.trim().is_empty()
    {
        shown = format!("{first}: {}", next.trim());
    }
    written(&shown, MAX_FAILURE_CHARACTERS)
}

fn is_exit_code(line: &str) -> bool {
    let Some(code) = line.trim().strip_prefix(EXIT_CODE_PREFIX) else {
        return false;
    };
    !code.is_empty() && code.chars().all(|character| character.is_ascii_digit())
}

// A tool that failed, as a step that says so; an interrupted one is not logged.
fn failed_entry(parsed: &Value) -> Option<EntryRead> {
    let interrupted = parsed.pointer("/is_interrupt");
    if interrupted == Some(&Value::Bool(true)) {
        return None;
    }
    let tool = text_at(parsed, "/tool_name")?;
    if activity_tool(&tool).is_some() {
        return None;
    }
    let reason = match first_error_line(parsed) {
        Some(line) => line,
        None => FAILED_WITHOUT_ERROR.to_string(),
    };
    let summary = summary_of(&tool, parsed);
    let step = Step::Did {
        tool,
        summary,
        failed: Some(reason),
    };
    let context = step_context(parsed);
    Some(EntryRead::Step { step, context })
}

pub(super) fn read_entries(event: &str, payload: &str) -> Vec<EntryRead> {
    let Ok(parsed) = serde_json::from_str::<Value>(payload) else {
        return Vec::new();
    };
    match event {
        "UserPromptSubmit" => {
            let Some(value) = parsed.pointer("/prompt") else {
                return Vec::new();
            };
            let Some(prompt) = value.as_str() else {
                return Vec::new();
            };
            let turn = text_at(&parsed, "/prompt_id");
            prompt_entries(prompt, turn)
        }
        // logged once a tool has run, so one the user refused never shows as done
        "PostToolUse" => {
            let entry = tool_entry(&parsed);
            let found = found_by_app(&parsed);
            let mut entries: Vec<EntryRead> = entry.into_iter().collect();
            entries.extend(found.map(EntryRead::Found));
            entries
        }
        "PostToolUseFailure" => {
            let entry = failed_entry(&parsed);
            entry.into_iter().collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::text::{CUT_MARK, MAX_ENTRY_CHARACTERS};
    use super::*;

    #[test]
    fn a_prompt_a_finished_tool_and_a_changed_file_each_make_an_entry_and_nothing_else_does() {
        let entry = one_entry;
        let did = |tool: &str, summary: &str| {
            let step = Step::Did {
                tool: tool.into(),
                summary: summary.into(),
                failed: None,
            };
            let context = StepContext::default();
            EntryRead::Step { step, context }
        };
        let changed = |tool: &str, path: &str| {
            let step = Step::Changed {
                tool: tool.into(),
                path: path.into(),
            };
            let context = StepContext::default();
            EntryRead::Step { step, context }
        };

        let prompt =
            r#"{"hook_event_name":"UserPromptSubmit","prompt":"  Fix the tacos\nplease  "}"#;
        let asked = EntryRead::Asked {
            text: "Fix the tacos\nplease".into(),
            turn: None,
        };
        assert_eq!(entry("UserPromptSubmit", prompt), Some(asked));
        assert_eq!(entry("UserPromptSubmit", r#"{"prompt":"  "}"#), None);
        let in_a_turn = r#"{"prompt":"Also this","prompt_id":"3ff9bab6"}"#;
        let Some(EntryRead::Asked { turn, .. }) = entry("UserPromptSubmit", in_a_turn) else {
            panic!("a prompt makes an entry");
        };
        assert_eq!(turn.as_deref(), Some("3ff9bab6"));
        let prompt_of = |length: usize| format!(r#"{{"prompt":"{}"}}"#, "p".repeat(length));
        let at_the_limit = prompt_of(MAX_WRITTEN_CHARACTERS);
        let Some(EntryRead::Asked { text, .. }) = entry("UserPromptSubmit", &at_the_limit) else {
            panic!("a prompt makes an entry");
        };
        assert!(!text.ends_with(CUT_MARK), "a prompt that fits is whole");
        let over = prompt_of(MAX_WRITTEN_CHARACTERS + 1);
        let Some(EntryRead::Asked { text, .. }) = entry("UserPromptSubmit", &over) else {
            panic!("a long prompt makes an entry");
        };
        assert!(text.ends_with(CUT_MARK), "a cut prompt says so");
        let kept = text.chars().count() - CUT_MARK.chars().count();
        assert_eq!(kept, MAX_WRITTEN_CHARACTERS);

        // the app's own activity tools are what the agent wrote, never steps
        let note = r#"{"tool_name":"mcp__plugin_taco-shells_activity__note","tool_input":{"text":" Found it "},"tool_response":{"content":[]}}"#;
        let written = EntryRead::Note {
            text: "Found it".into(),
        };
        assert_eq!(entry("PostToolUse", note), Some(written));
        let now = r#"{"tool_name":"mcp__plugin_taco-shells-dev_activity__now","tool_input":{"text":"Reading the PTY code"}}"#;
        let line = EntryRead::Now {
            text: "Reading the PTY code".into(),
        };
        assert_eq!(entry("PostToolUse", now), Some(line));
        let long_now = format!(
            r#"{{"tool_name":"mcp__plugin_taco-shells_activity__now","tool_input":{{"text":"{}"}}}}"#,
            "n".repeat(MAX_NOW_CHARACTERS + 1)
        );
        let Some(EntryRead::Now { text }) = entry("PostToolUse", &long_now) else {
            panic!("a long now line is read");
        };
        assert!(text.ends_with(CUT_MARK), "the now line is kept short");
        let someone_elses =
            r#"{"tool_name":"mcp__plugin_other_activity__note","tool_input":{"text":"x"}}"#;
        assert_eq!(
            entry("PostToolUse", someone_elses),
            Some(did("mcp__plugin_other_activity__note", ""))
        );

        let bash = r#"{"tool_name":"Bash","tool_input":{"command":"cargo test\n--locked"}}"#;
        assert_eq!(entry("PostToolUse", bash), Some(did("Bash", "cargo test")));
        assert_eq!(entry("PreToolUse", bash), None, "only a tool that ran");
        let edit =
            r#"{"tool_name":"Edit","tool_input":{"file_path":"/w/src/main.rs","old_string":"a"}}"#;
        assert_eq!(
            entry("PostToolUse", edit),
            Some(changed("Edit", "/w/src/main.rs"))
        );
        // reading a file names it, but changes nothing
        let read = r#"{"tool_name":"Read","tool_input":{"file_path":"/w/a.rs"}}"#;
        assert_eq!(entry("PostToolUse", read), Some(did("Read", "/w/a.rs")));
        assert_eq!(entry("Stop", prompt), None);

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
        let Some(EntryRead::Step {
            step: Step::Did { summary, .. },
            ..
        }) = entry("PostToolUse", &long_command)
        else {
            panic!("a command makes an entry");
        };
        assert_eq!(summary.chars().count(), MAX_ENTRY_CHARACTERS);
    }

    #[test]
    fn a_prompt_splits_into_messages_and_the_users_words_and_a_sent_message_is_a_step() {
        let peer = |name: &str, address: &str| Peer {
            name: Some(name.into()),
            address: Some(address.into()),
        };
        let prompt = serde_json::json!({
            "prompt": "Before <cross-session-message from=\"uds:/tmp/cc-socks/9873.sock\" from-name=\"code-83\" from-mode=\"bypass\">\nReview #155.\n</cross-session-message><cross-session-message from=\"uds:/tmp/cc-socks/9879.sock\" from-name=\"code-f1\">\nRoute is up.\n</cross-session-message>\nThanks",
            "prompt_id": "turn-a",
        });
        let turn = Some("turn-a".to_string());
        let expected = vec![
            EntryRead::Asked {
                text: "Before".into(),
                turn: turn.clone(),
            },
            EntryRead::MessageIn {
                from: peer("code-83", "uds:/tmp/cc-socks/9873.sock"),
                text: "Review #155.".into(),
                turn: turn.clone(),
            },
            EntryRead::MessageIn {
                from: peer("code-f1", "uds:/tmp/cc-socks/9879.sock"),
                text: "Route is up.".into(),
                turn: turn.clone(),
            },
            EntryRead::Asked {
                text: "Thanks".into(),
                turn,
            },
        ];
        assert_eq!(
            read_entries("UserPromptSubmit", &prompt.to_string()),
            expected
        );
        let unclosed = r#"{"prompt":"<cross-session-message from=\"x\">never closed"}"#;
        let Some(EntryRead::Asked { text, .. }) = one_entry("UserPromptSubmit", unclosed) else {
            panic!("an unclosed tag is the user's own text");
        };
        assert!(text.starts_with("<cross-session-message"));

        let sent = |to: &str, success: bool| {
            serde_json::json!({
                "tool_name": "SendMessage",
                "tool_input": { "to": to, "summary": "s", "message": "ack" },
                "tool_response": { "success": success },
                "prompt_id": "turn-b",
                "agent_id": "helper-1",
            })
            .to_string()
        };
        let context = StepContext {
            turn: Some("turn-b".into()),
            subagent: true,
        };
        for main_agent in [serde_json::json!(""), serde_json::Value::Null] {
            let payload = serde_json::json!({ "tool_name": "Bash", "agent_id": main_agent });
            assert!(
                !step_context(&payload).subagent,
                "an empty or null agent_id is the main agent"
            );
        }
        let to_name = EntryRead::MessageOut {
            to: Peer {
                name: Some("code-78".into()),
                address: None,
            },
            text: "ack".into(),
            context: context.clone(),
        };
        assert_eq!(
            read_entries("PostToolUse", &sent("code-78", true)),
            vec![to_name]
        );
        let Some(EntryRead::MessageOut { to, .. }) =
            one_entry("PostToolUse", &sent("uds:/tmp/cc-socks/1.sock", true))
        else {
            panic!("a message to an address is sent");
        };
        assert_eq!(to.address.as_deref(), Some("uds:/tmp/cc-socks/1.sock"));
        let by_its_older_name = sent("code-78", true).replace("SendMessage", "CrossSessionMessage");
        let Some(EntryRead::MessageOut { .. }) = one_entry("PostToolUse", &by_its_older_name)
        else {
            panic!("the tool's older name sends a message too");
        };
        let Some(EntryRead::Step { step, .. }) = one_entry("PostToolUse", &sent("x", false)) else {
            panic!("a message that was not sent is a plain step");
        };
        assert!(matches!(step, Step::Did { .. }));
    }

    #[test]
    fn a_failed_tool_is_a_failed_step_and_an_interrupted_one_is_not_logged() {
        let failure = |error: &str, interrupted: bool| {
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "cargo test" },
                "error": error,
                "is_interrupt": interrupted,
            })
            .to_string()
        };
        let failed_with = |error: &str| {
            let Some(EntryRead::Step {
                step: Step::Did { failed, .. },
                ..
            }) = one_entry("PostToolUseFailure", &failure(error, false))
            else {
                panic!("a failure is a step");
            };
            failed
        };
        assert_eq!(
            failed_with("Exit code 1\nls: no-such-file: No such file or directory\nmore")
                .as_deref(),
            Some("Exit code 1: ls: no-such-file: No such file or directory"),
            "an exit code alone takes the line that says what went wrong"
        );
        assert_eq!(
            failed_with("Permission denied\nsecond line").as_deref(),
            Some("Permission denied")
        );
        assert_eq!(failed_with("Exit code 2").as_deref(), Some("Exit code 2"));
        assert_eq!(
            failed_with("Exit code 1\n\nlater").as_deref(),
            Some("Exit code 1"),
            "a blank next line adds nothing"
        );
        assert_eq!(
            failed_with("Exit code unknown\nmore").as_deref(),
            Some("Exit code unknown"),
            "only a numbered exit code takes the next line"
        );
        let long = failed_with(&"e".repeat(MAX_FAILURE_CHARACTERS + 1)).unwrap();
        assert!(long.ends_with(CUT_MARK), "a long error line is cut");
        assert_eq!(failed_with("  ").as_deref(), Some(FAILED_WITHOUT_ERROR));
        assert!(read_entries("PostToolUseFailure", &failure("Interrupted", true)).is_empty());
        let refused =
            r#"{"tool_name":"mcp__plugin_taco-shells_activity__note","error":"note needs a text"}"#;
        assert!(read_entries("PostToolUseFailure", refused).is_empty());
    }

    #[test]
    fn a_share_opens_a_link_a_pull_request_an_absolute_file_or_its_text() {
        let share = |input: serde_json::Value| {
            let payload = serde_json::json!({
                "tool_name": "mcp__plugin_taco-shells_activity__share",
                "tool_input": input,
                "cwd": "/work/tacos",
            });
            one_entry("PostToolUse", &payload.to_string())
        };
        let target_of_share = |kind: &str, target: &str| {
            let input = serde_json::json!({ "kind": kind, "title": "T", "target": target });
            let Some(EntryRead::Share { target, .. }) = share(input) else {
                return None;
            };
            target
        };
        let file = |path: &str| SharedTarget::File { path: path.into() };

        assert_eq!(
            target_of_share("document", "docs/plan.md"),
            Some(file("/work/tacos/docs/plan.md")),
            "a relative path is made absolute against the agent's directory"
        );
        assert_eq!(
            target_of_share("image", "/tmp/logo.png"),
            Some(file("/tmp/logo.png"))
        );
        let pull_request = SharedTarget::PullRequest {
            url: Some("https://github.com/ldelvoye/taco-shells/pull/155".into()),
            number: 155,
            state: PrState::Open,
            checked_ms: None,
        };
        assert_eq!(
            target_of_share(
                "pull_request",
                "https://github.com/ldelvoye/taco-shells/pull/155#discussion"
            ),
            Some(pull_request)
        );
        let link = SharedTarget::Link {
            url: "https://claude.ai/artifact/abc".into(),
        };
        assert_eq!(
            target_of_share("artifact", "https://claude.ai/artifact/abc"),
            Some(link)
        );
        let issue = "https://github.com/ldelvoye/taco-shells/issues/12";
        let issue_link = SharedTarget::Link { url: issue.into() };
        assert_eq!(
            target_of_share("other", issue),
            Some(issue_link),
            "an issue is a plain link"
        );
        let too_long = format!("/{}", "a".repeat(MAX_TARGET_CHARACTERS));
        assert_eq!(
            target_of_share("document", &too_long),
            None,
            "a cut path would open the wrong file"
        );
        assert_eq!(target_of("plan.md", Some("relative/dir")), None);
        for opens_nothing in ["http://example.com", "file:///etc/hosts", "~/notes.md"] {
            assert_eq!(
                target_of_share("other", opens_nothing),
                None,
                "{opens_nothing}"
            );
        }

        let text_only =
            serde_json::json!({ "kind": "other", "title": " Findings ", "body": "The cause" });
        let expected = EntryRead::Share {
            kind: "Note".into(),
            title: "Findings".into(),
            target: None,
            body: Some("The cause".into()),
        };
        assert_eq!(share(text_only), Some(expected));
        let unknown_kind = serde_json::json!({ "kind": "poem", "title": "T", "body": "b" });
        let Some(EntryRead::Share { kind, .. }) = share(unknown_kind) else {
            panic!("a share of an unknown kind is kept");
        };
        assert_eq!(kind, NOTE_KIND);
        let opens_nothing = serde_json::json!({ "kind": "other", "title": "T", "target": "~/x" });
        assert_eq!(
            share(opens_nothing),
            None,
            "nothing to open, nothing shared"
        );
        let untitled = serde_json::json!({ "kind": "other", "title": " ", "body": "b" });
        assert_eq!(share(untitled), None);
    }

    fn one_entry(event: &str, payload: &str) -> Option<EntryRead> {
        let mut reads = read_entries(event, payload);
        assert!(reads.len() <= 1, "one entry at most from {payload}");
        reads.pop()
    }
}
