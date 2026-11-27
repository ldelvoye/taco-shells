use taco_agents::{NOTE_TOOL, activity_tool_prefix};
use taco_protocol::{
    ActivityEvent, ActivityKind, ActivityRequest, Author, HarnessId, HookCall, SharedTarget,
    WorkspaceRequest,
};

use super::*;

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
    let tool = format!("{}{NOTE_TOOL}", activity_tool_prefix());
    let payload = serde_json::json!({ "tool_name": tool, "tool_input": { "text": text } });
    tool_used(workspace, terminal, payload);
}

/// Each entry of a terminal's log as a short line: "step 3 in 2", "note in 2", "note alone", "asked".
fn stretches(workspace: &mut Workspace, terminal: &str) -> Vec<String> {
    let entries = loaded_entries(workspace, terminal);
    let lines = entries.into_iter().map(stretch_line);
    lines.collect()
}

#[test]
fn a_log_belongs_to_its_terminal_across_sessions_and_only_the_watched_one_is_streamed() {
    let mut workspace = workspace();
    ask(
        &mut workspace,
        WorkspaceRequest::OpenTerminal { file: None },
    );

    prompted(&mut workspace, "t1", SESSION, None, "first");
    // a /clear, a compaction or a new agent in the same terminal starts another session
    let other_session = "223e4567-e89b-12d3-a456-426614174000";
    prompted(&mut workspace, "t1", other_session, None, "second");
    assert_eq!(logged(&mut workspace, "t1"), vec!["first", "second"]);

    let streamed = prompted(&mut workspace, "t1", other_session, None, "third");
    let appended = activity(&streamed);
    assert!(matches!(&appended[..], [ActivityEvent::Appended { entry, .. }] if entry.seq == 2));
    let unwatched = prompted(&mut workspace, "t2", SESSION, None, "elsewhere");
    assert!(activity(&unwatched).is_empty());
    assert_eq!(logged(&mut workspace, "t2"), vec!["elsewhere"]);

    let nothing = ActivityRequest::Watch { terminal: None };
    assert!(workspace.activity(nothing).is_empty());
    let quiet = prompted(&mut workspace, "t2", SESSION, None, "fourth");
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
        prompted(&mut workspace, "t1", SESSION, None, &index.to_string());
    }
    assert_eq!(
        lines(&disk),
        crate::log::KEPT_ENTRIES,
        "rewritten down to what it keeps"
    );

    disk.fail(true);
    prompted(&mut workspace, "t1", SESSION, None, "lost on the disk");
    disk.fail(false);
    prompted(&mut workspace, "t1", SESSION, None, "after");
    let on_disk = disk.read_log(&id("t1"));
    assert_eq!(on_disk.len(), crate::log::KEPT_ENTRIES);
    assert!(on_disk[on_disk.len() - 2].contains("lost on the disk"));
}

#[test]
fn hook_payloads_become_messages_steps_and_notes_in_their_turns_stretch() {
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
    prompted(&mut workspace, "t1", SESSION, Some("turn-a"), messages);
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

    let expected = [
        "message from code-83",
        "message from code-f1",
        "step 2 in 2",
        "sent 3 in 2",
        "note in 2",
        "step 5 in 2",
    ];
    assert_eq!(stretches(&mut workspace, "t1"), expected);
}

#[test]
fn the_now_line_shows_for_its_own_session_and_through_a_compaction() {
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

    let cleared = session_starts(&mut workspace, "t1", "223e4567-e89b-12d3-a456-426614174000");
    assert_eq!(now_lines(&cleared), vec![None], "a new session hides it");
}

#[test]
fn a_shared_thing_tops_shared_logs_a_line_of_its_own_and_opens_to_its_text() {
    let mut workspace = workspace();
    let watch = ActivityRequest::Watch {
        terminal: Some(id("t1")),
    };
    workspace.activity(watch);

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
fn a_compacted_session_gets_its_now_line_and_its_own_notes_back_and_nothing_else_gets_any() {
    let mut workspace = workspace();
    noted(&mut workspace, "t1", "From the session before");
    session_starts(&mut workspace, "t1", SESSION);
    let (context, _) = workspace.session_context(&id("t1"), &claude(), SESSION, "startup", 0);
    assert_eq!(context, None, "a new session is handed nothing");
    now_set(&mut workspace, "t1", "Fixing the logo");
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
    assert!(context.contains("Planned the fix"), "{context}");
    assert!(context.contains("Found the cause"), "{context}");
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
}

#[test]
fn a_stop_after_real_work_with_no_note_is_asked_for_one_once_per_stretch() {
    let mut workspace = workspace();
    let edit = |workspace: &mut Workspace, path: &str| {
        let payload =
            serde_json::json!({ "tool_name": "Edit", "tool_input": { "file_path": path } });
        tool_used(workspace, "t1", payload);
    };
    for _ in 0..3 {
        ran(&mut workspace, "t1", "cargo test");
    }
    let nudge = workspace.stop_nudge(&id("t1"), &claude(), SESSION);
    assert!(nudge.is_some_and(|nudge| nudge.contains("took 3 steps")));

    session_starts(&mut workspace, "t1", SESSION);
    edit(&mut workspace, "/work/tacos/src/draw.rs");
    noted(&mut workspace, "t1", "Found the cause");
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
        ["step 5 in 5", "AskedForNote", "note in 5"],
        "the ask leaves the stretch for the note"
    );
}

#[test]
fn a_log_whose_terminal_is_gone_at_launch_is_removed_once_the_restore_has_succeeded() {
    let disk = Disk::default();
    let mut workspace = Workspace::new(options_on(&disk, 0));
    workspace.start(0);
    prompted(&mut workspace, "t1", SESSION, None, "open");
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
