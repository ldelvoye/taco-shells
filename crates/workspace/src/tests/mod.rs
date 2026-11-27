use taco_agents::{NOW_TOOL, SHARE_TOOL, activity_tool_prefix};
use taco_protocol::{
    ActivityEntry, ActivityEvent, ActivityKind, ActivityRequest, Author, HarnessId, HookCall,
    HookEvent, Peer, RuntimeCommand, RuntimeReport, TerminalId, TerminalStart, WorkspaceRequest,
};

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::{Options, Outbound, Storage, Workspace};

mod activity;
mod save;
mod terminals;

const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";

/// A save file in memory, shared with the test so it can read what was written and count the writes.
#[derive(Clone, Default)]
struct Disk {
    file: Arc<Mutex<Option<String>>>,
    writes: Arc<Mutex<usize>>,
    failing: Arc<Mutex<bool>>,
    logs: Arc<Mutex<BTreeMap<TerminalId, Vec<String>>>>,
    states: Arc<Mutex<BTreeMap<TerminalId, String>>>,
}

impl Disk {
    fn holding(text: &str) -> Disk {
        let disk = Disk::default();
        *disk.file.lock().unwrap() = Some(text.to_string());
        disk
    }

    fn text(&self) -> Option<String> {
        self.file.lock().unwrap().clone()
    }

    fn writes(&self) -> usize {
        *self.writes.lock().unwrap()
    }

    fn fail(&self, failing: bool) {
        *self.failing.lock().unwrap() = failing;
    }
}

impl Storage for Disk {
    fn read_save(&self) -> Option<String> {
        self.text()
    }

    fn write_save(&mut self, text: &str) -> bool {
        if *self.failing.lock().unwrap() {
            return false;
        }
        *self.file.lock().unwrap() = Some(text.to_string());
        *self.writes.lock().unwrap() += 1;
        true
    }

    fn read_log(&self, terminal: &TerminalId) -> Vec<String> {
        let logs = self.logs.lock().unwrap();
        match logs.get(terminal) {
            Some(lines) => lines.clone(),
            None => Vec::new(),
        }
    }

    fn append_log(&mut self, terminal: &TerminalId, line: &str) -> bool {
        if *self.failing.lock().unwrap() {
            return false;
        }
        let mut logs = self.logs.lock().unwrap();
        let lines = logs.entry(terminal.clone()).or_default();
        lines.push(line.to_string());
        true
    }

    fn rewrite_log(&mut self, terminal: &TerminalId, lines: &[String]) -> bool {
        if *self.failing.lock().unwrap() {
            return false;
        }
        let mut logs = self.logs.lock().unwrap();
        logs.insert(terminal.clone(), lines.to_vec());
        true
    }

    fn delete_log(&mut self, terminal: &TerminalId) {
        self.logs.lock().unwrap().remove(terminal);
    }

    fn read_state(&self, terminal: &TerminalId) -> Option<String> {
        let states = self.states.lock().unwrap();
        states.get(terminal).cloned()
    }

    fn write_state(&mut self, terminal: &TerminalId, text: &str) -> bool {
        if *self.failing.lock().unwrap() {
            return false;
        }
        let mut states = self.states.lock().unwrap();
        states.insert(terminal.clone(), text.to_string());
        true
    }

    fn delete_state(&mut self, terminal: &TerminalId) {
        self.states.lock().unwrap().remove(terminal);
    }

    fn activity_terminals(&self) -> Vec<TerminalId> {
        let logs = self.logs.lock().unwrap();
        let states = self.states.lock().unwrap();
        let mut terminals: Vec<TerminalId> = logs.keys().cloned().collect();
        for terminal in states.keys() {
            if !terminals.contains(terminal) {
                terminals.push(terminal.clone());
            }
        }
        terminals
    }
}

fn id(name: &str) -> TerminalId {
    TerminalId(name.to_string())
}

fn claude() -> HarnessId {
    HarnessId("claude".into())
}

fn options_on(disk: &Disk, first_id: usize) -> Options {
    let mut next = first_id;
    Options {
        settings_path: "/config/it's/settings.json".into(),
        keybindings_path: "/config/keybindings.json".into(),
        new_terminal_id: Box::new(move || {
            next += 1;
            id(&format!("t{next}"))
        }),
        storage: Box::new(disk.clone()),
        restore_on_launch: true,
    }
}

fn workspace() -> Workspace {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    workspace
}

fn launched(disk: &Disk) -> (Workspace, Vec<TerminalStart>) {
    let mut workspace = Workspace::new(options_on(disk, 0));
    let out = workspace.start(0);
    (workspace, opened(&out))
}

fn opened(out: &[Outbound]) -> Vec<TerminalStart> {
    let starts = out.iter().filter_map(|outbound| match outbound {
        Outbound::Runtime(RuntimeCommand::Open(start)) => Some(start.clone()),
        _ => None,
    });
    starts.collect()
}

/// The rows as names in order, the selected one in brackets.
fn listed(workspace: &Workspace) -> String {
    let drawn = workspace.drawn();
    let Some(arrangement) = drawn.arrangement else {
        return "none".into();
    };
    let names = arrangement.rows.iter().enumerate().map(|(index, row)| {
        let name = row.terminal.0.clone();
        if index == arrangement.selected {
            return format!("[{name}]");
        }
        name
    });
    let listed: Vec<String> = names.collect();
    listed.join(" ")
}

fn ask(workspace: &mut Workspace, request: WorkspaceRequest) -> Vec<Outbound> {
    let (_, out) = workspace.request(request, 0);
    out
}

fn hook(workspace: &mut Workspace, terminal: &str, call: HookCall, now_ms: u64) -> Vec<Outbound> {
    let event = HookEvent {
        terminal: id(terminal),
        call,
    };
    workspace.hook(event, now_ms)
}

fn agent_starts(workspace: &mut Workspace, terminal: &str) {
    let call = HookCall::AgentStart {
        harness: claude(),
        session_id: SESSION.into(),
        cwd: "/work/tacos".into(),
        has_conversation: true,
    };
    hook(workspace, terminal, call, 0);
}

fn session_starts(workspace: &mut Workspace, terminal: &str, session_id: &str) -> Vec<Outbound> {
    let call = HookCall::AgentStart {
        harness: claude(),
        session_id: session_id.into(),
        cwd: "/work/tacos".into(),
        has_conversation: false,
    };
    hook(workspace, terminal, call, 0)
}

fn titled(workspace: &mut Workspace, terminal: &str, title: &str) {
    let report = RuntimeReport::Titled {
        id: id(terminal),
        title: title.into(),
    };
    workspace.runtime_report(report, 0);
}

fn exited(workspace: &mut Workspace, terminal: &str) -> Vec<Outbound> {
    let report = RuntimeReport::Exited {
        id: id(terminal),
        code: 0,
    };
    workspace.runtime_report(report, 0)
}

fn prompted(
    workspace: &mut Workspace,
    terminal: &str,
    session_id: &str,
    turn: Option<&str>,
    prompt: &str,
) -> Vec<Outbound> {
    let mut payload = serde_json::json!({ "prompt": prompt });
    if let Some(turn) = turn {
        payload["prompt_id"] = serde_json::json!(turn);
    }
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: session_id.into(),
        event: "UserPromptSubmit".into(),
        payload: payload.to_string(),
    };
    hook(workspace, terminal, call, 0)
}

fn now_set(workspace: &mut Workspace, terminal: &str, text: &str) -> Vec<Outbound> {
    let tool = format!("{}{NOW_TOOL}", activity_tool_prefix());
    let payload = serde_json::json!({ "tool_name": tool, "tool_input": { "text": text } });
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "PostToolUse".into(),
        payload: payload.to_string(),
    };
    hook(workspace, terminal, call, 0)
}

fn shared_by_agent(workspace: &mut Workspace, input: serde_json::Value) -> Vec<Outbound> {
    let tool = format!("{}{SHARE_TOOL}", activity_tool_prefix());
    let payload = serde_json::json!({ "tool_name": tool, "tool_input": input, "cwd": "/work" });
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "PostToolUse".into(),
        payload: payload.to_string(),
    };
    hook(workspace, "t1", call, 0)
}

fn activity(out: &[Outbound]) -> Vec<ActivityEvent> {
    let events = out.iter().filter_map(|outbound| match outbound {
        Outbound::Activity(event) => Some(event.clone()),
        _ => None,
    });
    events.collect()
}

fn loaded_entries(workspace: &mut Workspace, terminal: &str) -> Vec<ActivityEntry> {
    let watch = ActivityRequest::Watch {
        terminal: Some(id(terminal)),
    };
    let out = workspace.activity(watch);
    let Some(ActivityEvent::Loaded { entries, .. }) = activity(&out).pop() else {
        panic!("watching {terminal} loads its log");
    };
    entries
}

pub(crate) fn stretch_line(entry: ActivityEntry) -> String {
    match entry.kind {
        ActivityKind::Step { stretch, .. } => format!("step {} in {stretch}", entry.seq),
        ActivityKind::Note {
            stretch: Some(stretch),
            ..
        } => format!("note in {stretch}"),
        ActivityKind::Note { stretch: None, .. } => "note alone".to_string(),
        ActivityKind::Asked { .. } => "asked".to_string(),
        ActivityKind::MessageIn { .. } => match entry.author {
            Author::OtherAgent(Peer {
                name: Some(name), ..
            }) => format!("message from {name}"),
            _ => "message from someone".to_string(),
        },
        ActivityKind::MessageOut { stretch, .. } => format!("sent {} in {stretch}", entry.seq),
        other => format!("{other:?}"),
    }
}

/// The texts a terminal's log shows when the window watches it, oldest first.
fn logged(workspace: &mut Workspace, terminal: &str) -> Vec<String> {
    let entries = loaded_entries(workspace, terminal);
    let texts = entries.into_iter().map(|entry| match entry.kind {
        ActivityKind::Asked { text } => text,
        other => format!("{other:?}"),
    });
    texts.collect()
}

/// The now line each board event carries, in order.
fn now_lines(out: &[Outbound]) -> Vec<Option<String>> {
    let boards = activity(out).into_iter().filter_map(|event| match event {
        ActivityEvent::BoardChanged { board, .. } => Some(board.now),
        ActivityEvent::Loaded { board, .. } => Some(board.now),
        _ => None,
    });
    boards.collect()
}

fn watched_now(workspace: &mut Workspace, terminal: &str) -> Option<String> {
    let watch = ActivityRequest::Watch {
        terminal: Some(id(terminal)),
    };
    let out = workspace.activity(watch);
    let mut lines = now_lines(&out);
    lines.pop().flatten()
}

/// Each thing in the board events, as "title (kind)", newest first, one list per event.
fn shared_lists(out: &[Outbound]) -> Vec<Vec<String>> {
    let boards = activity(out).into_iter().filter_map(|event| match event {
        ActivityEvent::BoardChanged { board, .. } | ActivityEvent::Loaded { board, .. } => {
            let things = board
                .shared
                .iter()
                .map(|thing| format!("{} ({})", thing.title, thing.kind));
            Some(things.collect())
        }
        _ => None,
    });
    boards.collect()
}
