use std::path::Path;

use serde_json::Value;
use taco_protocol::{AgentSignal, Peer, PrState, SharedTarget, Step};

use crate::harness::{
    AgentTitle, EntryRead, Found, Harness, HarnessSetup, PullRequestSeen, StepContext,
};

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
// A command or path is cut to this many characters in a log entry.
const MAX_ENTRY_CHARACTERS: usize = 1000;
// A prompt or a note keeps this many characters, then CUT_MARK.
const MAX_WRITTEN_CHARACTERS: usize = 8_000;
// The agent's line on what it is doing now is shown above the log, so it stays short.
const MAX_NOW_CHARACTERS: usize = 300;
// A failed step shows the first line of its error, this long at most.
const MAX_FAILURE_CHARACTERS: usize = 200;
const FAILED_WITHOUT_ERROR: &str = "Failed";
// How a failed command's error starts, before its own message on the next line.
const EXIT_CODE_PREFIX: &str = "Exit code ";
// Character caps on what a share carries; a target that does not fit opens nothing.
const MAX_TITLE_CHARACTERS: usize = 200;
const MAX_TARGET_CHARACTERS: usize = 1_000;
const MAX_BODY_CHARACTERS: usize = 20_000;
// The share tool's kinds, as the Shared list labels them; any other kind is labelled as a note.
const NOTE_KIND: &str = "Note";
const DOCUMENT_KIND: &str = "Document";
const IMAGE_KIND: &str = "Image";
const PULL_REQUEST_KIND: &str = "Pull request";
const ARTIFACT_KIND: &str = "Artifact";
const SHARE_KINDS: &[(&str, &str)] = &[
    ("document", DOCUMENT_KIND),
    ("image", IMAGE_KIND),
    ("pull_request", PULL_REQUEST_KIND),
    ("artifact", ARTIFACT_KIND),
    ("other", NOTE_KIND),
];
const LINK_PREFIX: &str = "https://";
const DOCUMENT_EXTENSIONS: &[&str] = &["md", "markdown", "rst", "adoc", "pdf"];
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp"];
// Folders whose files are the agent's or a tool's own, never things for the user.
const NOT_SHARED_FOLDERS: &[&str] = &["/.claude/", "/.git/", "/node_modules/"];
const DRAFT_FLAG: &str = "--draft";
const PULL_REQUEST_PREFIX: &str = "https://github.com/";
// Another session's message, as Claude Code hands it to the agent in a prompt.
const MESSAGE_OPEN: &str = "<cross-session-message";
const MESSAGE_CLOSE: &str = "</cross-session-message>";
// The tools that send another session a message; Claude Code has used both names.
const MESSAGE_TOOLS: &[&str] = &["SendMessage", "CrossSessionMessage"];
// How a session's address starts, as opposed to a name.
const SESSION_ADDRESS_PREFIX: &str = "uds:";
const CUT_MARK: &str = "… (cut)";
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

const UUID_LENGTH: usize = 36;
const UUID_HYPHENS: [usize; 4] = [8, 13, 18, 23];

static SETUP: HarnessSetup = HarnessSetup {
    product_name: "Claude Code",
    commands: &["claude"],
    config_directory_variable: "CLAUDE_CONFIG_DIR",
    default_config_directory: ".claude",
    settings_file: "settings.json",
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
    entry_reader: Some(read_entries),
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
            byte == b'-'
        } else {
            byte.is_ascii_hexdigit()
        }
    })
}

fn signal_for(event: &str, detail: Option<&str>) -> Option<AgentSignal> {
    match event {
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" => Some(AgentSignal::Working),
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

fn written_at(parsed: &Value, pointer: &str, limit: usize) -> Option<String> {
    let text = parsed.pointer(pointer)?.as_str()?;
    written(text, limit)
}

// Prose someone wrote (a prompt, a message, a note, a now line): trimmed, none when blank, and ending in CUT_MARK when cut.
fn written(text: &str, limit: usize) -> Option<String> {
    let kept = cut_to(text, limit + 1);
    if kept.is_empty() {
        return None;
    }
    let length = kept.chars().count();
    if length <= limit {
        return Some(kept);
    }
    let shortened: String = kept.chars().take(limit).collect();
    Some(format!("{shortened}{CUT_MARK}"))
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

// A text that is only any use whole, such as a path or a link: none when it is blank or longer than `limit`.
fn whole_text_at(parsed: &Value, pointer: &str, limit: usize) -> Option<String> {
    let text = parsed.pointer(pointer)?.as_str()?;
    let trimmed = text.trim();
    let length = trimmed.chars().count();
    if trimmed.is_empty() || length > limit {
        return None;
    }
    Some(trimmed.to_string())
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

// A pull request's plain link and number, ("https://github.com/o/r/pull/155", 155) from ".../pull/155#discussion",
// so one pull request shared from two of its pages is the same thing.
fn pull_request_of(url: &str) -> Option<(String, u64)> {
    let rest = url.strip_prefix(PULL_REQUEST_PREFIX)?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repository = parts.next()?;
    let pull = parts.next()?;
    if owner.is_empty() || repository.is_empty() || pull != "pull" {
        return None;
    }
    let number_part = parts.next()?;
    let digits: String = number_part
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect();
    let after = &number_part[digits.len()..];
    let ends_the_number = after.is_empty() || after.starts_with(['#', '?']);
    if !ends_the_number {
        return None;
    }
    let number: u64 = digits.parse().ok()?;
    let plain = format!("{PULL_REQUEST_PREFIX}{owner}/{repository}/pull/{number}");
    Some((plain, number))
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

// What the app shares by itself from a tool the agent ran: a pull request, from the gitOperation Claude Code reads
// out of a gh command, a document or image the agent wrote, or an artifact it published.
fn found_by_app(parsed: &Value) -> Option<Found> {
    let tool = text_at(parsed, "/tool_name")?;
    match tool.as_str() {
        "Bash" => found_pull_request(parsed),
        "Write" => found_file(parsed),
        "Artifact" => found_artifact(parsed),
        _ => None,
    }
}

fn found_pull_request(parsed: &Value) -> Option<Found> {
    let operation = parsed.pointer("/tool_response/gitOperation/pr")?;
    let number = operation.pointer("/number")?.as_u64()?;
    let action = operation.pointer("/action")?.as_str()?;
    let mut url = None;
    let given = operation.pointer("/url");
    if let Some(Value::String(given)) = given
        && let Some((plain, _)) = pull_request_of(given)
    {
        url = Some(plain);
    }
    // the response never says "draft", so the flag on the whole command does, wherever it comes in a long one
    let command = parsed.pointer("/tool_input/command");
    let mut as_draft = false;
    if let Some(Value::String(command)) = command {
        let mut words = command.split_whitespace();
        as_draft = words.any(|word| word == DRAFT_FLAG);
    }
    let state = match action {
        "created" if as_draft => Some(PrState::Draft),
        "created" | "ready" => Some(PrState::Open),
        "merged" => Some(PrState::Merged),
        "closed" => Some(PrState::Closed),
        "edited" => None,
        _ => return None,
    };
    let mut title = format!("#{number}");
    if let Some(url) = &url
        && let Some(repository) = repository_of(url)
    {
        title = format!("#{number} {repository}");
    }
    Some(Found::PullRequest(PullRequestSeen {
        url,
        number,
        state,
        kind: PULL_REQUEST_KIND.to_string(),
        title,
    }))
}

// "ldelvoye/taco-shells" from "https://github.com/ldelvoye/taco-shells/pull/155".
fn repository_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix(PULL_REQUEST_PREFIX)?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    Some(format!("{owner}/{name}"))
}

// Only a new file is shared; rewriting one that exists is not news.
fn found_file(parsed: &Value) -> Option<Found> {
    let write = text_at(parsed, "/tool_response/type")?;
    if write != "create" {
        return None;
    }
    let path = whole_text_at(parsed, "/tool_response/filePath", MAX_TARGET_CHARACTERS)?;
    let file = Path::new(&path);
    let extension = file.extension()?.to_str()?;
    let lowered = extension.to_lowercase();
    let is_document = DOCUMENT_EXTENSIONS.contains(&lowered.as_str());
    let is_image = IMAGE_EXTENSIONS.contains(&lowered.as_str());
    let kind = match (is_document, is_image) {
        (true, _) => DOCUMENT_KIND,
        (false, true) => IMAGE_KIND,
        (false, false) => return None,
    };
    let scratchpad = text_at(parsed, "/scratchpad_dir");
    let in_scratchpad = scratchpad.is_some_and(|folder| path.starts_with(&folder));
    let in_a_tool_folder = NOT_SHARED_FOLDERS
        .iter()
        .any(|folder| path.contains(folder));
    let is_absolute = file.is_absolute();
    if in_scratchpad || in_a_tool_folder || !is_absolute {
        return None;
    }
    let name = file.file_name()?.to_str()?;
    let title = written(name, MAX_TITLE_CHARACTERS)?;
    Some(Found::Thing {
        kind: kind.to_string(),
        title,
        target: SharedTarget::File { path },
    })
}

// A page the agent published; the Artifact tool publishes when it is given no action, and an asset is no page.
fn found_artifact(parsed: &Value) -> Option<Found> {
    let action = text_at(parsed, "/tool_input/action");
    let publishes = action.is_none() || action.as_deref() == Some("publish");
    let asset = parsed.pointer("/tool_input/asset");
    if !publishes || asset == Some(&Value::Bool(true)) {
        return None;
    }
    let url = whole_text_at(parsed, "/tool_response/url", MAX_TARGET_CHARACTERS)?;
    if !url.starts_with(LINK_PREFIX) {
        return None;
    }
    let titled = written_at(parsed, "/tool_response/title", MAX_TITLE_CHARACTERS);
    let title = match titled {
        Some(title) => title,
        None => ARTIFACT_KIND.to_string(),
    };
    Some(Found::Thing {
        kind: ARTIFACT_KIND.to_string(),
        title,
        target: SharedTarget::Link { url },
    })
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

fn read_entries(event: &str, payload: &str) -> Vec<EntryRead> {
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
        let notebook =
            r#"{"tool_name":"NotebookEdit","tool_input":{"notebook_path":"/w/a.ipynb"}}"#;
        assert_eq!(
            entry("PostToolUse", notebook),
            Some(changed("NotebookEdit", "/w/a.ipynb"))
        );
        let multi = r#"{"tool_name":"MultiEdit","tool_input":{"file_path":"/w/a.rs","edits":[]}}"#;
        assert_eq!(
            entry("PostToolUse", multi),
            Some(changed("MultiEdit", "/w/a.rs"))
        );
        // reading a file names it, but changes nothing
        let read = r#"{"tool_name":"Read","tool_input":{"file_path":"/w/a.rs"}}"#;
        assert_eq!(entry("PostToolUse", read), Some(did("Read", "/w/a.rs")));
        let grep = r#"{"tool_name":"Grep","tool_input":{"pattern":"fn main","path":"/w"}}"#;
        assert_eq!(entry("PostToolUse", grep), Some(did("Grep", "fn main")));
        let agent =
            r#"{"tool_name":"Agent","tool_input":{"description":"Find the bug","prompt":"..."}}"#;
        assert_eq!(
            entry("PostToolUse", agent),
            Some(did("Agent", "Find the bug"))
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
        assert_eq!(
            signal_for("PostToolUseFailure", Some("Bash")),
            Some(AgentSignal::Working),
            "a tool that failed is the agent still at work"
        );
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

    #[test]
    fn the_app_finds_pull_requests_documents_images_and_artifacts_in_the_agents_tools() {
        let found = |payload: serde_json::Value| {
            let mut reads = read_entries("PostToolUse", &payload.to_string());
            let last = reads.pop();
            match last {
                Some(EntryRead::Found(found)) => Some(found),
                _ => None,
            }
        };
        let gh = |command: &str, operation: serde_json::Value| {
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": command },
                "tool_response": { "stdout": "", "gitOperation": { "pr": operation } },
            })
        };
        let seen = |url: Option<&str>, number: u64, state: Option<PrState>| {
            let mut title = format!("#{number}");
            if url.is_some() {
                title = format!("#{number} o/r");
            }
            Found::PullRequest(PullRequestSeen {
                url: url.map(str::to_string),
                number,
                state,
                kind: PULL_REQUEST_KIND.to_string(),
                title,
            })
        };
        let created = serde_json::json!({ "number": 79, "url": "https://github.com/o/r/pull/79", "action": "created" });
        assert_eq!(
            found(gh("gh pr create --title x", created.clone())),
            Some(seen(
                Some("https://github.com/o/r/pull/79"),
                79,
                Some(PrState::Open)
            ))
        );
        assert_eq!(
            found(gh("gh pr create --draft --title x", created)),
            Some(seen(
                Some("https://github.com/o/r/pull/79"),
                79,
                Some(PrState::Draft)
            ))
        );
        let created = serde_json::json!({ "number": 79, "url": "https://github.com/o/r/pull/79", "action": "created" });
        let long_body = format!("gh pr create --body '{}' --draft", "b".repeat(2_000));
        let Some(Found::PullRequest(after_a_long_body)) = found(gh(&long_body, created.clone()))
        else {
            panic!("a created pull request is found");
        };
        assert_eq!(
            after_a_long_body.state,
            Some(PrState::Draft),
            "the flag counts wherever it comes"
        );
        let Some(Found::PullRequest(not_a_draft)) =
            found(gh("gh pr create --draft=false --title x", created))
        else {
            panic!("a created pull request is found");
        };
        assert_eq!(
            not_a_draft.state,
            Some(PrState::Open),
            "only the flag itself makes a draft"
        );
        let merged = serde_json::json!({ "number": 79, "action": "merged" });
        assert_eq!(
            found(gh("gh pr merge 79", merged)),
            Some(seen(None, 79, Some(PrState::Merged)))
        );
        let closed = serde_json::json!({ "number": 79, "action": "closed" });
        assert_eq!(
            found(gh("gh pr close 79", closed)),
            Some(seen(None, 79, Some(PrState::Closed)))
        );
        let edited = serde_json::json!({ "number": 79, "url": "https://github.com/o/r/pull/79", "action": "edited" });
        assert_eq!(
            found(gh("gh pr edit 79", edited)),
            Some(seen(Some("https://github.com/o/r/pull/79"), 79, None)),
            "an edit says nothing of the state"
        );
        let commented = serde_json::json!({ "number": 79, "action": "commented" });
        assert_eq!(found(gh("gh pr comment 79", commented)), None);
        let bash = read_entries(
            "PostToolUse",
            &gh(
                "gh pr merge 79",
                serde_json::json!({ "number": 79, "action": "merged" }),
            )
            .to_string(),
        );
        assert!(
            matches!(bash[0], EntryRead::Step { .. }),
            "the command is still a step"
        );

        let wrote = |path: &str, write: &str| {
            serde_json::json!({
                "tool_name": "Write",
                "tool_input": { "file_path": path },
                "tool_response": { "type": write, "filePath": path },
                "scratchpad_dir": "/tmp/scratch",
            })
        };
        let file = |kind: &str, title: &str, path: &str| Found::Thing {
            kind: kind.into(),
            title: title.into(),
            target: SharedTarget::File { path: path.into() },
        };
        assert_eq!(
            found(wrote("/work/PLAN.md", "create")),
            Some(file("Document", "PLAN.md", "/work/PLAN.md"))
        );
        assert_eq!(
            found(wrote("/work/logo.PNG", "create")),
            Some(file("Image", "logo.PNG", "/work/logo.PNG"))
        );
        for not_shared in [
            wrote("/work/PLAN.md", "update"),
            wrote("/work/main.rs", "create"),
            wrote("/work/notes.txt", "create"),
            wrote("/tmp/scratch/page.md", "create"),
            wrote("/Users/x/.claude/projects/memory.md", "create"),
            wrote("/work/.git/COMMIT_EDITMSG.md", "create"),
            wrote("/work/node_modules/a/README.md", "create"),
            wrote("PLAN.md", "create"),
        ] {
            assert_eq!(found(not_shared.clone()), None, "{not_shared}");
        }

        let artifact = |input: serde_json::Value, response: serde_json::Value| serde_json::json!({ "tool_name": "Artifact", "tool_input": input, "tool_response": response });
        let published = serde_json::json!({ "url": "https://claude.ai/artifact/abc", "title": "Window choices", "updated": false });
        let page = Found::Thing {
            kind: "Artifact".into(),
            title: "Window choices".into(),
            target: SharedTarget::Link {
                url: "https://claude.ai/artifact/abc".into(),
            },
        };
        assert_eq!(
            found(artifact(
                serde_json::json!({ "file_path": "/x/index.html" }),
                published.clone()
            )),
            Some(page)
        );
        assert_eq!(
            found(artifact(
                serde_json::json!({ "action": "read" }),
                published.clone()
            )),
            None
        );
        assert_eq!(
            found(artifact(serde_json::json!({ "asset": true }), published)),
            None
        );
        let not_https = serde_json::json!({ "url": "http://claude.ai/artifact/abc", "title": "T" });
        assert_eq!(found(artifact(serde_json::json!({}), not_https)), None);
        let untitled = serde_json::json!({ "url": "https://claude.ai/artifact/abc" });
        let Some(Found::Thing { title, .. }) = found(artifact(serde_json::json!({}), untitled))
        else {
            panic!("an untitled page is still shared");
        };
        assert_eq!(title, ARTIFACT_KIND);
    }

    fn one_entry(event: &str, payload: &str) -> Option<EntryRead> {
        let mut reads = read_entries(event, payload);
        assert!(reads.len() <= 1, "one entry at most from {payload}");
        reads.pop()
    }
}
