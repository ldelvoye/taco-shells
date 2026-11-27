//! Made-up states the window can be drawn from, each built only through the inputs the host sends the real workspace.
//! No fixture draws a draft, merged or closed pull request pill yet; only "Open, seen" and a pull request with no pill.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value, json};
use taco_agents::{activity_tool_prefix, reduced_payload};
use taco_protocol::{
    AgentSignal, DrawnState, HarnessId, HookCall, HookEvent, Panel, Peer, RequestReply, RowState,
    RuntimeReport, TabGroupId, TerminalId, WorkspaceRequest,
};
use taco_services::{
    AgentMenuAction, AgentSetupNotice, AgentSetupNoticeKind, AgentSetupView, changed_file_text,
    done_text, offer_text, plugin_dirs_hidden_text, too_old_reason,
};
use taco_settings::{
    Binding, ConfigTexts, SettingChange, Settings, ShellRowName, UnreadSettings, resolve_config,
};
use taco_workspace::{Outbound, Workspace};

use crate::activity_panel::{MS_PER_MINUTE, now_ms};
use crate::banners::Notices;
use crate::stand_in::{PRODUCT_NAME, delivered, home, stand_in_workspace};
use crate::window::Shown;

const CLAUDE: &str = "claude";
const STATE_MINUTES_AGO: u64 = 52;
const QUESTION: &str = "Can I force-push the rebased branch?";
const NOTIFICATION: &str = "Notification";
const PERMISSION_PROMPT: &str = "permission_prompt";
const FONT_SIZE_PROBLEM: &str = "settings.json: \"fontSize\" must be a number between 1 and 72";
const KEYBINDINGS_PROBLEM: &str = "keybindings.json: expected a comma at line 7";
const OLD_VERSION: &str = "2.1.212";
const SETTINGS_FILE: &str = "~/.claude/settings.json";
const PLAIN_SHELL_ROW: usize = 4;

const PLACEHOLDER_SCREEN: [&str; 8] = [
    "~/code/getsentry % claude",
    "",
    "> rework the billing platform lock",
    "",
    "  Reading billing/platform/lock.py",
    "  Running the billing tests: 214 passed",
    "",
    "  Rebasing onto master",
];

const MIXED_ROWS: [FixtureRow; 7] = [
    FixtureRow {
        title: "claude: lock rework",
        tab_group: Some("billing"),
        state: Some(RowState::Working),
        directory: "code/getsentry",
    },
    FixtureRow {
        title: "zsh",
        tab_group: Some("billing"),
        state: None,
        directory: "code/getsentry",
    },
    FixtureRow {
        title: "claude: review PR",
        tab_group: None,
        state: Some(RowState::Needs),
        directory: "code/sentry",
    },
    FixtureRow {
        title: "claude: flaky test",
        tab_group: None,
        state: Some(RowState::Done),
        directory: "code/sentry",
    },
    FixtureRow {
        title: "zsh",
        tab_group: None,
        state: None,
        directory: "code",
    },
    FixtureRow {
        title: "aider",
        tab_group: None,
        state: None,
        directory: "code/smorg",
    },
    FixtureRow {
        title: "claude: docs",
        tab_group: None,
        state: Some(RowState::Idle),
        directory: "code/sentry-docs",
    },
];

const ASKED: &str = "asked";
const WORKING_ROW: usize = 0;
const NEEDS_ROW: usize = 2;
const DONE_ROW: usize = 3;
// Enough retries that the workspace's 200-entry page starts inside the nudged fix's noted stretch.
const PAGED_RETRIES: usize = 90;
const USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
const POST_TOOL_USE: &str = "PostToolUse";
const POST_TOOL_USE_FAILURE: &str = "PostToolUseFailure";
const STARTUP: &str = "startup";
const COMPACT: &str = "compact";
const MESSAGE_OPEN: &str = "<cross-session-message";
const MESSAGE_CLOSE: &str = "</cross-session-message>";
const TOOL_INPUTS: [(&str, &str); 6] = [
    ("Bash", "command"),
    ("Read", "file_path"),
    ("Edit", "file_path"),
    ("Write", "file_path"),
    ("Grep", "pattern"),
    ("Glob", "pattern"),
];
const PULL_REQUESTS: &str = "https://github.com/ldelvoye/taco-shells/pull/";
const ARTIFACT_LINK: &str = "https://claude.ai/artifact/9NXFTKtkNKmvbxjMDsn1zL";
const REVIEWER: &str = "code-83";
const NARROW_PANEL: f32 = 140.;
const PLAN_NOTE: &str = "Wrote PLAN-sigkill.md. Today a closed terminal or a stopping runtime gets one SIGHUP and \
nothing after it, so a shell that ignores it keeps running with its children. The plan adds a SIGKILL to the whole \
shell session, 2 s after a terminal is closed and 500 ms after the runtime is told to stop, and a test for each. It \
also covers what could go wrong: a shell that traps SIGHUP to save history gets its 2 s, and a process that left the \
session on purpose, like a daemon started with setsid, is not touched. No code was changed.";
const NUDGED_NOTE: &str = "Hook script paths are now quoted safely for any characters, and plain paths stay \
unquoted as before. All 48 taco-services tests pass, including a new one that runs each quoted path through the \
shell. Nothing is committed.";
const REVIEW_MESSAGE: &str = "Review of #155: faint box lines are painted twice where a line's two halves meet, so a \
dim border reads as dotted. Fix before merging.";
const FIXED_MESSAGE: &str = "#155 is fixed: the arms are cut into rectangles that no longer overlap, so a faint line \
stays solid. Tests and clippy are clean.";
const HEADS_UP: &str = "Heads-up: main moved to 42bf29b while you were rebasing.";
const ADDRESS: &str = "uds:/tmp/cc-socks/9838.sock";
const NOW_LINE: &str = "Rebasing #179 onto main, then resolving the conflict in draw.rs.";
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const CHECKOUT_IMAGE: &str = "branding/icons/taco-shells.svg";
const PLAN_PATH: &str = "/work/taco-shells/PLAN-sigkill.md";
/// The text of the finding the agent shares, sent when it is opened.
const FINDING_BODY: &str = "Closing a terminal sends its shell one SIGHUP and nothing after it.

A shell that ignores SIGHUP keeps running, and so do its children: in a test, `trap '' HUP; sleep 600` outlived the terminal by the full ten minutes.

The runtime does the same when it is told to stop. It sends SIGHUP to each shell's session and exits without waiting, so a process that ignores it is left behind with no terminal.

The plan in PLAN-sigkill.md adds a SIGKILL to the whole session after a short grace period.";

const WORKING_LOG: [FixtureEntry; 10] = [
    entry(42, ASKED, "rework the billing platform lock"),
    entry(41, "Grep", "acquire_lock"),
    entry(40, "Read", "billing/platform/lock.py"),
    entry(40, "Read", "billing/platform/models.py"),
    entry(39, "Read", "billing/tests/test_lock.py"),
    entry(31, "Edit", "billing/platform/lock.py"),
    entry(30, "Edit", "billing/tests/test_lock.py"),
    entry(22, "Bash", "pytest billing/tests"),
    entry(4, ASKED, "now rebase it onto master"),
    entry(3, "Bash", "git rebase master"),
];

const NEEDS_LOG: [FixtureEntry; 3] = [
    entry(18, ASKED, "address the review comments on the PR"),
    entry(12, "Edit", "src/sentry/api/endpoints/project_details.py"),
    entry(9, "Bash", "git rebase origin/master"),
];

const DONE_LOG: [FixtureEntry; 3] = [
    entry(75, ASKED, "find why test_flaky_retry fails on CI"),
    entry(70, "Glob", "tests/**/test_retry*.py"),
    entry(52, "Edit", "tests/sentry/test_retry.py"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixture {
    /// A tab group, agents in each state, plain shells, and two banners.
    Mixed,
    /// The same, without the banners.
    Quiet,
    /// No terminals and no banners.
    Empty,
    /// The quiet one with a settings file whose font size could not be read.
    Broken,
    /// The quiet one with plain shells named by their short path.
    Paths,
    /// The quiet one with some switches on and some off.
    Switches,
    /// The quiet one with a keybindings.json that could not be read.
    BrokenKeys,
    /// The switches one, whose settings writes are applied, so a click moves its control.
    Writable,
    /// The quiet one whose working agent has written every kind of log line.
    Activity,
    /// The same with Agent Activity at its narrowest.
    ActivityNarrow,
    /// The same log sent a page at a time, the newest page starting inside a noted stretch.
    Paged,
    /// The quiet one on a plain shell where Claude Code runs but Agent Activity is off, or still on the old hooks.
    SetupOff,
    /// The quiet one on a plain shell where Claude Code is too old for Agent Activity.
    SetupTooOld,
    /// The activity one just after Set up, with the hint above the running agent's log.
    SetupDone,
    /// The activity just after a Set up that changed the agent's settings file.
    SetupDoneChanged,
    /// Agent Activity off, and an agent known only by its title, with nothing in its log: the offer shows beside it.
    SetupOffAgent,
    /// Agent Activity on, and an agent known only by its title, with nothing in its log yet.
    AgentNoLog,
    /// The longest offer, Claude Code's own settings replacing the plugin folder, beside an agent.
    SetupPluginDirs,
    /// The same with Agent Activity at its narrowest.
    SetupPluginDirsNarrow,
}

struct FixtureRow {
    title: &'static str,
    tab_group: Option<&'static str>,
    state: Option<RowState>,
    directory: &'static str,
}

/// One log entry: `tool` is the tool the agent ran, or `ASKED` for a prompt.
struct FixtureEntry {
    minutes_ago: u64,
    tool: &'static str,
    text: &'static str,
}

/// The agent session in a terminal whose hooks are fed.
struct AgentLog {
    terminal: TerminalId,
    session_id: String,
}

/// Feeds the workspace what the host would: rows at the instant each agent row's state began, hooks at their own times.
struct Inputs {
    workspace: Workspace,
    now_ms: u64,
    at_ms: u64,
    focus_moves: u64,
    turns: u64,
}

impl Fixture {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "mixed" => Some(Self::Mixed),
            "quiet" => Some(Self::Quiet),
            "empty" => Some(Self::Empty),
            "broken" => Some(Self::Broken),
            "paths" => Some(Self::Paths),
            "switches" => Some(Self::Switches),
            "brokenkeys" => Some(Self::BrokenKeys),
            "writable" => Some(Self::Writable),
            "activity" => Some(Self::Activity),
            "activitynarrow" => Some(Self::ActivityNarrow),
            "paged" => Some(Self::Paged),
            "setupoff" => Some(Self::SetupOff),
            "setuptooold" => Some(Self::SetupTooOld),
            "setupdone" => Some(Self::SetupDone),
            "setupdonechanged" => Some(Self::SetupDoneChanged),
            "setupoffagent" => Some(Self::SetupOffAgent),
            "agentnolog" => Some(Self::AgentNoLog),
            "setupplugindirs" => Some(Self::SetupPluginDirs),
            "setupplugindirsnarrow" => Some(Self::SetupPluginDirsNarrow),
            _ => None,
        }
    }

    pub(crate) fn workspace(self, panel: Option<f32>) -> Workspace {
        let now = now_ms();
        let mut inputs = Inputs::new(now);
        inputs.start();
        if self == Self::Empty {
            let opened = terminal_at(0);
            inputs.close(&opened);
        } else {
            mixed_rows(&mut inputs);
            self.agent_logs(&mut inputs);
        }
        match self {
            Self::ActivityNarrow | Self::SetupPluginDirsNarrow => {
                inputs.resize_activity(NARROW_PANEL);
            }
            Self::SetupOff | Self::SetupTooOld => {
                let plain_shell = terminal_at(PLAIN_SHELL_ROW);
                inputs.select(&plain_shell);
            }
            _ => {}
        }
        if let Some(size) = panel {
            inputs.resize_activity(size);
        }
        inputs.workspace
    }

    pub(crate) fn shown(self, drawn: DrawnState) -> Shown {
        match self {
            Self::Mixed => mixed(drawn),
            Self::Quiet => quiet(drawn),
            Self::Empty => empty(drawn),
            Self::Broken => broken(drawn),
            Self::Paths => paths(drawn),
            Self::Switches => switches(drawn),
            Self::BrokenKeys => broken_keys(drawn),
            Self::Writable => switches(drawn),
            Self::Activity | Self::ActivityNarrow | Self::Paged => quiet(drawn),
            Self::SetupOff => setup_on_shell(drawn, offer_kind()),
            Self::SetupTooOld => setup_on_shell(drawn, too_old_kind()),
            Self::SetupDone => done_beside_agent(drawn, None),
            Self::SetupDoneChanged => done_beside_agent(drawn, Some(SETTINGS_FILE)),
            Self::SetupOffAgent => {
                let offer = offer_text(PRODUCT_NAME);
                setup_beside_agent(drawn, offer_kind(), &offer)
            }
            Self::AgentNoLog => quiet(drawn),
            Self::SetupPluginDirs | Self::SetupPluginDirsNarrow => {
                let offer = plugin_dirs_offer_text();
                setup_beside_agent(drawn, offer_kind(), &offer)
            }
        }
    }

    pub(crate) fn applies_writes(self) -> bool {
        self == Self::Writable
    }

    fn agent_logs(self, inputs: &mut Inputs) {
        let working = agent_log(WORKING_ROW);
        match self {
            Self::Activity | Self::ActivityNarrow | Self::SetupDone | Self::SetupDoneChanged => {
                full_story(inputs, &working, 0);
            }
            Self::Paged => full_story(inputs, &working, PAGED_RETRIES),
            Self::SetupOffAgent
            | Self::AgentNoLog
            | Self::SetupPluginDirs
            | Self::SetupPluginDirsNarrow => {}
            _ => short_log(inputs, &working, &WORKING_LOG),
        }
        let needs = agent_log(NEEDS_ROW);
        short_log(inputs, &needs, &NEEDS_LOG);
        let done = agent_log(DONE_ROW);
        short_log(inputs, &done, &DONE_LOG);
    }
}

impl Inputs {
    fn new(now_ms: u64) -> Self {
        let ago = STATE_MINUTES_AGO * MS_PER_MINUTE;
        let at_ms = now_ms.saturating_sub(ago);
        Self {
            workspace: stand_in_workspace(),
            now_ms,
            at_ms,
            focus_moves: 0,
            turns: 0,
        }
    }

    fn deliver(&mut self, out: Vec<Outbound>) {
        delivered(&mut self.workspace, out, self.at_ms);
    }

    fn ago(&self, minutes: u64) -> u64 {
        let ago = minutes * MS_PER_MINUTE;
        self.now_ms.saturating_sub(ago)
    }

    fn turn(&self) -> String {
        format!("turn-{}", self.turns)
    }

    fn start(&mut self) {
        let out = self.workspace.start(self.at_ms);
        self.deliver(out);
    }

    fn request(&mut self, request: WorkspaceRequest) -> RequestReply {
        let (reply, out) = self.workspace.request(request, self.at_ms);
        self.deliver(out);
        reply
    }

    fn report(&mut self, report: RuntimeReport) {
        let out = self.workspace.runtime_report(report, self.at_ms);
        self.deliver(out);
    }

    fn hook(&mut self, terminal: &TerminalId, call: HookCall) {
        self.hook_at(terminal, call, self.at_ms);
    }

    fn hook_at(&mut self, terminal: &TerminalId, call: HookCall, at_ms: u64) {
        let event = HookEvent {
            terminal: terminal.clone(),
            call,
        };
        let out = self.workspace.hook(event, at_ms);
        self.deliver(out);
    }

    fn select(&mut self, terminal: &TerminalId) {
        self.focus_moves += 1;
        let request = WorkspaceRequest::FocusReported {
            terminal: terminal.clone(),
            count: self.focus_moves,
        };
        self.request(request);
    }

    fn close(&mut self, terminal: &TerminalId) {
        let request = WorkspaceRequest::CloseTerminal {
            terminal: terminal.clone(),
        };
        self.request(request);
    }

    fn tab_group(&mut self, terminal: &TerminalId, name: &str) -> Option<TabGroupId> {
        let created = WorkspaceRequest::TabGroupCreated {
            terminal: terminal.clone(),
        };
        let reply = self.request(created);
        let id = reply.tab_group?;
        let renamed = WorkspaceRequest::TabGroupRenamed {
            id: id.clone(),
            name: name.to_string(),
        };
        self.request(renamed);
        Some(id)
    }

    fn resize_activity(&mut self, size: f32) {
        let request = WorkspaceRequest::PanelResized {
            panel: Panel::AgentActivity,
            size,
        };
        self.request(request);
    }

    fn agent(&mut self, terminal: &TerminalId, index: usize, cwd: &str, state: RowState) {
        let session_id = session_at(index);
        let start = HookCall::AgentStart {
            harness: claude(),
            session_id: session_id.clone(),
            cwd: cwd.to_string(),
            has_conversation: true,
        };
        self.hook(terminal, start);
        if state == RowState::Needs {
            self.permission_prompt(terminal, &session_id);
        }
        let signal = match state {
            RowState::Idle => return,
            RowState::Working => AgentSignal::Working,
            RowState::Needs => AgentSignal::Needs,
            RowState::Done => AgentSignal::Stopped,
        };
        let call = HookCall::AgentState {
            harness: claude(),
            session_id,
            signal,
        };
        self.hook(terminal, call);
    }

    fn permission_prompt(&mut self, terminal: &TerminalId, session_id: &str) {
        let payload = serde_json::json!({
            "hook_event_name": NOTIFICATION,
            "notification_type": PERMISSION_PROMPT,
            "message": QUESTION,
        });
        let call = HookCall::AgentHook {
            harness: claude(),
            session_id: session_id.to_string(),
            event: NOTIFICATION.to_string(),
            payload: as_the_plugin_sends(&payload),
        };
        self.hook(terminal, call);
    }

    fn agent_hook(&mut self, log: &AgentLog, minutes_ago: u64, event: &str, payload: Value) {
        let at_ms = self.ago(minutes_ago);
        let call = HookCall::AgentHook {
            harness: claude(),
            session_id: log.session_id.clone(),
            event: event.to_string(),
            payload: as_the_plugin_sends(&payload),
        };
        self.hook_at(&log.terminal, call, at_ms);
    }

    fn submit(&mut self, log: &AgentLog, minutes_ago: u64, prompt: &str) {
        let turn = self.turn();
        let payload = json!({
            "hook_event_name": USER_PROMPT_SUBMIT,
            "prompt": prompt,
            "prompt_id": turn,
        });
        self.agent_hook(log, minutes_ago, USER_PROMPT_SUBMIT, payload);
    }

    fn asked(&mut self, log: &AgentLog, minutes_ago: u64, text: &str) {
        self.turns += 1;
        self.submit(log, minutes_ago, text);
    }

    fn heard(&mut self, log: &AgentLog, minutes_ago: u64, from: &Peer, text: &str) {
        self.turns += 1;
        self.heard_mid_turn(log, minutes_ago, from, text);
    }

    fn heard_mid_turn(&mut self, log: &AgentLog, minutes_ago: u64, from: &Peer, text: &str) {
        let prompt = message_prompt(from, text);
        self.submit(log, minutes_ago, &prompt);
    }

    fn tool_ran(&mut self, log: &AgentLog, minutes_ago: u64, mut payload: Value) {
        let turn = self.turn();
        payload["hook_event_name"] = Value::from(POST_TOOL_USE);
        payload["prompt_id"] = Value::from(turn);
        self.agent_hook(log, minutes_ago, POST_TOOL_USE, payload);
    }

    fn step(&mut self, log: &AgentLog, minutes_ago: u64, tool: &str, text: &str) {
        let input = tool_input(tool, text);
        let payload = json!({ "tool_name": tool, "tool_input": input });
        self.tool_ran(log, minutes_ago, payload);
    }

    fn failed_step(
        &mut self,
        log: &AgentLog,
        minutes_ago: u64,
        tool: &str,
        text: &str,
        error: &str,
    ) {
        let input = tool_input(tool, text);
        let turn = self.turn();
        let payload = json!({
            "hook_event_name": POST_TOOL_USE_FAILURE,
            "tool_name": tool,
            "tool_input": input,
            "error": error,
            "prompt_id": turn,
        });
        self.agent_hook(log, minutes_ago, POST_TOOL_USE_FAILURE, payload);
    }

    fn wrote(&mut self, log: &AgentLog, minutes_ago: u64, path: &str) {
        let payload = json!({
            "tool_name": "Write",
            "tool_input": { "file_path": path },
            "tool_response": { "type": "create", "filePath": path },
        });
        self.tool_ran(log, minutes_ago, payload);
    }

    fn sent(&mut self, log: &AgentLog, minutes_ago: u64, to: &str, text: &str) {
        let payload = json!({
            "tool_name": "SendMessage",
            "tool_input": { "to": to, "message": text },
            "tool_response": { "success": true },
        });
        self.tool_ran(log, minutes_ago, payload);
    }

    fn pull_request_seen(
        &mut self,
        log: &AgentLog,
        minutes_ago: u64,
        command: &str,
        number: u64,
        action: &str,
    ) {
        let url = pull_request_link(number);
        let operation = json!({ "number": number, "url": url, "action": action });
        let response = json!({ "stdout": "", "gitOperation": { "pr": operation } });
        let payload = json!({
            "tool_name": "Bash",
            "tool_input": { "command": command },
            "tool_response": response,
        });
        self.tool_ran(log, minutes_ago, payload);
    }

    fn activity_tool(&mut self, log: &AgentLog, minutes_ago: u64, tool: &str, input: Value) {
        let prefix = activity_tool_prefix();
        let name = format!("{prefix}{tool}");
        let payload = json!({ "tool_name": name, "tool_input": input });
        self.tool_ran(log, minutes_ago, payload);
    }

    fn note(&mut self, log: &AgentLog, minutes_ago: u64, text: &str) {
        let input = json!({ "text": text });
        self.activity_tool(log, minutes_ago, "note", input);
    }

    fn now_line(&mut self, log: &AgentLog, minutes_ago: u64, text: &str) {
        let input = json!({ "text": text });
        self.activity_tool(log, minutes_ago, "now", input);
    }

    fn share(&mut self, log: &AgentLog, minutes_ago: u64, kind: &str, title: &str, target: &str) {
        let input = json!({ "kind": kind, "title": title, "target": target });
        self.activity_tool(log, minutes_ago, "share", input);
    }

    fn share_text(&mut self, log: &AgentLog, minutes_ago: u64, title: &str, body: &str) {
        let input = json!({ "kind": "other", "title": title, "body": body });
        self.activity_tool(log, minutes_ago, "share", input);
    }

    fn session_started(&mut self, log: &AgentLog, minutes_ago: u64, source: &str) {
        let at_ms = self.ago(minutes_ago);
        let harness = claude();
        let (_, out) =
            self.workspace
                .session_context(&log.terminal, &harness, &log.session_id, source, at_ms);
        self.deliver(out);
    }

    fn asked_for_note(&mut self, log: &AgentLog, minutes_ago: u64) {
        let at_ms = self.ago(minutes_ago);
        let harness = claude();
        let out = self
            .workspace
            .asked_for_note(&log.terminal, &harness, &log.session_id, at_ms);
        self.deliver(out);
    }
}

// Trimmed as the plugin trims a payload before the app sees it, so a fixture shows only what the app can.
fn as_the_plugin_sends(payload: &Value) -> String {
    let reduced = reduced_payload(payload);
    reduced.to_string()
}

fn claude() -> HarnessId {
    HarnessId(CLAUDE.to_string())
}

fn terminal_at(index: usize) -> TerminalId {
    TerminalId(format!("t{index}"))
}

fn mixed_rows(inputs: &mut Inputs) {
    let mut tab_groups: BTreeMap<&str, TabGroupId> = BTreeMap::new();
    for (index, fixture_row) in MIXED_ROWS.iter().enumerate() {
        let terminal = terminal_at(index);
        let opened_at_launch = index == 0;
        if !opened_at_launch {
            let mut tab_group = None;
            if let Some(name) = fixture_row.tab_group {
                tab_group = tab_groups.get(name).cloned();
            }
            let open = WorkspaceRequest::OpenTerminalIn { tab_group };
            inputs.request(open);
        }
        let Some(name) = fixture_row.tab_group else {
            continue;
        };
        if tab_groups.contains_key(name) {
            continue;
        }
        if let Some(id) = inputs.tab_group(&terminal, name) {
            tab_groups.insert(name, id);
        }
    }
    let first = terminal_at(0);
    inputs.select(&first);
    for (index, fixture_row) in MIXED_ROWS.iter().enumerate() {
        let terminal = terminal_at(index);
        let directory = home().join(fixture_row.directory);
        let directory_text = directory.to_string_lossy().into_owned();
        let moved = RuntimeReport::DirectoryChanged {
            id: terminal.clone(),
            directory: directory_text.clone(),
        };
        inputs.report(moved);
        let titled = RuntimeReport::Titled {
            id: terminal.clone(),
            title: fixture_row.title.to_string(),
        };
        inputs.report(titled);
        if let Some(state) = fixture_row.state {
            inputs.agent(&terminal, index, &directory_text, state);
        }
    }
}

const fn entry(minutes_ago: u64, tool: &'static str, text: &'static str) -> FixtureEntry {
    FixtureEntry {
        minutes_ago,
        tool,
        text,
    }
}

fn session_at(index: usize) -> String {
    format!("00000000-0000-4000-8000-{index:012}")
}

fn agent_log(index: usize) -> AgentLog {
    AgentLog {
        terminal: terminal_at(index),
        session_id: session_at(index),
    }
}

fn tool_input(tool: &str, text: &str) -> Value {
    let mut fields = TOOL_INPUTS.iter();
    let field = fields.find(|(name, _)| *name == tool);
    let mut input = Map::new();
    if let Some((_, key)) = field {
        input.insert(key.to_string(), Value::from(text));
    }
    Value::Object(input)
}

fn pull_request_link(number: u64) -> String {
    format!("{PULL_REQUESTS}{number}")
}

fn message_prompt(from: &Peer, text: &str) -> String {
    let mut attributes = String::new();
    if let Some(address) = &from.address {
        let attribute = format!(" from=\"{address}\"");
        attributes.push_str(&attribute);
    }
    if let Some(name) = &from.name {
        let attribute = format!(" from-name=\"{name}\"");
        attributes.push_str(&attribute);
    }
    format!("{MESSAGE_OPEN}{attributes}>\n{text}\n{MESSAGE_CLOSE}")
}

fn short_log(inputs: &mut Inputs, log: &AgentLog, entries: &[FixtureEntry]) {
    for entry in entries {
        if entry.tool == ASKED {
            inputs.asked(log, entry.minutes_ago, entry.text);
            continue;
        }
        inputs.step(log, entry.minutes_ago, entry.tool, entry.text);
    }
}

fn named(name: &str) -> Peer {
    Peer {
        name: Some(name.to_string()),
        address: None,
    }
}

fn addressed(address: &str) -> Peer {
    Peer {
        name: None,
        address: Some(address.to_string()),
    }
}

/// A turn left with no note, then three sessions of plugin-written work: a plan, a nudged fix, a review round.
fn full_story(inputs: &mut Inputs, log: &AgentLog, retries: usize) {
    inputs.asked(log, 140, "find why test_flaky_retry fails on CI");
    inputs.step(log, 139, "Glob", "tests/**/test_retry*.py");
    inputs.step(log, 138, "Read", "tests/sentry/test_retry.py");
    inputs.step(log, 130, "Edit", "tests/sentry/test_retry.py");
    inputs.session_started(log, 96, STARTUP);
    inputs.asked(
        log,
        95,
        "Look into what closing a terminal sends, and write a plan. No code.",
    );
    inputs.step(log, 94, "Read", "crates/runtime/src/pty.rs");
    inputs.step(log, 93, "Read", "crates/runtime/src/program.rs");
    inputs.step(log, 92, "Grep", "SIGHUP");
    inputs.step(log, 92, "Grep", "kill(");
    inputs.step(log, 91, "Grep", "fn shutdown");
    inputs.wrote(log, 88, PLAN_PATH);
    inputs.share_text(
        log,
        87,
        "Why closing a terminal leaves its shell running",
        FINDING_BODY,
    );
    inputs.note(log, 87, PLAN_NOTE);
    inputs.asked(log, 70, "Quote hook script paths safely.");
    inputs.step(log, 69, "Read", "crates/services/src/agent_hooks.rs");
    inputs.step(log, 66, "Edit", "crates/services/src/agent_hooks.rs");
    for _ in 0..retries {
        inputs.step(log, 65, "Bash", "cargo test -p taco-services");
        inputs.step(log, 65, "Edit", "crates/services/src/agent_hooks.rs");
    }
    inputs.step(log, 64, "Bash", "cargo test -p taco-services");
    inputs.asked_for_note(log, 62);
    inputs.note(log, 61, NUDGED_NOTE);
    inputs.session_started(log, 50, COMPACT);
    let reviewer = named(REVIEWER);
    inputs.heard(log, 49, &reviewer, REVIEW_MESSAGE);
    inputs.step(log, 48, "Read", "crates/terminal-view/src/shapes.rs");
    let reviewed = pull_request_link(155);
    inputs.share(
        log,
        44,
        "pull_request",
        "#155 Draw block and box characters as shapes",
        &reviewed,
    );
    inputs.step(log, 44, "Edit", "crates/terminal-view/src/shapes.rs");
    inputs.failed_step(
        log,
        43,
        "Bash",
        "cargo test -p taco-terminal-view",
        "Exit code 101\ntest shapes::faint_lines_meet ... FAILED",
    );
    inputs.step(log, 42, "Bash", "cargo test -p taco-terminal-view");
    inputs.sent(log, 40, REVIEWER, FIXED_MESSAGE);
    let logo = checkout_file(CHECKOUT_IMAGE);
    inputs.share(log, 38, "image", "The logo before and after", &logo);
    inputs.asked(log, 30, "Now rebase it onto main.");
    inputs.step(log, 29, "Bash", "git fetch origin");
    inputs.failed_step(
        log,
        28,
        "Bash",
        "git rebase origin/main",
        "Exit code 1\nCONFLICT (content): Merge conflict in crates/terminal-view/src/draw.rs",
    );
    inputs.now_line(log, 25, NOW_LINE);
    inputs.step(log, 20, "Read", "crates/terminal-view/src/draw.rs");
    inputs.share(
        log,
        20,
        "artifact",
        "What was approved for Agent Activity",
        ARTIFACT_LINK,
    );
    inputs.step(log, 15, "Edit", "crates/terminal-view/src/draw.rs");
    inputs.pull_request_seen(log, 8, "gh pr ready 179", 179, "ready");
    let heads_up_from = addressed(ADDRESS);
    inputs.heard_mid_turn(log, 5, &heads_up_from, HEADS_UP);
    inputs.step(log, 3, "Bash", "git rebase --continue");
}

fn checkout_file(relative: &str) -> String {
    let joined = Path::new(CHECKOUT).join(relative);
    let resolved = fs::canonicalize(&joined);
    let path = match resolved {
        Ok(resolved) => resolved,
        Err(_) => joined,
    };
    path.to_string_lossy().into_owned()
}

fn mixed(drawn: DrawnState) -> Shown {
    let notices = Notices {
        config_problems: vec!["Could not read settings.json: line 4.".to_string()],
        update: Some("0.27.0".to_string()),
        ..Notices::default()
    };
    let screen = PLACEHOLDER_SCREEN.iter().map(|line| line.to_string());
    Shown {
        drawn,
        notices,
        placeholder_screen: screen.collect(),
        setups: offered_setup(),
        bindings: default_bindings(),
        default_bindings: default_bindings(),
        unread_keybindings: false,
        settings: Settings::default(),
        unread: UnreadSettings::None,
    }
}

fn quiet(drawn: DrawnState) -> Shown {
    let mut shown = mixed(drawn);
    shown.notices = Notices::default();
    shown
}

fn broken_keys(drawn: DrawnState) -> Shown {
    let mut shown = quiet(drawn);
    shown.notices.config_problems = vec![KEYBINDINGS_PROBLEM.to_string()];
    shown.unread_keybindings = true;
    shown
}

fn switches(drawn: DrawnState) -> Shown {
    let mut shown = quiet(drawn);
    shown.settings.restore_on_launch = false;
    shown
}

fn paths(drawn: DrawnState) -> Shown {
    let mut shown = quiet(drawn);
    shown.settings.shell_row_name = ShellRowName::Path;
    shown
}

fn broken(drawn: DrawnState) -> Shown {
    let mut shown = quiet(drawn);
    let font_size = SettingChange::FontSize(Settings::default().font_size);
    shown.notices.config_problems = vec![FONT_SIZE_PROBLEM.to_string()];
    shown.unread = UnreadSettings::Keys(vec![font_size.key()]);
    shown
}

fn empty(drawn: DrawnState) -> Shown {
    Shown {
        drawn,
        notices: Notices::default(),
        placeholder_screen: Vec::new(),
        setups: AgentSetupView::default(),
        bindings: default_bindings(),
        default_bindings: default_bindings(),
        unread_keybindings: false,
        settings: Settings::default(),
        unread: UnreadSettings::None,
    }
}

fn default_bindings() -> Vec<Binding> {
    let no_files = ConfigTexts {
        settings: None,
        keybindings: None,
    };
    let config = resolve_config(&no_files, false);
    config.bindings
}

fn offer_kind() -> AgentSetupNoticeKind {
    AgentSetupNoticeKind::Offer {
        action: AgentMenuAction::SetUp,
    }
}

fn too_old_kind() -> AgentSetupNoticeKind {
    let reason = too_old_reason(PRODUCT_NAME, OLD_VERSION);
    AgentSetupNoticeKind::Unavailable { reason }
}

fn one_notice(kind: AgentSetupNoticeKind, text: &str, terminal: &str) -> AgentSetupView {
    let notice = AgentSetupNotice {
        harness: HarnessId(CLAUDE.to_string()),
        kind,
        text: text.to_string(),
        detail: None,
        terminals: vec![TerminalId(terminal.to_string())],
    };
    AgentSetupView {
        menus: Vec::new(),
        notices: vec![notice],
    }
}

// The services send the offer's text in both states; a too-old Claude Code's reason rides in the kind.
fn setup_on_shell(drawn: DrawnState, kind: AgentSetupNoticeKind) -> Shown {
    let mut shown = quiet(drawn);
    let terminal = format!("t{PLAIN_SHELL_ROW}");
    shown.setups = one_notice(kind, &offer_text(PRODUCT_NAME), &terminal);
    shown
}

fn setup_beside_agent(drawn: DrawnState, kind: AgentSetupNoticeKind, text: &str) -> Shown {
    let mut shown = quiet(drawn);
    shown.setups = one_notice(kind, text, "t0");
    shown
}

fn done_beside_agent(drawn: DrawnState, changed_file: Option<&str>) -> Shown {
    let done = done_text(PRODUCT_NAME);
    let mut shown = setup_beside_agent(drawn, AgentSetupNoticeKind::Done, &done);
    let detail = changed_file.map(changed_file_text);
    for notice in &mut shown.setups.notices {
        notice.detail = detail.clone();
    }
    shown
}

fn plugin_dirs_offer_text() -> String {
    plugin_dirs_hidden_text(PRODUCT_NAME, SETTINGS_FILE)
}

fn offered_setup() -> AgentSetupView {
    let notice = AgentSetupNotice {
        harness: HarnessId(CLAUDE.to_string()),
        kind: AgentSetupNoticeKind::Offer {
            action: AgentMenuAction::SetUp,
        },
        text: offer_text(PRODUCT_NAME),
        detail: None,
        terminals: vec![TerminalId("t4".to_string())],
    };
    AgentSetupView {
        menus: Vec::new(),
        notices: vec![notice],
    }
}
