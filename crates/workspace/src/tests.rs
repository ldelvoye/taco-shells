use taco_protocol::{
    ActivityEvent, ActivityKind, ActivityRequest, AgentSignal, ConfigFile, DrawnState, HarnessId,
    HookCall, HookEvent, RowState, RuntimeCommand, RuntimeReport, TabGroupId, TerminalId,
    TerminalStart, WorkspaceRequest,
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

    fn logs(&self) -> Vec<TerminalId> {
        let logs = self.logs.lock().unwrap();
        logs.keys().cloned().collect()
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
fn a_close_goes_through_the_runtime_and_the_last_exit_ends_the_app_once() {
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
    let started = RuntimeReport::Started {
        id: id("t1"),
        directory: "/work".into(),
    };
    workspace.runtime_report(started, 0);
    exited(&mut workspace, "t1");
    assert_eq!(listed(&workspace), "[t2]");
    let drawn = workspace.drawn();
    assert!(!drawn.agent_rows.contains_key(&id("t1")));
    assert!(!drawn.directories.contains_key(&id("t1")));

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

    let out = exited(&mut workspace, "t2");
    assert!(out.contains(&Outbound::Emptied));
    let out = exited(&mut workspace, "t2");
    assert!(!out.contains(&Outbound::Emptied));
}

#[test]
fn when_the_runtime_dies_every_terminal_goes_the_window_says_why_and_nothing_reopens() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    let out = workspace.runtime_died("killed", 0);
    assert!(!out.contains(&Outbound::Emptied));
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

    for index in 0..(2 * crate::KEPT_ENTRIES) {
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
        !disk.logs().contains(&id("t2")),
        "a terminal that closes takes its log with it"
    );
    assert!(disk.logs().contains(&id("t1")));

    reopened.runtime_died("killed", 2_000);
    reopened.flush(3_000);
    let (mut after_crash, _) = launched(&disk);
    assert_eq!(logged(&mut after_crash, "t1"), vec!["kept through a quit"]);
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

    let unreadable = Disk::holding("not a save");
    let mut kept = unreadable.clone();
    kept.append_log(&id("t9"), "{}");
    launched(&unreadable);
    assert!(
        unreadable.logs().contains(&id("t9")),
        "no restore, so nothing is known to be gone"
    );

    launched(&disk);
    assert_eq!(disk.logs(), vec![id("t1")]);
}
