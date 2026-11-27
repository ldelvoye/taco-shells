use taco_protocol::{
    AgentSignal, ConfigFile, DrawnState, HookCall, RowState, RuntimeCommand, RuntimeReport,
    TabGroupId, WorkspaceRequest,
};

use super::*;

fn published(out: &[Outbound]) -> Option<DrawnState> {
    let mut states = out.iter().filter_map(|outbound| match outbound {
        Outbound::State(state) => Some(state.clone()),
        _ => None,
    });
    states.next_back()
}

fn agent_signals(workspace: &mut Workspace, terminal: &str, signal: AgentSignal, now_ms: u64) {
    let call = HookCall::AgentState {
        harness: claude(),
        session_id: SESSION.into(),
        signal,
    };
    hook(workspace, terminal, call, now_ms);
}

fn row_state(workspace: &Workspace, terminal: &str) -> Option<(RowState, Option<String>)> {
    let drawn = workspace.drawn();
    let row = drawn.agent_rows.get(&id(terminal))?;
    Some((row.state, row.question.clone()))
}

#[test]
fn ids_and_tab_groups_are_minted_in_order_a_config_file_opens_escaped_and_a_row_opens_in_a_group() {
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
fn a_rows_title_state_and_question_follow_its_agents_hooks_and_the_windows_focus() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );
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

    let payload = r#"{"notification_type":"permission_prompt","message":"Allow Bash?"}"#;
    let asks = HookCall::AgentHook {
        harness: claude(),
        session_id: SESSION.into(),
        event: "Notification".into(),
        payload: payload.into(),
    };
    hook(&mut workspace, "t1", asks, 2);
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Idle, None)));
    agent_signals(&mut workspace, "t1", AgentSignal::Needs, 3);
    assert_eq!(
        row_state(&workspace, "t1"),
        Some((RowState::Needs, Some("Allow Bash?".into())))
    );

    agent_signals(&mut workspace, "t1", AgentSignal::Stopped, 4);
    ask(
        &mut workspace,
        WorkspaceRequest::FocusReported {
            terminal: id("t1"),
            count: 1,
        },
    );
    assert_eq!(row_state(&workspace, "t1"), Some((RowState::Idle, None)));
}
