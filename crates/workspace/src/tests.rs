use taco_protocol::{
    AgentSignal, ConfigFile, DrawnState, HarnessId, HookCall, HookEvent, RowState, RuntimeCommand,
    RuntimeReport, TabGroupId, TerminalId, TerminalStart, WorkspaceRequest,
};

use crate::{Options, Outbound, Workspace};

const SESSION: &str = "123e4567-e89b-12d3-a456-426614174000";

fn id(name: &str) -> TerminalId {
    TerminalId(name.to_string())
}

fn claude() -> HarnessId {
    HarnessId("claude".into())
}

fn workspace() -> Workspace {
    let mut next = 0;
    let options = Options {
        settings_path: "/config/it's/settings.json".into(),
        keybindings_path: "/config/keybindings.json".into(),
        new_terminal_id: Box::new(move || {
            next += 1;
            id(&format!("t{next}"))
        }),
    };
    let mut workspace = Workspace::new(options);
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
    names.collect::<Vec<_>>().join(" ")
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
        settings_path: String::new(),
        keybindings_path: String::new(),
        new_terminal_id: Box::new(|| id("only")),
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
}

#[test]
fn a_fork_opens_under_its_source_with_the_agents_fork_line_or_as_a_plain_shell_beside_it() {
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

    let out = ask(
        &mut workspace,
        WorkspaceRequest::ForkTerminal { terminal: id("t2") },
    );
    let plain = &opened(&out)[0];
    assert_eq!(
        (plain.directory_of.clone(), plain.command.clone()),
        (Some(id("t2")), None)
    );

    let (reply, out) = workspace.request(
        WorkspaceRequest::ForkTerminal {
            terminal: id("gone"),
        },
        0,
    );
    assert!(!reply.ok);
    assert!(opened(&out).is_empty());
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
    exited(&mut workspace, "t1");
    assert_eq!(listed(&workspace), "[t2]");
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
fn attention_waits_for_the_app_to_come_back_and_a_hook_can_take_the_keyboard() {
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
    let refocus = workspace.drawn().refocus;
    workspace.app_activated(0);
    assert_eq!(listed(&workspace), "[t1] t2");
    assert_eq!(workspace.drawn().refocus, refocus + 1);
    assert!(
        workspace.app_activated(0).is_empty(),
        "arriving clears the wait"
    );

    hook(&mut workspace, "t1", HookCall::FocusTab, 0);
    assert_eq!(workspace.drawn().refocus, refocus + 2);
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
}

#[test]
fn a_title_is_read_by_the_rows_agent_and_read_again_once_the_agent_starts() {
    let mut workspace = workspace();
    titled(&mut workspace, "t1", "✳ Tacos");
    let title =
        |workspace: &Workspace| workspace.drawn().arrangement.unwrap().rows[0].title.clone();
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
