//! The services host: config, agent setup, the hook listener and the updater, wired to the runtime, the workspace and the window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use taco_identity::PLUGIN_PROGRAM;
use taco_protocol::{
    ConfigFile, HarnessId, HookCall, HookEvent, HookMessage, RuntimeAnswer, RuntimeQuery,
    TerminalId,
};
use taco_services::{
    AgentMenuAction, AgentSetups, AgentSetupsHost, AnswerPart, ConfigChange, ConfigStore,
    ConfigWriter, HookListener, ListenerEvent, PluginHeard, Question, QuestionKind, Refusal,
    UpdateEvent, Updater, UpdaterHost, app_version, config_file_path, focus_port_path,
};
use taco_settings::{Config, KeymapChange, SettingChange};

use crate::ports::{ServicesOutput, WindowOutput, WindowSender};
use crate::runtime_client::RuntimeClient;
use crate::supervised::spawn_supervised;
use crate::terminal_settings::send_terminal_settings;
use crate::workspace_host::WorkspaceHost;

const PORTS_TAKEN: &str = "Every focus port is taken, so agents' hooks cannot reach this app.";
const ROUTE_NOT_STARTED: &str =
    "Agents' hooks cannot reach the terminals: the thread that routes them did not start.";
const ROUTE_STOPPED: &str = "Agents' hooks stopped reaching the terminals after an internal error. Quit and reopen Taco Shells.";
// A runtime that is alive but never answers must not hold a hook, a setup check or quitting for ever.
const RUNTIME_ANSWER_WAIT: Duration = Duration::from_secs(2);
const FORWARD_WAIT: Duration = Duration::from_secs(1);

type Slot<T> = Mutex<Option<T>>;
type Work = Box<dyn FnOnce() + Send>;

enum Routed {
    Post(HookMessage),
    Question(Question),
}

#[derive(Clone)]
pub struct ServicesHost {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<Slot<ConfigStore>>,
    writer: Arc<Slot<ConfigWriter>>,
    setups: Arc<Slot<AgentSetups>>,
    updater: Arc<Slot<Updater>>,
    listener: Slot<HookListener>,
    hook_inbox: Arc<Slot<Sender<Routed>>>,
    last_config: Mutex<Config>,
}

fn locked<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send_to_window(window: &WindowSender, output: ServicesOutput) {
    let _ = window.unbounded_send(WindowOutput::Services(output));
}

fn plugin_program_beside(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    Some(directory.join(PLUGIN_PROGRAM))
}

fn start_setups(
    directory: &Path,
    activity_on: &Arc<AtomicBool>,
    writer: &Arc<Slot<ConfigWriter>>,
    runtime: &RuntimeClient,
    window: &WindowSender,
) -> AgentSetups {
    let is_on_flag = Arc::clone(activity_on);
    let is_on = move || is_on_flag.load(Ordering::SeqCst);
    let turn_on_writer = Arc::clone(writer);
    let turn_on = move || {
        let writer = locked(&turn_on_writer);
        if let Some(writer) = writer.as_ref() {
            let change = SettingChange::AgentActivity(true);
            writer.write(ConfigChange::Setting(change));
        }
    };
    let ask_runtime = runtime.clone();
    let foreground_commands = move |terminal: &TerminalId| {
        let query = RuntimeQuery::ForegroundCommands {
            id: terminal.clone(),
        };
        let answered = ask_runtime.ask(query);
        let answer = answered.recv_timeout(RUNTIME_ANSWER_WAIT);
        match answer {
            Ok(Some(RuntimeAnswer::Commands(commands))) => commands,
            _ => Vec::new(),
        }
    };
    let changed_window = window.clone();
    let on_change = move |view| send_to_window(&changed_window, ServicesOutput::Setups(view));
    let host = AgentSetupsHost {
        is_on: Box::new(is_on),
        turn_on: Box::new(turn_on),
        foreground_commands: Box::new(foreground_commands),
        on_change: Box::new(on_change),
    };
    let found = std::env::current_exe();
    let mut executable = PathBuf::new();
    let mut plugin_program = None;
    if let Ok(path) = found {
        plugin_program = plugin_program_beside(&path);
        executable = path;
    }
    let version = app_version(&executable);
    let (setups, problems) =
        AgentSetups::start(directory.to_path_buf(), plugin_program, version, host);
    for problem in problems {
        send_to_window(window, ServicesOutput::Problem(problem));
    }
    setups
}

fn start_updater(directory: &Path, window: &WindowSender) -> Updater {
    let executable = std::env::current_exe();
    let executable = match executable {
        Ok(path) => path,
        Err(_) => PathBuf::new(),
    };
    let event_window = window.clone();
    let on_event = move |event| {
        let output = match event {
            UpdateEvent::Ready(version) => ServicesOutput::UpdateReady { version },
            UpdateEvent::Problem(text) => ServicesOutput::UpdateProblem(text),
        };
        send_to_window(&event_window, output);
    };
    let quit_window = window.clone();
    let quit = move || send_to_window(&quit_window, ServicesOutput::QuitForUpdate);
    let host = UpdaterHost {
        executable,
        on_event: Box::new(on_event),
        quit: Box::new(quit),
    };
    Updater::start(directory, host)
}

fn start_writer(
    directory: &Path,
    store: &Arc<Slot<ConfigStore>>,
    window: &WindowSender,
) -> ConfigWriter {
    let settings_path = config_file_path(directory, ConfigFile::Settings);
    let keybindings_path = config_file_path(directory, ConfigFile::Keybindings);
    let written_store = Arc::clone(store);
    let after_write = move || {
        let store = locked(&written_store);
        if let Some(store) = store.as_ref() {
            // Published now, so a change shows at once without waiting for the watcher.
            store.refresh_now();
        }
    };
    let refused_window = window.clone();
    let on_refused = move |refusal: Refusal| {
        let refused = match refusal {
            Refusal::Setting { key, reason } => ServicesOutput::SettingRefused { key, reason },
            Refusal::Keymap { command, reason } => {
                ServicesOutput::KeymapRefused { command, reason }
            }
        };
        send_to_window(&refused_window, refused);
    };
    ConfigWriter::start(
        settings_path,
        keybindings_path,
        Box::new(after_write),
        Box::new(on_refused),
    )
}

// One message at a time, each waiting for its own answer, so the workspace hears a session's start before its first state.
fn route_hooks(
    messages: Receiver<Routed>,
    answer_wait: Duration,
    place_wait: Duration,
    resolve: impl Fn(Vec<u32>) -> mpsc::Receiver<Option<RuntimeAnswer>>,
    deliver: impl Fn(HookEvent),
    ask: impl Fn(TerminalId, Question),
    activity_on: impl Fn() -> bool,
) {
    for routed in messages {
        match routed {
            Routed::Post(message) => {
                let answered = resolve(message.pids);
                let answer = answered.recv_timeout(answer_wait);
                if let Ok(Some(RuntimeAnswer::Terminal(Some(terminal)))) = answer {
                    let event = HookEvent {
                        terminal,
                        call: message.call,
                    };
                    deliver(event);
                }
            }
            Routed::Question(question) => {
                // A shell started while Agent Activity was on still loads the plugin; once it is off, no agent is told about the panel.
                if !activity_on() {
                    let _ = question.reply.send(AnswerPart::Placed(false));
                    continue;
                }
                let pids = question.message.pids.clone();
                let answered = resolve(pids);
                let answer = answered.recv_timeout(place_wait);
                if let Ok(Some(RuntimeAnswer::Terminal(Some(terminal)))) = answer {
                    let _ = question.reply.send(AnswerPart::Placed(true));
                    ask(terminal, question);
                } else {
                    let _ = question.reply.send(AnswerPart::Placed(false));
                }
            }
        }
    }
}

fn context_work(answered: mpsc::Receiver<Option<String>>, reply: mpsc::Sender<AnswerPart>) -> Work {
    Box::new(move || {
        let waited = answered.recv_timeout(FORWARD_WAIT);
        let Ok(context) = waited else {
            let _ = reply.send(AnswerPart::Context(None));
            return;
        };
        let _ = reply.send(AnswerPart::Context(context));
    })
}

fn stop_work(
    notes_on: bool,
    reply: mpsc::Sender<AnswerPart>,
    request: impl FnOnce() -> mpsc::Receiver<Option<String>>,
    record: impl FnOnce() + Send + 'static,
) -> Option<Work> {
    if !notes_on {
        let _ = reply.send(AnswerPart::Nudge(None, None));
        return None;
    }
    let answered = request();
    let work: Work = Box::new(move || {
        let waited = answered.recv_timeout(FORWARD_WAIT);
        if let Ok(Some(reason)) = waited {
            let on_delivered: Work = Box::new(record);
            let _ = reply.send(AnswerPart::Nudge(Some(reason), Some(on_delivered)));
        } else {
            let _ = reply.send(AnswerPart::Nudge(None, None));
        }
    });
    Some(work)
}

fn ask_workspace(
    question: Question,
    terminal: TerminalId,
    notes_on: bool,
    workspace: &WorkspaceHost,
) {
    let HookCall::AgentHook {
        harness,
        session_id,
        ..
    } = &question.message.call
    else {
        return;
    };
    let harness = harness.clone();
    let session_id = session_id.clone();
    let reply = question.reply.clone();
    let work = match &question.kind {
        QuestionKind::Context { source } => {
            let source = source.clone();
            let answered = workspace.session_context(terminal, harness, session_id, source);
            Some(context_work(answered, reply))
        }
        QuestionKind::Stop => {
            let asker = workspace.clone();
            let asked_terminal = terminal.clone();
            let asked_harness = harness.clone();
            let asked_session = session_id.clone();
            let request = move || asker.stop_nudge(asked_terminal, asked_harness, asked_session);
            let recorder = workspace.clone();
            let record = move || recorder.asked_for_note(terminal, harness, session_id);
            stop_work(notes_on, reply, request, record)
        }
    };
    let Some(work) = work else {
        return;
    };
    // The route never waits on the workspace; a late answer is dropped by the responder.
    let builder = thread::Builder::new().name("hook-answer".into());
    let _ = builder.spawn(work);
}

fn start_hook_route(
    runtime: RuntimeClient,
    workspace: WorkspaceHost,
    activity_on: Arc<AtomicBool>,
    plugin_heard: PluginHeard,
    ask_notes_on: Arc<AtomicBool>,
    window: &WindowSender,
) -> Sender<Routed> {
    let (inbox, messages) = unbounded();
    let resolve = move |pids| {
        let query = RuntimeQuery::TerminalForPids { pids };
        runtime.ask(query)
    };
    let asking = workspace.clone();
    let delivering_activity_on = Arc::clone(&activity_on);
    let deliver = move |event: HookEvent| {
        let is_on = delivering_activity_on.load(Ordering::SeqCst);
        if is_on {
            // While on, only the plugin's posts get through, so any post means the plugin loaded in this terminal.
            plugin_heard.note(event.terminal.clone());
        }
        workspace.hook(event);
    };
    let ask = move |terminal, question| {
        let notes_on = ask_notes_on.load(Ordering::SeqCst);
        ask_workspace(question, terminal, notes_on, &asking);
    };
    let is_on = move || activity_on.load(Ordering::SeqCst);
    let route = move || {
        route_hooks(
            messages,
            RUNTIME_ANSWER_WAIT,
            taco_protocol::deadlines::QUESTION_PLACE,
            resolve,
            deliver,
            ask,
            is_on,
        )
    };
    let spawned = spawn_supervised("hook-route", ROUTE_STOPPED, window.clone(), route);
    if spawned.is_err() {
        let problem = ROUTE_NOT_STARTED.to_string();
        send_to_window(window, ServicesOutput::Problem(problem));
    }
    inbox
}

impl ServicesHost {
    pub fn start(
        directory: PathBuf,
        system_is_dark: bool,
        runtime: RuntimeClient,
        workspace: WorkspaceHost,
        window: WindowSender,
    ) -> ServicesHost {
        // Copied from each config, so the setups can read it without calling back into the store.
        let activity_on = Arc::new(AtomicBool::new(false));
        let ask_notes_on = Arc::new(AtomicBool::new(false));
        let setups: Arc<Slot<AgentSetups>> = Arc::new(Mutex::new(None));

        let changed_activity_on = Arc::clone(&activity_on);
        let changed_ask_notes_on = Arc::clone(&ask_notes_on);
        let changed_setups = Arc::clone(&setups);
        let changed_window = window.clone();
        let changed_runtime = runtime.clone();
        let on_change = move |config: Config| {
            let is_on = config.settings.agent_activity;
            changed_activity_on.store(is_on, Ordering::SeqCst);
            let asks_notes = config.settings.ask_for_activity_notes;
            changed_ask_notes_on.store(asks_notes, Ordering::SeqCst);
            // New terminals start with this config's settings. Queued for the runtime: this thread waits only if the runtime has stopped reading, for at most `Patience::stalled`, once.
            send_terminal_settings(&changed_runtime, &config);
            send_to_window(&changed_window, ServicesOutput::Config(config));
            let setups = locked(&changed_setups);
            if let Some(setups) = setups.as_ref() {
                setups.activity_changed(is_on);
                setups.refresh();
            }
        };
        let store = ConfigStore::start(directory.clone(), system_is_dark, Box::new(on_change));
        let first = store.current();
        activity_on.store(first.settings.agent_activity, Ordering::SeqCst);
        ask_notes_on.store(first.settings.ask_for_activity_notes, Ordering::SeqCst);

        let store: Arc<Slot<ConfigStore>> = Arc::new(Mutex::new(Some(store)));
        let started_writer = start_writer(&directory, &store, &window);
        let writer: Arc<Slot<ConfigWriter>> = Arc::new(Mutex::new(Some(started_writer)));

        let started = start_setups(&directory, &activity_on, &writer, &runtime, &window);
        let plugin_heard = started.plugin_heard();
        *locked(&setups) = Some(started);

        let updater = start_updater(&directory, &window);
        let updater: Arc<Slot<Updater>> = Arc::new(Mutex::new(Some(updater)));

        let routed_activity_on = Arc::clone(&activity_on);
        let inbox = start_hook_route(
            runtime,
            workspace,
            routed_activity_on,
            plugin_heard,
            ask_notes_on,
            &window,
        );
        let hook_inbox: Arc<Slot<Sender<Routed>>> = Arc::new(Mutex::new(Some(inbox)));
        let routed_inbox = Arc::clone(&hook_inbox);
        let checked_updater = Arc::clone(&updater);
        let on_event = move |event| match event {
            ListenerEvent::Hook(message) => {
                let inbox = locked(&routed_inbox);
                if let Some(inbox) = inbox.as_ref() {
                    let _ = inbox.send(Routed::Post(message));
                }
            }
            ListenerEvent::Question(question) => {
                let inbox = locked(&routed_inbox);
                if let Some(inbox) = inbox.as_ref() {
                    let _ = inbox.send(Routed::Question(question));
                }
            }
            ListenerEvent::UpdateCheck => {
                let updater = locked(&checked_updater);
                if let Some(updater) = updater.as_ref() {
                    updater.check_now();
                }
            }
        };
        let port_file = focus_port_path(&directory);
        let listener = HookListener::start(port_file, Box::new(on_event));
        if listener.port().is_none() {
            send_to_window(&window, ServicesOutput::Problem(PORTS_TAKEN.to_string()));
        }

        let inner = Inner {
            store,
            writer,
            setups,
            updater,
            listener: Mutex::new(Some(listener)),
            hook_inbox,
            last_config: Mutex::new(first),
        };
        ServicesHost {
            inner: Arc::new(inner),
        }
    }

    pub fn config(&self) -> Config {
        let store = locked(&self.inner.store);
        let mut last = locked(&self.inner.last_config);
        if let Some(store) = store.as_ref() {
            *last = store.current();
        }
        last.clone()
    }

    pub fn set_system_dark(&self, dark: bool) {
        let store = locked(&self.inner.store);
        if let Some(store) = store.as_ref() {
            store.set_system_dark(dark);
        }
    }

    pub fn write_setting(&self, change: SettingChange) {
        let writer = locked(&self.inner.writer);
        if let Some(writer) = writer.as_ref() {
            writer.write(ConfigChange::Setting(change));
        }
    }

    pub fn write_keybinding(&self, change: KeymapChange) {
        let writer = locked(&self.inner.writer);
        if let Some(writer) = writer.as_ref() {
            writer.write(ConfigChange::Keymap(change));
        }
    }

    pub fn submitted(&self, terminal: TerminalId) {
        let setups = locked(&self.inner.setups);
        if let Some(setups) = setups.as_ref() {
            setups.note_submit(terminal);
        }
    }

    pub fn exited(&self, terminal: &TerminalId) {
        let setups = locked(&self.inner.setups);
        if let Some(setups) = setups.as_ref() {
            setups.note_exit(terminal);
        }
    }

    pub fn agent_action(&self, harness: &HarnessId, action: AgentMenuAction) {
        let setups = locked(&self.inner.setups);
        if let Some(setups) = setups.as_ref() {
            setups.act(harness, action);
        }
    }

    pub fn dismiss_notice(&self, harness: &HarnessId) {
        let setups = locked(&self.inner.setups);
        if let Some(setups) = setups.as_ref() {
            setups.dismiss(harness);
        }
    }

    pub fn apply_update(&self) {
        let updater = locked(&self.inner.updater);
        if let Some(updater) = updater.as_ref() {
            updater.apply_ready();
        }
    }

    pub fn app_activated(&self) {
        let setups = locked(&self.inner.setups);
        if let Some(setups) = setups.as_ref() {
            setups.recheck_if_too_old();
            setups.refresh();
        }
    }

    // The listener first, so nothing feeds the hook route; dropping its inbox last is what ends the route's thread.
    pub fn stop(&self) {
        let listener = locked(&self.inner.listener).take();
        if let Some(listener) = listener {
            listener.stop();
        }
        let updater = locked(&self.inner.updater).take();
        if let Some(updater) = updater {
            updater.stop();
        }
        let setups = locked(&self.inner.setups).take();
        if let Some(setups) = setups {
            setups.stop();
        }
        let writer = locked(&self.inner.writer).take();
        if let Some(writer) = writer {
            writer.stop();
        }
        let store = locked(&self.inner.store).take();
        if let Some(store) = store {
            store.stop();
        }
        let inbox = locked(&self.inner.hook_inbox).take();
        drop(inbox);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crossbeam_channel::unbounded;
    use taco_protocol::{HarnessId, HookCall, HookEvent, HookMessage, RuntimeAnswer, TerminalId};
    use taco_services::{AnswerPart, Question, QuestionKind};

    use super::{Routed, route_hooks, stop_work};

    const WAIT: Duration = Duration::from_secs(2);
    const SHORT_ANSWER_WAIT: Duration = Duration::from_millis(50);

    fn no_ask(_: TerminalId, _: Question) {}

    fn placed_unless_pid_is_12(pids: Vec<u32>) -> mpsc::Receiver<Option<RuntimeAnswer>> {
        let (answer, answered) = mpsc::channel();
        let found = match pids[0] {
            12 => None,
            pid => Some(terminal(&format!("terminal-{pid}"))),
        };
        answer.send(Some(RuntimeAnswer::Terminal(found))).unwrap();
        answered
    }

    fn question(pid: u32) -> (Question, mpsc::Receiver<AnswerPart>) {
        let (reply, replies) = mpsc::channel();
        let asked = Question {
            kind: QuestionKind::Stop,
            message: message(pid, "s"),
            reply,
        };
        (asked, replies)
    }

    fn call(session: &str) -> HookCall {
        HookCall::AgentEnd {
            harness: HarnessId("claude".to_string()),
            session_id: session.to_string(),
        }
    }

    fn message(pid: u32, session: &str) -> HookMessage {
        let pids = vec![pid];
        let call = call(session);
        HookMessage { pids, call }
    }

    fn terminal(name: &str) -> TerminalId {
        TerminalId(name.to_string())
    }

    #[test]
    fn hook_events_reach_the_workspace_in_arrival_order_and_unresolved_ones_are_dropped() {
        let (inbox, messages) = unbounded();
        for (pid, session) in [(1, "a"), (2, "lost"), (3, "b"), (4, "unanswered")] {
            let next = message(pid, session);
            inbox.send(Routed::Post(next)).unwrap();
        }
        drop(inbox);

        let (handed_out, first_ask) = mpsc::channel();
        let (kept_alive, _never_answered) = mpsc::channel::<mpsc::Sender<Option<RuntimeAnswer>>>();
        let handed_out = Mutex::new(handed_out);
        let kept_alive = Mutex::new(kept_alive);
        let resolve = move |pids: Vec<u32>| {
            let (answer, answered) = mpsc::channel();
            match pids[0] {
                1 => {
                    let sender = handed_out.lock().unwrap();
                    sender.send(answer).unwrap();
                }
                2 => {
                    let none = Some(RuntimeAnswer::Terminal(None));
                    answer.send(none).unwrap();
                }
                3 => {
                    let found = Some(RuntimeAnswer::Terminal(Some(terminal("terminal-b"))));
                    answer.send(found).unwrap();
                }
                _ => {
                    let holder = kept_alive.lock().unwrap();
                    holder.send(answer).unwrap();
                }
            }
            answered
        };
        let delivered = Arc::new(Mutex::new(Vec::new()));
        let delivered_in_route = Arc::clone(&delivered);
        let deliver = move |event: HookEvent| {
            let mut events = delivered_in_route.lock().unwrap();
            events.push(event);
        };

        let route = thread::spawn(move || {
            route_hooks(
                messages,
                SHORT_ANSWER_WAIT,
                SHORT_ANSWER_WAIT,
                resolve,
                deliver,
                no_ask,
                || true,
            )
        });
        let first_answer = first_ask.recv_timeout(WAIT).unwrap();
        let found = Some(RuntimeAnswer::Terminal(Some(terminal("terminal-a"))));
        first_answer.send(found).unwrap();
        route.join().unwrap();

        let delivered = delivered.lock().unwrap().clone();
        let expected = vec![
            HookEvent {
                terminal: terminal("terminal-a"),
                call: call("a"),
            },
            HookEvent {
                terminal: terminal("terminal-b"),
                call: call("b"),
            },
        ];
        assert_eq!(delivered, expected);
    }

    #[test]
    fn a_question_placed_in_a_terminal_is_asked_there_and_an_unplaced_one_is_not() {
        let (inbox, messages) = unbounded();
        let (placed, placed_replies) = question(11);
        let (unplaced, unplaced_replies) = question(12);
        inbox.send(Routed::Question(placed)).unwrap();
        inbox.send(Routed::Question(unplaced)).unwrap();
        drop(inbox);

        let asked = Arc::new(Mutex::new(Vec::new()));
        let asked_in_route = Arc::clone(&asked);
        let ask = move |terminal: TerminalId, question: Question| {
            let mut asked = asked_in_route.lock().unwrap();
            asked.push((terminal, question.message.pids));
        };
        route_hooks(
            messages,
            SHORT_ANSWER_WAIT,
            SHORT_ANSWER_WAIT,
            placed_unless_pid_is_12,
            |_| {},
            ask,
            || true,
        );

        let first = placed_replies.recv_timeout(WAIT).unwrap();
        assert!(matches!(first, AnswerPart::Placed(true)));
        let second = unplaced_replies.recv_timeout(WAIT).unwrap();
        assert!(matches!(second, AnswerPart::Placed(false)));
        let asked = asked.lock().unwrap().clone();
        assert_eq!(asked, vec![(terminal("terminal-11"), vec![11])]);
    }

    #[test]
    fn with_agent_activity_off_a_placed_session_is_answered_not_mine_and_never_asked() {
        let (inbox, messages) = unbounded();
        let (placed, placed_replies) = question(11);
        inbox.send(Routed::Question(placed)).unwrap();
        drop(inbox);

        let ask = |_: TerminalId, _: Question| panic!("the workspace was asked");
        route_hooks(
            messages,
            SHORT_ANSWER_WAIT,
            SHORT_ANSWER_WAIT,
            placed_unless_pid_is_12,
            |_| {},
            ask,
            || false,
        );

        let answer = placed_replies.recv_timeout(WAIT).unwrap();
        assert!(matches!(answer, AnswerPart::Placed(false)));
    }

    #[test]
    fn a_question_is_asked_after_the_posts_queued_before_it_are_delivered() {
        let (inbox, messages) = unbounded();
        let (asked, _replies) = question(2);
        inbox.send(Routed::Post(message(1, "a"))).unwrap();
        inbox.send(Routed::Question(asked)).unwrap();
        inbox.send(Routed::Post(message(3, "b"))).unwrap();
        drop(inbox);

        let log = Arc::new(Mutex::new(Vec::new()));
        let delivered_log = Arc::clone(&log);
        let deliver = move |event: HookEvent| {
            let mut log = delivered_log.lock().unwrap();
            log.push(format!("deliver {}", event.terminal.0));
        };
        let asked_log = Arc::clone(&log);
        let ask = move |terminal: TerminalId, _: Question| {
            let mut log = asked_log.lock().unwrap();
            log.push(format!("ask {}", terminal.0));
        };
        route_hooks(
            messages,
            SHORT_ANSWER_WAIT,
            SHORT_ANSWER_WAIT,
            placed_unless_pid_is_12,
            deliver,
            ask,
            || true,
        );

        let expected = vec!["deliver terminal-1", "ask terminal-2", "deliver terminal-3"];
        assert_eq!(*log.lock().unwrap(), expected);
    }

    #[test]
    fn with_the_notes_switch_off_a_stop_is_not_nudged_and_the_workspace_is_not_asked() {
        let (reply, replies) = mpsc::channel();
        let request = || -> mpsc::Receiver<Option<String>> { panic!("the workspace was asked") };

        let work = stop_work(false, reply, request, || {});

        assert!(work.is_none());
        let part = replies.recv_timeout(WAIT).unwrap();
        assert!(matches!(part, AnswerPart::Nudge(None, None)));
    }

    #[test]
    fn with_the_notes_switch_on_a_reason_is_forwarded_with_its_record() {
        let (reply, replies) = mpsc::channel();
        let (answer, answered) = mpsc::channel();
        answer.send(Some(String::from("write a note"))).unwrap();
        let recorded = Arc::new(Mutex::new(false));
        let recorded_by_hand = Arc::clone(&recorded);
        let record = move || *recorded_by_hand.lock().unwrap() = true;

        let work = stop_work(true, reply, move || answered, record).unwrap();
        work();

        let part = replies.recv_timeout(WAIT).unwrap();
        let AnswerPart::Nudge(Some(reason), Some(on_delivered)) = part else {
            panic!("expected a nudge with a record");
        };
        assert_eq!(reason, "write a note");
        assert!(!*recorded.lock().unwrap());
        on_delivered();
        assert!(*recorded.lock().unwrap());
    }
}
