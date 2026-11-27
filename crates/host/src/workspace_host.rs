//! The thread that owns the workspace: it takes one input at a time, wakes when the workspace has something due, and sends on what comes out.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use taco_protocol::{
    ActivityRequest, DrawnState, HarnessId, HookEvent, RequestReply, RuntimeCommand, RuntimeReport,
    TerminalId, WorkspaceRequest,
};
use taco_workspace::{Options, Outbound, Workspace};
use uuid::Uuid;

use taco_rules::default_panels;

use crate::ports::{WindowOutput, WindowSender, WorkspacePort};

const WORKSPACE_STOPPED: &str = "Taco Shells hit an internal error and stopped keeping the terminal list: changes to it are no longer shown or saved. The terminals still work. Quit and reopen Taco Shells.";

/// What the host sends the runtime: the runtime client in this crate, and a fake in tests.
pub trait RuntimeCommands: Send + 'static {
    fn send(&self, command: RuntimeCommand);
}

enum Input {
    Request {
        request: WorkspaceRequest,
        token: Option<u64>,
    },
    Activity(ActivityRequest),
    Report(RuntimeReport),
    RuntimeDied(String),
    Hook(HookEvent),
    SessionContext {
        terminal: TerminalId,
        harness: HarnessId,
        session_id: String,
        source: String,
        answer: mpsc::Sender<Option<String>>,
    },
    StopNudge {
        terminal: TerminalId,
        harness: HarnessId,
        session_id: String,
        answer: mpsc::Sender<Option<String>>,
    },
    AskedForNote {
        terminal: TerminalId,
        harness: HarnessId,
        session_id: String,
    },
    /// Answered once the save file is written.
    FreezeAndFlush(Sender<()>),
}

enum Next {
    Input(Input),
    Due,
    Closed,
}

/// The window's and the other hosts' way into the workspace thread. Cloning it shares the one thread.
#[derive(Clone)]
pub struct WorkspaceHost {
    inputs: Sender<Input>,
}

/// A terminal id that no earlier launch used: 32 lowercase hex characters, random.
pub fn new_terminal_id() -> TerminalId {
    let id = Uuid::new_v4().simple().to_string();
    TerminalId(id)
}

fn now_ms() -> u64 {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH);
    match since_epoch {
        Ok(elapsed) => elapsed.as_millis() as u64,
        Err(_) => 0,
    }
}

impl WorkspaceHost {
    /// Starts the thread that owns the `Workspace`, runs `start` and sends what it returns. `options.new_terminal_id` is `new_terminal_id`.
    pub fn start(
        options: Options,
        runtime: impl RuntimeCommands,
        window: WindowSender,
    ) -> WorkspaceHost {
        let (inputs, received) = unbounded();
        let spawned = thread::Builder::new()
            .name("workspace".into())
            .spawn(move || serve(options, runtime, window, received));
        if let Err(error) = spawned {
            eprintln!("the workspace thread did not start: {error}");
        }
        WorkspaceHost { inputs }
    }

    // A send fails only once the thread has gone, and then there is nothing left to tell.
    fn put(&self, input: Input) {
        let _ = self.inputs.send(input);
    }

    pub fn runtime_report(&self, report: RuntimeReport) {
        self.put(Input::Report(report));
    }

    /// Freezes what is saved first, as `Workspace::runtime_died` does.
    pub fn runtime_died(&self, reason: String) {
        self.put(Input::RuntimeDied(reason));
    }

    pub fn hook(&self, event: HookEvent) {
        self.put(Input::Hook(event));
    }

    /// What a session starting in `terminal` is handed back, answered after every input sent before it: Some only
    /// after a compaction, whose `source` is Claude Code's "compact".
    pub fn session_context(
        &self,
        terminal: TerminalId,
        harness: HarnessId,
        session_id: String,
        source: String,
    ) -> mpsc::Receiver<Option<String>> {
        let (answer, answered) = mpsc::channel();
        self.put(Input::SessionContext {
            terminal,
            harness,
            session_id,
            source,
            answer,
        });
        answered
    }

    /// The ask for a note an agent is about to stop without, or none; answered after every input sent before it.
    pub fn stop_nudge(
        &self,
        terminal: TerminalId,
        harness: HarnessId,
        session_id: String,
    ) -> mpsc::Receiver<Option<String>> {
        let (answer, answered) = mpsc::channel();
        self.put(Input::StopNudge {
            terminal,
            harness,
            session_id,
            answer,
        });
        answered
    }

    /// Records in the log that the agent was asked for a note; called only once the ask has reached it.
    pub fn asked_for_note(&self, terminal: TerminalId, harness: HarnessId, session_id: String) {
        self.put(Input::AskedForNote {
            terminal,
            harness,
            session_id,
        });
    }

    /// `freeze`, then `flush`; blocks the caller until the save file is written, behind every input sent before it. Called once, at quit, before the runtime is stopped.
    pub fn freeze_and_flush(&self) {
        let (done, finished) = bounded(1);
        self.put(Input::FreezeAndFlush(done));
        let _ = finished.recv();
    }
}

impl WorkspacePort for WorkspaceHost {
    fn send(&self, request: WorkspaceRequest, token: Option<u64>) {
        self.put(Input::Request { request, token });
    }

    fn activity(&self, request: ActivityRequest) {
        self.put(Input::Activity(request));
    }
}

// The next input, or the workspace's deadline if it comes first. A deadline already passed is due before any waiting input, so a stream of inputs never holds back a save.
fn next_input(workspace: &Workspace, inputs: &Receiver<Input>) -> Next {
    let Some(deadline) = workspace.next_deadline() else {
        return match inputs.recv() {
            Ok(input) => Next::Input(input),
            Err(_) => Next::Closed,
        };
    };
    let now = now_ms();
    if deadline <= now {
        return Next::Due;
    }
    let wait = Duration::from_millis(deadline - now);
    match inputs.recv_timeout(wait) {
        Ok(input) => Next::Input(input),
        Err(RecvTimeoutError::Timeout) => Next::Due,
        Err(RecvTimeoutError::Disconnected) => Next::Closed,
    }
}

/// Sends each output where it goes, in order; a request's reply goes last, after the state that holds its effect. Keeps the last state sent in `published`.
fn deliver(
    outputs: Vec<Outbound>,
    reply: Option<(u64, RequestReply)>,
    runtime: &impl RuntimeCommands,
    window: &WindowSender,
    published: &mut Option<DrawnState>,
) {
    for output in outputs {
        let shown = match output {
            Outbound::Runtime(command) => {
                runtime.send(command);
                continue;
            }
            Outbound::State(state) => {
                *published = Some(state.clone());
                WindowOutput::State(state)
            }
            Outbound::Activity(event) => WindowOutput::Activity(event),
        };
        // a send fails only once the window is gone
        let _ = window.unbounded_send(shown);
    }
    if let Some((token, reply)) = reply {
        let _ = window.unbounded_send(WindowOutput::Replied { token, reply });
    }
}

/// Runs the workspace until its inputs close. On a panic, shows the last state with a problem, then answers waiting callers without saving.
fn serve(
    options: Options,
    runtime: impl RuntimeCommands,
    window: WindowSender,
    inputs: Receiver<Input>,
) {
    let mut published = None;
    let ran = panic::catch_unwind(AssertUnwindSafe(|| {
        run(options, &runtime, &window, &inputs, &mut published);
    }));
    if ran.is_ok() {
        return;
    }
    show_stopped(published, &window);
    answer_without_saving(&inputs);
}

/// The state drawn before the workspace publishes its first.
pub fn nothing_drawn() -> DrawnState {
    DrawnState {
        revision: 0,
        arrangement: None,
        agent_rows: BTreeMap::new(),
        directories: BTreeMap::new(),
        refocus: 0,
        focus_count: 0,
        agents: BTreeMap::new(),
        default_agent_name: String::new(),
        panels: default_panels(),
        problem: None,
        started: BTreeSet::new(),
    }
}

fn show_stopped(published: Option<DrawnState>, window: &WindowSender) {
    let mut shown = match published {
        Some(state) => state,
        None => nothing_drawn(),
    };
    // The window draws only a newer revision.
    shown.revision += 1;
    shown.problem = Some(WORKSPACE_STOPPED.to_string());
    let _ = window.unbounded_send(WindowOutput::State(shown));
}

/// The rest of the inputs, until they close: the quit's save and the session questions are answered so nobody waits on them, and nothing else is done.
fn answer_without_saving(inputs: &Receiver<Input>) {
    for input in inputs {
        match input {
            Input::FreezeAndFlush(done) => {
                let _ = done.send(());
            }
            Input::SessionContext { answer, .. } | Input::StopNudge { answer, .. } => {
                let _ = answer.send(None);
            }
            _ => {}
        }
    }
}

fn run(
    options: Options,
    runtime: &impl RuntimeCommands,
    window: &WindowSender,
    inputs: &Receiver<Input>,
    published: &mut Option<DrawnState>,
) {
    let mut workspace = Workspace::new(options);
    let started = workspace.start(now_ms());
    deliver(started, None, runtime, window, published);

    loop {
        let input = match next_input(&workspace, inputs) {
            Next::Input(input) => input,
            Next::Due => {
                workspace.tick(now_ms());
                continue;
            }
            Next::Closed => return,
        };
        let now = now_ms();
        let mut reply = None;
        let outputs = match input {
            Input::Request { request, token } => {
                let (answer, outputs) = workspace.request(request, now);
                if let Some(token) = token {
                    reply = Some((token, answer));
                }
                outputs
            }
            Input::Activity(request) => workspace.activity(request),
            Input::Report(report) => workspace.runtime_report(report, now),
            Input::RuntimeDied(reason) => workspace.runtime_died(&reason, now),
            Input::Hook(event) => workspace.hook(event, now),
            Input::SessionContext {
                terminal,
                harness,
                session_id,
                source,
                answer,
            } => {
                let (context, outputs) =
                    workspace.session_context(&terminal, &harness, &session_id, &source, now);
                // the asker may have stopped waiting, and then nobody needs the answer
                let _ = answer.send(context);
                outputs
            }
            Input::StopNudge {
                terminal,
                harness,
                session_id,
                answer,
            } => {
                let nudge = workspace.stop_nudge(&terminal, &harness, &session_id);
                let _ = answer.send(nudge);
                Vec::new()
            }
            Input::AskedForNote {
                terminal,
                harness,
                session_id,
            } => workspace.asked_for_note(&terminal, &harness, &session_id, now),
            Input::FreezeAndFlush(done) => {
                workspace.freeze();
                workspace.flush(now);
                let _ = done.send(());
                Vec::new()
            }
        };
        deliver(outputs, reply, runtime, window, published);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use futures::channel::mpsc::unbounded;
    use taco_protocol::{
        ActivityEvent, ActivityKind, ActivityRequest, HarnessId, HookCall, HookEvent,
        RuntimeCommand, TerminalId, WorkspaceRequest,
    };
    use taco_workspace::{Options, Storage};

    use super::{RuntimeCommands, WorkspaceHost, new_terminal_id};
    use crate::ports::{WindowOutput, WorkspacePort};

    #[derive(Clone, Default)]
    struct Commands(Arc<Mutex<Vec<RuntimeCommand>>>);

    impl RuntimeCommands for Commands {
        fn send(&self, command: RuntimeCommand) {
            self.0.lock().unwrap().push(command);
        }
    }

    struct Nowhere;

    impl Storage for Nowhere {
        fn read_save(&self) -> Option<String> {
            None
        }
        fn write_save(&mut self, _text: &str) -> bool {
            true
        }
        fn read_log(&self, _terminal: &TerminalId) -> Vec<String> {
            Vec::new()
        }
        fn append_log(&mut self, _terminal: &TerminalId, _line: &str) -> bool {
            true
        }
        fn rewrite_log(&mut self, _terminal: &TerminalId, _lines: &[String]) -> bool {
            true
        }
        fn delete_log(&mut self, _terminal: &TerminalId) {}
        fn read_state(&self, _terminal: &TerminalId) -> Option<String> {
            None
        }
        fn write_state(&mut self, _terminal: &TerminalId, _text: &str) -> bool {
            true
        }
        fn delete_state(&mut self, _terminal: &TerminalId) {}
        fn activity_terminals(&self) -> Vec<TerminalId> {
            Vec::new()
        }
    }

    #[test]
    fn a_reply_comes_after_the_state_that_holds_its_effect() {
        let commands = Commands::default();
        let (window, mut shown) = unbounded();
        let options = Options {
            settings_path: String::new(),
            keybindings_path: String::new(),
            new_terminal_id: Box::new(new_terminal_id),
            storage: Box::new(Nowhere),
            restore_on_launch: false,
        };
        let host = WorkspaceHost::start(options, commands.clone(), window);

        host.send(WorkspaceRequest::OpenTerminal { file: None }, Some(7));
        // the thread takes inputs in order, so this returns once the request has been applied
        host.freeze_and_flush();

        let mut outputs = Vec::new();
        while let Ok(output) = shown.try_recv() {
            outputs.push(output);
        }
        let Some(WindowOutput::Replied { token, reply }) = outputs.last() else {
            panic!("the reply is the last thing the window is sent");
        };
        let Some(WindowOutput::State(state)) = outputs.iter().rev().nth(1) else {
            panic!("a state comes just before the reply");
        };
        assert_eq!((*token, reply.revision), (7, state.revision));
        let rows = state.arrangement.as_ref().unwrap().rows.len();
        assert_eq!(rows, 2, "the state holds the terminal the request opened");

        let opened = commands.0.lock().unwrap().len();
        assert_eq!(opened, 2, "the first terminal and the requested one");
        let first_id = &state.arrangement.as_ref().unwrap().rows[0].terminal.0;
        let hex = first_id
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase());
        assert!(first_id.len() == 32 && hex);
    }

    #[test]
    fn a_question_is_answered_once_after_the_inputs_sent_before_it() {
        let (window, mut shown) = unbounded();
        let options = Options {
            settings_path: String::new(),
            keybindings_path: String::new(),
            new_terminal_id: Box::new(new_terminal_id),
            storage: Box::new(Nowhere),
            restore_on_launch: false,
        };
        let host = WorkspaceHost::start(options, Commands::default(), window);
        // a call that blocks until the thread is through what came before, so the first terminal is published
        host.freeze_and_flush();
        let mut terminal = None;
        while let Ok(output) = shown.try_recv() {
            if let WindowOutput::State(state) = output {
                let rows = state.arrangement.unwrap().rows;
                terminal = Some(rows[0].terminal.clone());
            }
        }
        let terminal = terminal.expect("the first terminal is published");
        let claude = HarnessId("claude".into());
        let session = "123e4567-e89b-12d3-a456-426614174000".to_string();
        let note = r#"{"tool_name":"mcp__plugin_taco-shells-dev_activity__note","tool_input":{"text":"Found the cause"}}"#;
        host.hook(HookEvent {
            terminal: terminal.clone(),
            call: HookCall::AgentHook {
                harness: claude.clone(),
                session_id: session.clone(),
                event: "PostToolUse".into(),
                payload: note.to_string(),
            },
        });

        let wait = Duration::from_secs(5);
        let answered = host.session_context(
            terminal.clone(),
            claude.clone(),
            session.clone(),
            "compact".into(),
        );
        let context = answered.recv_timeout(wait).unwrap().unwrap();
        assert!(
            context.contains("Found the cause"),
            "the note sent before it is in the answer"
        );
        let unknown = TerminalId("gone".into());
        let nothing = host.stop_nudge(unknown, claude.clone(), session.clone());
        assert_eq!(
            nothing.recv_timeout(wait).unwrap(),
            None,
            "a question always gets its one answer"
        );

        let edit = r#"{"tool_name":"Edit","tool_input":{"file_path":"/work/logo.rs"}}"#;
        host.hook(HookEvent {
            terminal: terminal.clone(),
            call: HookCall::AgentHook {
                harness: claude.clone(),
                session_id: session.clone(),
                event: "PostToolUse".into(),
                payload: edit.to_string(),
            },
        });
        let asked = host.stop_nudge(terminal.clone(), claude.clone(), session.clone());
        let nudge = asked.recv_timeout(wait).unwrap();
        assert!(nudge.is_some_and(|nudge| nudge.contains("logo.rs")));
        host.asked_for_note(terminal.clone(), claude, session);
        host.activity(ActivityRequest::Watch {
            terminal: Some(terminal),
        });
        host.freeze_and_flush();
        let mut asked_logged = false;
        while let Ok(output) = shown.try_recv() {
            if let WindowOutput::Activity(ActivityEvent::Loaded { entries, .. }) = output {
                asked_logged = entries
                    .iter()
                    .any(|entry| entry.kind == ActivityKind::AskedForNote);
            }
        }
        assert!(asked_logged, "the host records the ask");
    }

    /// Counts its saves and panics in the first, as a fault anywhere in the workspace would.
    struct BreaksOnSave(Arc<AtomicUsize>);

    impl Storage for BreaksOnSave {
        fn read_save(&self) -> Option<String> {
            None
        }
        fn write_save(&mut self, _text: &str) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("a fault in the workspace, on purpose");
        }
        fn read_log(&self, _terminal: &TerminalId) -> Vec<String> {
            Vec::new()
        }
        fn append_log(&mut self, _terminal: &TerminalId, _line: &str) -> bool {
            true
        }
        fn rewrite_log(&mut self, _terminal: &TerminalId, _lines: &[String]) -> bool {
            true
        }
        fn delete_log(&mut self, _terminal: &TerminalId) {}
        fn read_state(&self, _terminal: &TerminalId) -> Option<String> {
            None
        }
        fn write_state(&mut self, _terminal: &TerminalId, _text: &str) -> bool {
            true
        }
        fn delete_state(&mut self, _terminal: &TerminalId) {}
        fn activity_terminals(&self) -> Vec<TerminalId> {
            Vec::new()
        }
    }

    #[test]
    fn a_panic_in_the_workspace_is_shown_and_after_it_nothing_is_saved_and_nobody_waits() {
        let saves = Arc::new(AtomicUsize::new(0));
        let (window, mut shown) = unbounded();
        let options = Options {
            settings_path: String::new(),
            keybindings_path: String::new(),
            new_terminal_id: Box::new(new_terminal_id),
            storage: Box::new(BreaksOnSave(saves.clone())),
            restore_on_launch: false,
        };
        let host = WorkspaceHost::start(options, Commands::default(), window);

        // The first save panics; the call still returns.
        host.freeze_and_flush();
        host.send(WorkspaceRequest::OpenTerminal { file: None }, None);
        // Answered after the window has been told, and without writing anything.
        host.freeze_and_flush();

        let mut states = Vec::new();
        while let Ok(WindowOutput::State(state)) = shown.try_recv() {
            states.push(state);
        }
        let (stopped, before) = states.split_last().unwrap();
        assert_eq!(stopped.problem.as_deref(), Some(super::WORKSPACE_STOPPED));
        let last_before = before.last().unwrap();
        assert!(
            stopped.revision > last_before.revision,
            "drawn, not skipped as old"
        );
        assert_eq!(
            stopped.arrangement, last_before.arrangement,
            "the rows stay as they were"
        );

        let wait = Duration::from_secs(5);
        let terminal = TerminalId("any".into());
        let claude = HarnessId("claude".into());
        let nudge = host.stop_nudge(terminal, claude, "session".into());
        assert_eq!(nudge.recv_timeout(wait).unwrap(), None);
        assert_eq!(saves.load(Ordering::SeqCst), 1);
    }
}
