//! The thread that owns the workspace: it takes one input at a time, wakes when the workspace has something due, and sends on what comes out.

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use taco_protocol::{
    ActivityRequest, HookEvent, RequestReply, RuntimeCommand, RuntimeReport, TerminalId,
    WorkspaceRequest,
};
use taco_workspace::{Options, Outbound, Workspace};
use uuid::Uuid;

use crate::link::{WindowOutput, WindowSender, WorkspacePort};

/// What the host sends the runtime; the runtime client in the app, and a fake in tests.
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
    /// Answered once the save file is written.
    FreezeAndFlush(Sender<()>),
}

enum Next {
    Input(Input),
    Due,
    Closed,
}

/// The window's and the app's way into the workspace thread. Cloning it shares the one thread.
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

/// Sends each output where it goes, in order; a request's reply goes last, after the state that holds its effect.
fn deliver(
    outputs: Vec<Outbound>,
    reply: Option<(u64, RequestReply)>,
    runtime: &impl RuntimeCommands,
    window: &WindowSender,
) {
    for output in outputs {
        let shown = match output {
            Outbound::Runtime(command) => {
                runtime.send(command);
                continue;
            }
            Outbound::State(state) => WindowOutput::State(state),
            Outbound::Activity(event) => WindowOutput::Activity(event),
            Outbound::Emptied => WindowOutput::Emptied,
        };
        // a send fails only once the window is gone
        let _ = window.unbounded_send(shown);
    }
    if let Some((token, reply)) = reply {
        let _ = window.unbounded_send(WindowOutput::Replied { token, reply });
    }
}

fn serve(
    options: Options,
    runtime: impl RuntimeCommands,
    window: WindowSender,
    inputs: Receiver<Input>,
) {
    let mut workspace = Workspace::new(options);
    let started = workspace.start(now_ms());
    deliver(started, None, &runtime, &window);

    loop {
        let input = match next_input(&workspace, &inputs) {
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
            Input::FreezeAndFlush(done) => {
                workspace.freeze();
                workspace.flush(now);
                let _ = done.send(());
                Vec::new()
            }
        };
        deliver(outputs, reply, &runtime, &window);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use futures::channel::mpsc::unbounded;
    use taco_protocol::{RuntimeCommand, TerminalId, WorkspaceRequest};
    use taco_workspace::{Options, Storage};

    use super::{RuntimeCommands, WorkspaceHost, new_terminal_id};
    use crate::link::{WindowOutput, WorkspacePort};

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
        fn logs(&self) -> Vec<TerminalId> {
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
}
