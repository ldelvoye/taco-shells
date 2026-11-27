use taco_protocol::{
    ActivityEvent, ActivityKind, ActivityRequest, AgentSignal, Author, ConfigFile, DrawnState,
    HarnessId, HookCall, HookEvent, Peer, RowState, RuntimeCommand, RuntimeReport, SharedTarget,
    TabGroupId, TerminalId, TerminalStart, WorkspaceRequest,
};

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::sessions::PROVISIONAL_MS;
use crate::{Options, Outbound, Storage, Workspace};

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

fn opened(out: &[Outbound]) -> Vec<TerminalStart> {
    let starts = out.iter().filter_map(|outbound| match outbound {
        Outbound::Runtime(RuntimeCommand::Open(start)) => Some(start.clone()),
        _ => None,
    });
    starts.collect()
}

fn published(out: &[Outbound]) -> Option<DrawnState> {
    let mut states = out.iter().filter_map(|outbound| match outbound {
        Outbound::State(state) => Some(state.clone()),
        _ => None,
    });
    states.next_back()
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

fn agent_signals(workspace: &mut Workspace, terminal: &str, signal: AgentSignal, now_ms: u64) {
    let call = HookCall::AgentState {
        harness: claude(),
        session_id: SESSION.into(),
        signal,
    };
    hook(workspace, terminal, call, now_ms);
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

fn row_state(workspace: &Workspace, terminal: &str) -> Option<(RowState, Option<String>)> {
    let drawn = workspace.drawn();
    let row = drawn.agent_rows.get(&id(terminal))?;
    Some((row.state, row.question.clone()))
}

#[test]
fn a_terminal_opens_under_a_minted_id_and_a_config_file_opens_in_the_shells_editor() {
    let mut first = Workspace::new(Options {
        new_terminal_id: Box::new(|| id("only")),
        ..options_on(&Disk::default(), 0)
    });
    let out = first.start(0);
    assert_eq!(opened(&out)[0].id, id("only"));
    assert_eq!(listed(&first), "[only]");
    assert_eq!(published(&out).map(|state| state.revision), Some(1));

    let mut workspace = workspace();
    let out = ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal {
            file: Some(ConfigFile::Settings),
        },
    );
    let command = opened(&out)[0].command.clone();
    assert_eq!(
        command.as_deref(),
        Some("${EDITOR:-vi} '/config/it'\\''s/settings.json'\n")
    );

    let (reply, _) = workspace.request(WorkspaceRequest::TabGroupCreated { terminal: id("t1") }, 0);
    assert_eq!(reply.tab_group, Some(TabGroupId("tab-group-1".into())));
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminalIn {
            tab_group: reply.tab_group,
        },
    );
    assert_eq!(listed(&workspace), "t1 [t3] t2");
    let (second, _) =
        workspace.request(WorkspaceRequest::TabGroupCreated { terminal: id("t2") }, 0);
    assert_eq!(second.tab_group, Some(TabGroupId("tab-group-2".into())));
}

#[test]
fn a_fork_opens_under_its_source_with_the_agents_fork_line_and_nothing_else_is_forked() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
    agent_starts(&mut workspace, "t1");
    titled(&mut workspace, "t1", "✳ Tacos");

    let out = ask(
        &mut workspace,
        WorkspaceRequest::ForkTerminal { terminal: id("t1") },
    );
    let fork = &opened(&out)[0];
    assert_eq!(listed(&workspace), "t1 [t3] t2");
    assert_eq!(fork.directory.as_deref(), Some("/work/tacos"));
    let expected = format!("claude --resume {SESSION} --fork-session --name '⑂ Tacos'\n");
    assert_eq!(fork.command.as_deref(), Some(expected.as_str()));

    // a fork that cannot be one is refused, publishes nothing and asks the runtime for nothing
    let refused = |workspace: &mut Workspace, terminal: &str| {
        let fork = WorkspaceRequest::ForkTerminal {
            terminal: id(terminal),
        };
        let (reply, out) = workspace.request(fork, 0);
        !reply.ok && out.is_empty()
    };
    assert!(refused(&mut workspace, "t2"), "a plain shell");
    let fresh = HookCall::AgentStart {
        harness: claude(),
        session_id: SESSION.into(),
        cwd: "/work".into(),
        has_conversation: false,
    };
    hook(&mut workspace, "t2", fresh, 0);
    assert!(
        refused(&mut workspace, "t2"),
        "a session with no conversation yet"
    );
    let ends = HookCall::AgentEnd {
        harness: claude(),
        session_id: SESSION.into(),
    };
    hook(&mut workspace, "t1", ends, 0);
    assert!(refused(&mut workspace, "t1"), "a session that has ended");
    assert!(
        refused(&mut workspace, "gone"),
        "a terminal that is not open"
    );
}

#[test]
fn a_close_goes_through_the_runtime_and_the_last_exit_leaves_an_empty_list() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    let out = ask(
        &mut workspace,
        WorkspaceRequest::CloseTerminal { terminal: id("t1") },
    );
    assert_eq!(
        out,
        vec![Outbound::Runtime(RuntimeCommand::Close { id: id("t1") })]
    );
    assert_eq!(listed(&workspace), "t1 [t2]");
    agent_starts(&mut workspace, "t1");
    assert!(
        workspace.drawn().started.is_empty(),
        "opened, but no shell reported yet"
    );
    let started = RuntimeReport::Started {
        id: id("t1"),
        directory: "/work".into(),
    };
    let out = workspace.runtime_report(started, 0);
    let published = published(&out).unwrap();
    assert_eq!(
        published.started,
        [id("t1")].into(),
        "the window may attach to it now"
    );
    exited(&mut workspace, "t1");
    assert_eq!(listed(&workspace), "[t2]");
    let drawn = workspace.drawn();
    assert!(!drawn.agent_rows.contains_key(&id("t1")));
    assert!(!drawn.directories.contains_key(&id("t1")));
    assert!(drawn.started.is_empty());

    // reports for a terminal that is not open change nothing
    let stray = RuntimeReport::DirectoryChanged {
        id: id("t1"),
        directory: "/elsewhere".into(),
    };
    assert!(workspace.runtime_report(stray, 0).is_empty());
    hook(&mut workspace, "t1", HookCall::Attention, 0);
    assert!(workspace.app_activated(0).is_empty());
    let out = ask(
        &mut workspace,
        WorkspaceRequest::CloseTerminal { terminal: id("t1") },
    );
    assert!(out.is_empty());

    // the last terminal going is an ordinary change: the empty list is published, and nothing else goes out
    let out = exited(&mut workspace, "t2");
    let only_the_state = matches!(out.as_slice(), [Outbound::State(_)]);
    assert!(only_the_state, "{out:?}");
    assert_eq!(listed(&workspace), "");

    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
    assert_eq!(listed(&workspace), "[t3]");
}

#[test]
fn when_the_runtime_dies_every_terminal_goes_the_window_says_why_and_nothing_reopens() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    let out = workspace.runtime_died("killed", 0);
    let drawn = published(&out).unwrap();
    assert!(drawn.problem.unwrap().contains("(killed)"));
    assert_eq!(listed(&workspace), "");

    let (reply, out) = workspace.request(WorkspaceRequest::OpenTerminal { file: None }, 0);
    assert!(!reply.ok);
    assert!(opened(&out).is_empty());
}

#[test]
fn attention_waits_for_the_app_to_come_back() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    hook(&mut workspace, "t2", HookCall::Attention, 0);
    let out = workspace.app_activated(0);
    assert!(
        out.is_empty(),
        "asking from the selected terminal goes nowhere"
    );

    hook(&mut workspace, "t1", HookCall::Attention, 0);
    hook(&mut workspace, "t2", HookCall::Attention, 0);
    let refocus = workspace.drawn().refocus;
    workspace.app_activated(0);
    assert_eq!(listed(&workspace), "[t1] t2");
    assert_eq!(workspace.drawn().refocus, refocus + 1);
    assert!(
        workspace.app_activated(0).is_empty(),
        "arriving clears the wait"
    );
}

#[test]
fn the_windows_focus_count_is_published_and_a_request_that_changes_nothing_publishes_nothing() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    let report = WorkspaceRequest::FocusReported {
        terminal: id("t2"),
        count: 3,
    };
    let out = ask(&mut workspace, report.clone());
    assert_eq!(published(&out).map(|state| state.focus_count), Some(3));
    assert!(ask(&mut workspace, report).is_empty());

    let (reply, out) = workspace.request(
        WorkspaceRequest::FocusReported {
            terminal: id("t1"),
            count: 4,
        },
        0,
    );
    assert_eq!(reply.revision, published(&out).unwrap().revision);
    assert_eq!(listed(&workspace), "[t1] t2");

    let late = WorkspaceRequest::FocusReported {
        terminal: id("t2"),
        count: 2,
    };
    ask(&mut workspace, late);
    assert_eq!(
        workspace.drawn().focus_count,
        4,
        "a late report never lowers the count"
    );
}

#[test]
fn a_title_is_read_by_the_rows_agent_and_read_again_once_the_agent_starts() {
    let mut workspace = workspace();
    titled(&mut workspace, "t1", "✳ Tacos");
    let title = |workspace: &Workspace| {
        let arrangement = workspace.drawn().arrangement.unwrap();
        arrangement.rows[0].title.clone()
    };
    assert_eq!(title(&workspace), "✳ Tacos");

    agent_starts(&mut workspace, "t1");
    assert_eq!(title(&workspace), "Tacos");

    agent_signals(&mut workspace, "t1", AgentSignal::Working, 1);
    titled(&mut workspace, "t1", "✳ Tacos");
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Idle, None)));
}

#[test]
fn a_question_shows_only_while_its_row_needs_you_and_a_stop_off_screen_is_done() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
    agent_starts(&mut workspace, "t1");
    let payload = r#"{"notification_type":"permission_prompt","message":"Allow Bash?"}"#;
    let asks = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "Notification".into(),
        payload: payload.into(),
    };

    hook(&mut workspace, "t1", asks.clone(), 1);
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Idle, None)));
    agent_signals(&mut workspace, "t1", AgentSignal::Needs, 2);
    assert_eq!(
        row_state(&workspace, "t1"),
        Some((RowState::Needs, Some("Allow Bash?".into())))
    );
    agent_signals(&mut workspace, "t1", AgentSignal::Working, 3);
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Working, None)));

    agent_signals(&mut workspace, "t1", AgentSignal::Stopped, 4);
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Done, None)));
    ask(
        &mut workspace,
        WorkspaceRequest::FocusReported {
            terminal: id("t1"),
            count: 1,
        },
    );
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Idle, None)));
}

const ELECTRON_FILE: &str = include_str!("../tests/fixtures/electron-workspace.json");

fn launched(disk: &Disk) -> (Workspace, Vec<TerminalStart>) {
    let mut workspace = Workspace::new(options_on(disk, 0));
    let out = workspace.start(0);
    (workspace, opened(&out))
}

#[test]
fn an_electron_save_reopens_each_pane_as_a_row_where_it_was_and_resumes_its_agent() {
    let disk = Disk::holding(ELECTRON_FILE);
    let (workspace, starts) = launched(&disk);

    assert_eq!(listed(&workspace), "t1 [t2] t3 t4");
    let resume = |session: &str| format!("claude --resume {session}\n");
    let shells: Vec<(Option<String>, Option<String>)> = starts
        .iter()
        .map(|start| (start.directory.clone(), start.command.clone()))
        .collect();
    assert_eq!(
        shells,
        vec![
            (Some("/Users/me/code/tacos".into()), Some(resume(SESSION))),
            (Some("/Users/me/code/tacos/docs".into()), None),
            (
                Some("/Users/me/notes".into()),
                Some(resume("223e4567-e89b-12d3-a456-426614174000"))
            ),
            (None, None),
        ]
    );
    // known before any shell has started, so the first save keeps them
    let drawn = workspace.drawn();
    assert_eq!(
        drawn.directories.get(&id("t2")).map(String::as_str),
        Some("/Users/me/code/tacos/docs")
    );
    assert!(
        drawn.started.is_empty(),
        "a restored row has a directory before its shell starts"
    );
}

#[test]
fn a_save_brings_back_the_same_rows_ids_groups_and_panels_and_waits_out_its_debounce() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
    workspace.request(
        WorkspaceRequest::TabGroupCreated { terminal: id("t2") },
        100,
    );
    let toggled = WorkspaceRequest::TabGroupToggled {
        id: TabGroupId("tab-group-1".into()),
    };
    workspace.request(toggled, 200);
    let moved = WorkspaceRequest::PanelMoved {
        panel: taco_protocol::Panel::AgentActivity,
        edge: taco_protocol::Edge::Bottom,
    };
    workspace.request(moved, 300);

    workspace.tick(549);
    assert_eq!(disk.writes(), 0, "each change pushes the write back");
    assert_eq!(workspace.next_deadline(), Some(550));
    workspace.tick(550);
    assert_eq!(disk.writes(), 1);
    // a title is not saved, so it neither writes nor pushes a write back
    titled(&mut workspace, "t1", "vim");
    assert_eq!(workspace.next_deadline(), None);
    workspace.tick(10_000);
    workspace.flush(10_000);
    assert_eq!(disk.writes(), 1);

    // a failed write is tried again, and a steady stream of changes still saves within two seconds
    disk.fail(true);
    let resize = |size: f32| WorkspaceRequest::PanelResized {
        panel: taco_protocol::Panel::Terminals,
        size,
    };
    workspace.request(resize(200.0), 20_000);
    workspace.tick(20_250);
    disk.fail(false);
    workspace.tick(21_250);
    assert_eq!(disk.writes(), 2);
    // a change every 200 ms for three seconds, each pushing the debounce back
    for step in 1..=15 {
        let now = 30_000 + step * 200;
        workspace.request(resize(200.0 + step as f32), now);
        workspace.tick(now);
    }
    assert_eq!(disk.writes(), 3, "written once by the two-second limit");

    let (reopened, starts) = launched(&disk);
    let ids: Vec<TerminalId> = starts.iter().map(|start| start.id.clone()).collect();
    assert_eq!(ids, vec![id("t1"), id("t2")]);
    assert_eq!(listed(&reopened), "[t1] t2");
    let drawn = reopened.drawn();
    let arrangement = drawn.arrangement.unwrap();
    assert!(arrangement.tab_groups[0].collapsed);
    assert_eq!(
        drawn.panels.agent_activity.edge,
        taco_protocol::Edge::Bottom
    );
}

#[test]
fn quitting_or_losing_the_runtime_keeps_the_open_terminals_in_the_save() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    workspace.freeze();
    exited(&mut workspace, "t1");
    exited(&mut workspace, "t2");
    workspace.flush(1_000);
    let (reopened, _) = launched(&disk);
    assert_eq!(listed(&reopened), "t1 [t2]");

    let died = Disk::default();
    let mut workspace = Workspace::new(options_on(&died, 0));
    workspace.start(0);
    workspace.runtime_died("killed", 10);
    workspace.flush(1_000);
    let (reopened, _) = launched(&died);
    assert_eq!(listed(&reopened), "[t1]");

    // closing every terminal before a quit saves none, and the next launch opens one fresh
    let emptied = Disk::default();
    let mut workspace = Workspace::new(options_on(&emptied, 0));
    workspace.start(0);
    exited(&mut workspace, "t1");
    workspace.flush(1_000);
    let (reopened, starts) = launched(&emptied);
    assert_eq!((listed(&reopened), starts.len()), ("[t1]".to_string(), 1));
    assert!(emptied.text().unwrap().contains("\"rows\":[]"));
}

#[test]
fn a_restored_agent_that_never_starts_again_is_forgotten_and_one_that_does_is_kept() {
    let disk = Disk::holding(ELECTRON_FILE);
    let (mut workspace, _) = launched(&disk);
    workspace.tick(1_000);
    let first = disk.text().unwrap();
    assert!(
        first.contains("223e4567"),
        "kept while its start may still come"
    );

    agent_starts(&mut workspace, "t1");
    workspace.tick(2_000);
    // nothing else changes: the expiry is a deadline of its own
    let expiry = workspace.next_deadline().unwrap();
    assert_eq!(expiry, PROVISIONAL_MS + 1);
    workspace.tick(expiry);
    let due = workspace.next_deadline().unwrap();
    workspace.tick(due);

    let saved = disk.text().unwrap();
    assert!(saved.contains(SESSION), "confirmed by its start");
    assert!(!saved.contains("223e4567"), "never started again");
}

#[test]
fn a_restore_keeps_ids_apart_and_opens_the_selected_rows_tab_group() {
    let file = r#"{"version":2,"selected":1,"tabGroups":[{"name":"g","collapsed":true}],
        "rows":[{"id":"a","tabGroup":0},{"id":"a","tabGroup":0},{"id":"","tabGroup":0}]}"#;
    let (workspace, _) = launched(&Disk::holding(file));

    assert_eq!(listed(&workspace), "a [t1] t2");
    let arrangement = workspace.drawn().arrangement.unwrap();
    assert!(!arrangement.tab_groups[0].collapsed);
}

#[test]
fn a_file_that_cannot_be_restored_or_a_restore_turned_off_opens_one_fresh_terminal() {
    for text in ["not json", r#"{"version":2,"rows":[]}"#, r#"{"version":9}"#] {
        let (workspace, starts) = launched(&Disk::holding(text));
        assert_eq!(
            (listed(&workspace), starts.len()),
            ("[t1]".to_string(), 1),
            "{text}"
        );
    }
    let disk = Disk::holding(ELECTRON_FILE);
    let mut workspace = Workspace::new(Options {
        restore_on_launch: false,
        ..options_on(&disk, 0)
    });
    let starts = opened(&workspace.start(0));
    assert_eq!(starts.len(), 1);
}

fn prompted(
    workspace: &mut Workspace,
    terminal: &str,
    session_id: &str,
    prompt: &str,
) -> Vec<Outbound> {
    let payload = serde_json::json!({ "prompt": prompt }).to_string();
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: session_id.into(),
        event: "UserPromptSubmit".into(),
        payload,
    };
    hook(workspace, terminal, call, 0)
}

fn activity(out: &[Outbound]) -> Vec<ActivityEvent> {
    let events = out.iter().filter_map(|outbound| match outbound {
        Outbound::Activity(event) => Some(event.clone()),
        _ => None,
    });
    events.collect()
}

/// The texts a terminal's log shows when the window watches it, oldest first.
fn logged(workspace: &mut Workspace, terminal: &str) -> Vec<String> {
    let watch = ActivityRequest::Watch {
        terminal: Some(id(terminal)),
    };
    let out = workspace.activity(watch);
    let Some(ActivityEvent::Loaded { entries, .. }) = activity(&out).pop() else {
        panic!("watching {terminal} loads its log");
    };
    let texts = entries.into_iter().map(|entry| match entry.kind {
        ActivityKind::Asked { text } => text,
        other => format!("{other:?}"),
    });
    texts.collect()
}

#[test]
fn a_log_belongs_to_its_terminal_across_sessions_and_only_the_watched_one_is_streamed() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    prompted(&mut workspace, "t1", SESSION, "first");
    // a /clear, a compaction or a new agent in the same terminal starts another session
    let other_session = "223e4567-e89b-12d3-a456-426614174000";
    prompted(&mut workspace, "t1", other_session, "second");
    assert_eq!(logged(&mut workspace, "t1"), vec!["first", "second"]);

    let streamed = prompted(&mut workspace, "t1", other_session, "third");
    let appended = activity(&streamed);
    assert!(matches!(&appended[..], [ActivityEvent::Appended { entry, .. }] if entry.seq == 2));
    let unwatched = prompted(&mut workspace, "t2", SESSION, "elsewhere");
    assert!(activity(&unwatched).is_empty());
    assert_eq!(logged(&mut workspace, "t2"), vec!["elsewhere"]);

    let nothing = ActivityRequest::Watch { terminal: None };
    assert!(workspace.activity(nothing).is_empty());
    let quiet = prompted(&mut workspace, "t2", SESSION, "fourth");
    assert!(
        activity(&quiet).is_empty(),
        "nothing is streamed once no terminal is watched"
    );
    let not_open = ActivityRequest::Watch {
        terminal: Some(id("gone")),
    };
    assert!(workspace.activity(not_open).is_empty());
}

#[test]
fn a_log_file_stays_small_and_a_failed_write_is_made_good_by_the_next_one() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    let lines = |disk: &Disk| disk.read_log(&id("t1")).len();

    for index in 0..crate::log::REWRITE_AT_LINES {
        prompted(&mut workspace, "t1", SESSION, &index.to_string());
    }
    assert_eq!(
        lines(&disk),
        crate::KEPT_ENTRIES,
        "rewritten down to what it keeps"
    );

    disk.fail(true);
    prompted(&mut workspace, "t1", SESSION, "lost on the disk");
    disk.fail(false);
    prompted(&mut workspace, "t1", SESSION, "after");
    let on_disk = disk.read_log(&id("t1"));
    assert_eq!(on_disk.len(), crate::KEPT_ENTRIES);
    assert!(on_disk[on_disk.len() - 2].contains("lost on the disk"));
}

fn tool_used(workspace: &mut Workspace, terminal: &str, payload: serde_json::Value) {
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "PostToolUse".into(),
        payload: payload.to_string(),
    };
    hook(workspace, terminal, call, 0);
}

fn ran(workspace: &mut Workspace, terminal: &str, command: &str) {
    let payload = serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": command } });
    tool_used(workspace, terminal, payload);
}

fn noted(workspace: &mut Workspace, terminal: &str, text: &str) {
    let tool = "mcp__plugin_taco-shells_activity__note";
    let payload = serde_json::json!({ "tool_name": tool, "tool_input": { "text": text } });
    tool_used(workspace, terminal, payload);
}

fn prompted_in(workspace: &mut Workspace, terminal: &str, turn: &str, prompt: &str) {
    let payload = serde_json::json!({ "prompt": prompt, "prompt_id": turn }).to_string();
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "UserPromptSubmit".into(),
        payload,
    };
    hook(workspace, terminal, call, 0);
}

/// Each entry of a terminal's log as a short line: "step 3 in 2", "note in 2", "note alone", "asked".
fn stretches(workspace: &mut Workspace, terminal: &str) -> Vec<String> {
    let watch = ActivityRequest::Watch {
        terminal: Some(id(terminal)),
    };
    let out = workspace.activity(watch);
    let Some(ActivityEvent::Loaded { entries, .. }) = activity(&out).pop() else {
        panic!("watching {terminal} loads its log");
    };
    let lines = entries.into_iter().map(|entry| match entry.kind {
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
    });
    lines.collect()
}

#[test]
fn messages_are_lines_of_their_own_and_a_subagents_late_step_keeps_to_its_turn() {
    let mut workspace = workspace();
    let step_of = |workspace: &mut Workspace, command: &str, turn: &str, subagent: bool| {
        let mut payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": command },
            "prompt_id": turn,
        });
        if subagent {
            payload["agent_id"] = serde_json::json!("helper-1");
        }
        tool_used(workspace, "t1", payload);
    };
    let messages = r#"<cross-session-message from="uds:/tmp/cc-socks/9873.sock" from-name="code-83">
Look into the logo.
</cross-session-message><cross-session-message from="uds:/tmp/cc-socks/9879.sock" from-name="code-f1">
The route is up.
</cross-session-message>"#;
    prompted_in(&mut workspace, "t1", "turn-a", messages);
    step_of(&mut workspace, "cargo test", "turn-a", false);
    let sent = serde_json::json!({
        "tool_name": "SendMessage",
        "tool_input": { "to": "code-83", "summary": "s", "message": "On it" },
        "tool_response": { "success": true },
        "prompt_id": "turn-a",
    });
    tool_used(&mut workspace, "t1", sent);
    noted(&mut workspace, "t1", "Found it");
    // a background helper finishing after the note
    step_of(&mut workspace, "grep logo", "turn-a", true);
    prompted_in(&mut workspace, "t1", "turn-b", "Next");
    step_of(&mut workspace, "ls", "turn-b", false);
    step_of(&mut workspace, "grep again", "turn-a", true);
    step_of(&mut workspace, "pwd", "turn-b", true);
    // only a subagent keeps to an older turn: the agent's own step joins what is open
    step_of(&mut workspace, "date", "turn-a", false);
    // a message that starts a new turn closes the stretch, as a prompt does
    let reply = r#"<cross-session-message from="uds:/tmp/cc-socks/9873.sock" from-name="code-83">
Merged.
</cross-session-message>"#;
    prompted_in(&mut workspace, "t1", "turn-c", reply);
    step_of(&mut workspace, "git pull", "turn-c", false);

    let expected = [
        "message from code-83",
        "message from code-f1",
        "step 2 in 2",
        "sent 3 in 2",
        "note in 2",
        "step 5 in 2",
        "asked",
        "step 7 in 7",
        "step 8 in 2",
        "step 9 in 7",
        "step 10 in 7",
        "message from code-83",
        "step 12 in 12",
    ];
    assert_eq!(stretches(&mut workspace, "t1"), expected);
}

#[test]
fn a_note_takes_the_steps_since_the_last_one_and_a_new_turn_or_session_leaves_them_unnoted() {
    let mut workspace = workspace();
    let session_starts = |workspace: &mut Workspace, session_id: &str| {
        let call = HookCall::AgentStart {
            harness: claude(),
            session_id: session_id.into(),
            cwd: "/work/tacos".into(),
            has_conversation: false,
        };
        hook(workspace, "t1", call, 0);
    };
    session_starts(&mut workspace, SESSION);
    prompted_in(&mut workspace, "t1", "turn-a", "Find the bug");
    ran(&mut workspace, "t1", "cargo test");
    // a message from another agent mid-turn carries the turn's id and leaves the stretch open
    prompted_in(&mut workspace, "t1", "turn-a", "Also check the logo");
    ran(&mut workspace, "t1", "cargo clippy");
    // a compaction starts the same session again, mid-turn
    session_starts(&mut workspace, SESSION);
    ran(&mut workspace, "t1", "cargo build");
    noted(&mut workspace, "t1", "Found it");
    noted(&mut workspace, "t1", "Nothing more to do");
    ran(&mut workspace, "t1", "git status");
    prompted_in(&mut workspace, "t1", "turn-b", "Next");
    ran(&mut workspace, "t1", "ls");
    session_starts(&mut workspace, "223e4567-e89b-12d3-a456-426614174000");
    ran(&mut workspace, "t1", "pwd");
    // a new session starts its own turns, even one whose id repeats the last session's
    prompted_in(&mut workspace, "t1", "turn-b", "Again");
    ran(&mut workspace, "t1", "date");
    prompted(&mut workspace, "t1", SESSION, "No turn id");
    ran(&mut workspace, "t1", "whoami");
    prompted(&mut workspace, "t1", SESSION, "Still no turn id");
    ran(&mut workspace, "t1", "uptime");

    let expected = [
        "asked",
        "step 1 in 1",
        "asked",
        "step 3 in 1",
        "step 4 in 1",
        "note in 1",
        "note alone",
        "step 7 in 7",
        "asked",
        "step 9 in 9",
        "step 10 in 10",
        "asked",
        "step 12 in 12",
        "asked",
        "step 14 in 14",
        "asked",
        "step 16 in 16",
    ];
    assert_eq!(stretches(&mut workspace, "t1"), expected);
}

#[test]
fn quitting_with_terminals_open_keeps_their_logs_and_closing_a_terminal_deletes_its_own() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
    prompted(&mut workspace, "t1", SESSION, "kept through a quit");
    prompted(&mut workspace, "t2", SESSION, "kept too");

    // the quit: frozen first, then every shell goes as the runtime stops
    workspace.freeze();
    exited(&mut workspace, "t1");
    exited(&mut workspace, "t2");
    workspace.flush(1_000);

    let (mut reopened, _) = launched(&disk);
    assert_eq!(logged(&mut reopened, "t1"), vec!["kept through a quit"]);
    assert_eq!(logged(&mut reopened, "t2"), vec!["kept too"]);

    exited(&mut reopened, "t2");
    assert!(
        !disk.activity_terminals().contains(&id("t2")),
        "a terminal that closes takes its log with it"
    );
    assert!(disk.activity_terminals().contains(&id("t1")));

    reopened.runtime_died("killed", 2_000);
    reopened.flush(3_000);
    let (mut after_crash, _) = launched(&disk);
    assert_eq!(logged(&mut after_crash, "t1"), vec!["kept through a quit"]);
}

fn now_set(workspace: &mut Workspace, terminal: &str, text: &str) -> Vec<Outbound> {
    let tool = "mcp__plugin_taco-shells_activity__now";
    let payload = serde_json::json!({ "tool_name": tool, "tool_input": { "text": text } });
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "PostToolUse".into(),
        payload: payload.to_string(),
    };
    hook(workspace, terminal, call, 0)
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

#[test]
fn the_now_line_shows_for_its_own_session_through_a_compaction_and_a_relaunch() {
    let disk = Disk::default();
    let mut unreadable = disk.clone();
    unreadable.write_state(&id("t1"), "not json");
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
    session_starts(&mut workspace, "t1", SESSION);
    assert_eq!(
        watched_now(&mut workspace, "t1"),
        None,
        "an unreadable state file is an empty board"
    );

    let set = now_set(&mut workspace, "t1", "Reading the PTY code");
    assert_eq!(
        now_lines(&set),
        vec![Some("Reading the PTY code".to_string())]
    );
    let replaced = now_set(&mut workspace, "t1", "Writing the plan");
    assert_eq!(
        now_lines(&replaced),
        vec![Some("Writing the plan".to_string())]
    );
    let again = now_set(&mut workspace, "t1", "Writing the plan");
    assert!(
        now_lines(&again).is_empty(),
        "the same line again sends nothing"
    );
    session_starts(&mut workspace, "t2", SESSION);
    let unwatched = now_set(&mut workspace, "t2", "Elsewhere");
    assert!(
        activity(&unwatched).is_empty(),
        "only the watched terminal's board goes out"
    );
    let compacted = session_starts(&mut workspace, "t1", SESSION);
    assert!(now_lines(&compacted).is_empty(), "a compaction keeps it");

    workspace.freeze();
    exited(&mut workspace, "t1");
    exited(&mut workspace, "t2");
    workspace.flush(1_000);
    let (mut reopened, _) = launched(&disk);
    assert_eq!(
        watched_now(&mut reopened, "t1"),
        Some("Writing the plan".to_string())
    );

    let cleared = session_starts(&mut reopened, "t1", "223e4567-e89b-12d3-a456-426614174000");
    assert_eq!(now_lines(&cleared), vec![None], "a new session hides it");
    exited(&mut reopened, "t1");
    assert!(
        disk.read_state(&id("t1")).is_none(),
        "a terminal that closes takes its state file with it"
    );
}

fn shared_by_agent(workspace: &mut Workspace, input: serde_json::Value) -> Vec<Outbound> {
    let tool = "mcp__plugin_taco-shells_activity__share";
    let payload = serde_json::json!({ "tool_name": tool, "tool_input": input, "cwd": "/work" });
    let call = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "PostToolUse".into(),
        payload: payload.to_string(),
    };
    hook(workspace, "t1", call, 0)
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

#[test]
fn a_shared_thing_tops_shared_logs_a_line_of_its_own_and_opens_to_its_text() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    let watch = ActivityRequest::Watch {
        terminal: Some(id("t1")),
    };
    workspace.activity(watch.clone());

    ran(&mut workspace, "t1", "cargo test");
    let plan = serde_json::json!({ "kind": "document", "title": "Plan", "target": "plan.md" });
    let first = shared_by_agent(&mut workspace, plan.clone());
    assert_eq!(shared_lists(&first), vec![vec!["Plan (Document)"]]);
    let finding =
        serde_json::json!({ "kind": "other", "title": "Finding", "body": "The font stops short." });
    shared_by_agent(&mut workspace, finding);
    let again = shared_by_agent(&mut workspace, plan);
    assert_eq!(
        shared_lists(&again),
        vec![vec!["Plan (Document)", "Finding (Note)"]],
        "sharing the same file again moves it up rather than listing it twice"
    );
    noted(&mut workspace, "t1", "Wrote the plan");
    assert_eq!(
        stretches(&mut workspace, "t1"),
        [
            "step 0 in 0",
            "Shared { thing: 0, title: \"Plan\" }",
            "Shared { thing: 1, title: \"Finding\" }",
            "Shared { thing: 2, title: \"Plan\" }",
            "note in 0"
        ],
        "a share is a line of its own and leaves the stretch for the note"
    );

    let opened = workspace.activity(ActivityRequest::Body {
        terminal: id("t1"),
        thing: 1,
    });
    let Some(ActivityEvent::Body { body, .. }) = activity(&opened).pop() else {
        panic!("a body request is answered");
    };
    assert_eq!(body.as_deref(), Some("The font stops short."));

    workspace.freeze();
    exited(&mut workspace, "t1");
    workspace.flush(1_000);
    let (mut reopened, _) = launched(&disk);
    let loaded = reopened.activity(watch);
    assert_eq!(
        shared_lists(&loaded),
        vec![vec!["Plan (Document)", "Finding (Note)"]],
        "Shared is kept through a relaunch"
    );
}

#[test]
fn the_app_adds_what_the_agent_opened_or_wrote_and_shows_a_pull_requests_state_once_seen() {
    let mut workspace = workspace();
    let watch = ActivityRequest::Watch {
        terminal: Some(id("t1")),
    };
    let gh = |workspace: &mut Workspace, operation: serde_json::Value, at_ms: u64| {
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": "gh pr" },
            "tool_response": { "gitOperation": { "pr": operation } },
        });
        let call = HookCall::AgentHook {
            harness: claude(),
            session_id: SESSION.into(),
            event: "PostToolUse".into(),
            payload: payload.to_string(),
        };
        hook(workspace, "t1", call, at_ms);
    };
    let shared_pull = serde_json::json!({ "kind": "pull_request", "title": "Older fix", "target": "https://github.com/o/r/pull/12" });
    shared_by_agent(&mut workspace, shared_pull);
    gh(
        &mut workspace,
        serde_json::json!({ "number": 12, "action": "merged" }),
        500,
    );
    let opened = serde_json::json!({ "number": 13, "url": "https://github.com/o/r/pull/13", "action": "created" });
    gh(&mut workspace, opened, 900);
    let wrote = serde_json::json!({
        "tool_name": "Write",
        "tool_input": { "file_path": "/work/PLAN.md" },
        "tool_response": { "type": "create", "filePath": "/work/PLAN.md" },
    });
    tool_used(&mut workspace, "t1", wrote);

    let out = workspace.activity(watch);
    let Some(ActivityEvent::Loaded { entries, board, .. }) = activity(&out).pop() else {
        panic!("watching t1 loads its board");
    };
    let things: Vec<String> = board
        .shared
        .into_iter()
        .map(|thing| {
            let mut line = format!("{} by {:?}", thing.title, thing.by);
            if let SharedTarget::PullRequest {
                state, checked_ms, ..
            } = thing.target
            {
                line = format!("{line}, {state:?} seen at {checked_ms:?}");
            }
            line
        })
        .collect();
    let expected = [
        "PLAN.md by App",
        "#13 o/r by App, Open seen at Some(900)",
        "Older fix by Agent, Merged seen at Some(500)",
    ];
    assert_eq!(
        things, expected,
        "the merge is seen on the agent's own link by its number"
    );
    let shared_lines: Vec<(String, Author)> = entries
        .into_iter()
        .filter_map(|entry| match entry.kind {
            ActivityKind::Shared { title, .. } => Some((title, entry.author)),
            _ => None,
        })
        .collect();
    let expected_lines = vec![
        ("Older fix".to_string(), Author::Agent),
        ("#13 o/r".to_string(), Author::App),
        ("PLAN.md".to_string(), Author::App),
    ];
    assert_eq!(
        shared_lines, expected_lines,
        "a merge changes the thing in place, with no line of its own"
    );
}

#[test]
fn a_compacted_session_gets_its_notes_back_and_a_stop_without_one_is_asked_for_a_note() {
    let mut workspace = workspace();
    let edit = |workspace: &mut Workspace, path: &str| {
        let payload =
            serde_json::json!({ "tool_name": "Edit", "tool_input": { "file_path": path } });
        tool_used(workspace, "t1", payload);
    };
    noted(&mut workspace, "t1", "From the session before");
    session_starts(&mut workspace, "t1", SESSION);
    let (context, _) = workspace.session_context(&id("t1"), &claude(), SESSION, "startup", 0);
    assert_eq!(context, None, "a new session is handed nothing");
    now_set(&mut workspace, "t1", "Fixing the logo");
    edit(&mut workspace, "/work/tacos/src/draw.rs");
    noted(&mut workspace, "t1", "Found the cause");
    noted(&mut workspace, "t1", "Planned the fix");
    let (resumed, _) = workspace.session_context(&id("t1"), &claude(), SESSION, "resume", 0);
    assert_eq!(
        resumed, None,
        "a resume keeps its own conversation, so it is handed nothing"
    );
    let (context, _) = workspace.session_context(&id("t1"), &claude(), SESSION, "compact", 0);
    let context = context.unwrap();
    assert!(context.contains("Fixing the logo"), "{context}");
    let newest = context.find("Planned the fix").unwrap();
    let older = context.find("Found the cause").unwrap();
    assert!(newest < older, "newest first");
    assert!(
        !context.contains("From the session before"),
        "only this session's own notes come back"
    );
    let unknown = HarnessId("other".into());
    let (refused, _) = workspace.session_context(&id("t1"), &unknown, SESSION, "compact", 0);
    assert_eq!(
        refused, None,
        "an agent the app does not know is handed nothing"
    );

    assert_eq!(
        workspace.stop_nudge(&id("t1"), &claude(), SESSION),
        None,
        "the change before the note is the note's"
    );
    edit(&mut workspace, "/work/tacos/src/logo.rs");
    let nudge = workspace.stop_nudge(&id("t1"), &claude(), SESSION).unwrap();
    assert!(
        nudge.contains("changed src/logo.rs,"),
        "only the open stretch counts: {nudge}"
    );
    let replaced = "223e4567-e89b-12d3-a456-426614174000";
    assert_eq!(
        workspace.stop_nudge(&id("t1"), &claude(), replaced),
        None,
        "a session that is not the terminal's is not asked"
    );
    let before_the_ask = stretches(&mut workspace, "t1").len();

    workspace.asked_for_note(&id("t1"), &claude(), SESSION, 0);
    noted(&mut workspace, "t1", "Drew the logo as shapes");
    let lines = stretches(&mut workspace, "t1");
    assert_eq!(
        lines.len(),
        before_the_ask + 2,
        "asking records nothing by itself"
    );
    let tail = &lines[lines.len() - 3..];
    assert_eq!(
        tail,
        ["step 7 in 7", "AskedForNote", "note in 7"],
        "the ask leaves the stretch for the note"
    );
}

#[test]
fn an_agent_whose_start_the_workspace_never_heard_is_still_asked_for_a_note() {
    let mut workspace = workspace();
    for _ in 0..3 {
        ran(&mut workspace, "t1", "cargo test");
    }
    let nudge = workspace.stop_nudge(&id("t1"), &claude(), SESSION);
    assert!(nudge.is_some_and(|nudge| nudge.contains("took 3 steps")));
}

#[test]
fn a_log_whose_terminal_is_gone_at_launch_is_removed_once_the_restore_has_succeeded() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    prompted(&mut workspace, "t1", SESSION, "open");
    workspace.flush(1_000);
    let mut stray = disk.clone();
    stray.append_log(&id("closed-while-crashing"), "{}");
    stray.write_state(&id("closed-while-crashing"), "{}");
    stray.write_state(&id("only-a-state-file"), "{}");

    let unreadable = Disk::holding("not a save");
    let mut kept = unreadable.clone();
    kept.append_log(&id("t9"), "{}");
    launched(&unreadable);
    assert!(
        unreadable.activity_terminals().contains(&id("t9")),
        "no restore, so nothing is known to be gone"
    );

    launched(&disk);
    assert_eq!(disk.activity_terminals(), vec![id("t1")]);

    // with the restore turned off, no earlier terminal can come back, so none keeps its log
    let mut fresh = Workspace::new(Options {
        restore_on_launch: false,
        ..options_on(&disk, 100)
    });
    fresh.start(0);
    assert!(disk.activity_terminals().is_empty());
}
