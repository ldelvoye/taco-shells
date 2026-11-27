use taco_protocol::{ActivityRequest, TabGroupId, TerminalId, WorkspaceRequest};

use crate::sessions::PROVISIONAL_MS;

use super::*;

const RESTORED_FILE: &str = concat!(
    r#"{"version":2,"selected":1,"rows":["#,
    r#"{"id":"t1","directory":"/Users/me/code/tacos","agent":{"harness":"claude","sessionId":"123e4567-e89b-12d3-a456-426614174000","cwd":"/Users/me/code/tacos"}},"#,
    r#"{"id":"t2","directory":"/Users/me/code/tacos/docs"},"#,
    r#"{"id":"t3","directory":"/Users/me","agent":{"harness":"claude","sessionId":"223e4567-e89b-12d3-a456-426614174000","cwd":"/Users/me/notes"}},"#,
    r#"{"id":"t4"}]}"#,
);

fn grouped_collapsed_and_moved_by_300_ms(disk: &Disk) -> Workspace {
    let mut workspace = Workspace::new(options_on(disk, 0));
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
    workspace
}

#[test]
fn a_save_reopens_each_row_in_its_directory_and_resumes_its_agent_where_it_ran() {
    let disk = Disk::holding(RESTORED_FILE);
    let (workspace, starts) = launched(&disk);

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
fn a_save_brings_back_the_same_rows_ids_collapsed_group_and_panel_edge_after_a_relaunch() {
    let disk = Disk::default();
    let mut workspace = grouped_collapsed_and_moved_by_300_ms(&disk);
    workspace.flush(1_000);

    let (reopened, starts) = launched(&disk);
    let ids: Vec<TerminalId> = starts.iter().map(|start| start.id.clone()).collect();
    assert_eq!(ids, vec![id("t1"), id("t2")]);
    assert_eq!(listed(&reopened), "[t1] t2");
    let drawn = reopened.drawn();
    let arrangement = drawn.arrangement.unwrap();
    assert!(arrangement.tab_groups[0].collapsed);
    let activity = drawn.panels.place(taco_protocol::Panel::AgentActivity);
    assert_eq!(activity.edge, taco_protocol::Edge::Bottom);
}

#[test]
fn a_save_waits_out_its_debounce_skips_a_title_retries_a_failed_write_and_caps_its_wait_at_2_s() {
    let disk = Disk::default();
    let mut workspace = grouped_collapsed_and_moved_by_300_ms(&disk);

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
    prompted(&mut workspace, "t1", SESSION, None, "kept through a quit");
    prompted(&mut workspace, "t2", SESSION, None, "kept too");
    session_starts(&mut workspace, "t1", SESSION);
    session_starts(&mut workspace, "t2", SESSION);
    now_set(&mut workspace, "t1", "Writing the plan");
    now_set(&mut workspace, "t2", "Reading the PTY code");
    let plan = serde_json::json!({ "kind": "document", "title": "Plan", "target": "plan.md" });
    shared_by_agent(&mut workspace, plan);
    let finding =
        serde_json::json!({ "kind": "other", "title": "Finding", "body": "The font stops short." });
    shared_by_agent(&mut workspace, finding);
    let keeps_t1 = |workspace: &mut Workspace, after: &str| {
        let expected_log = vec![
            "kept through a quit",
            "Shared { thing: 0, title: \"Plan\" }",
            "Shared { thing: 1, title: \"Finding\" }",
        ];
        assert_eq!(logged(workspace, "t1"), expected_log, "{after}");
        let now = watched_now(workspace, "t1");
        assert_eq!(now, Some("Writing the plan".to_string()), "{after}");
        let watch = ActivityRequest::Watch {
            terminal: Some(id("t1")),
        };
        let loaded = workspace.activity(watch);
        assert_eq!(
            shared_lists(&loaded),
            vec![vec!["Finding (Note)", "Plan (Document)"]],
            "Shared is kept through {after}"
        );
    };

    // the quit: frozen first, then every shell goes as the runtime stops
    workspace.freeze();
    exited(&mut workspace, "t1");
    exited(&mut workspace, "t2");
    workspace.flush(1_000);

    let (mut reopened, _) = launched(&disk);
    assert_eq!(listed(&reopened), "t1 [t2]");
    keeps_t1(&mut reopened, "a quit");
    assert_eq!(logged(&mut reopened, "t2"), vec!["kept too"]);
    assert_eq!(
        watched_now(&mut reopened, "t2"),
        Some("Reading the PTY code".to_string())
    );

    exited(&mut reopened, "t2");
    assert!(
        disk.read_log(&id("t2")).is_empty(),
        "a terminal that closes takes its log with it"
    );
    assert!(
        disk.read_state(&id("t2")).is_none(),
        "a terminal that closes takes its state file with it"
    );

    reopened.runtime_died("killed", 2_000);
    reopened.flush(3_000);
    let mut after_crash = Workspace::new(options_on(&disk, 10));
    after_crash.start(0);
    assert_eq!(listed(&after_crash), "[t1]");
    keeps_t1(&mut after_crash, "a runtime death");

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
    let disk = Disk::holding(RESTORED_FILE);
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
fn a_restore_keeps_ids_apart_opens_the_selected_group_and_falls_back_to_one_fresh_terminal() {
    let file = r#"{"version":2,"selected":1,"tabGroups":[{"name":"g","collapsed":true}],
        "rows":[{"id":"a","tabGroup":0},{"id":"a","tabGroup":0},{"id":"","tabGroup":0}]}"#;
    let (workspace, _) = launched(&Disk::holding(file));

    assert_eq!(listed(&workspace), "a [t1] t2");
    let arrangement = workspace.drawn().arrangement.unwrap();
    assert!(!arrangement.tab_groups[0].collapsed);

    for text in ["not json", r#"{"version":2,"rows":[]}"#, r#"{"version":9}"#] {
        let (workspace, starts) = launched(&Disk::holding(text));
        assert_eq!(
            (listed(&workspace), starts.len()),
            ("[t1]".to_string(), 1),
            "{text}"
        );
    }
    let disk = Disk::holding(RESTORED_FILE);
    let mut workspace = Workspace::new(Options {
        restore_on_launch: false,
        ..options_on(&disk, 0)
    });
    let starts = opened(&workspace.start(0));
    assert_eq!(starts.len(), 1);
}
