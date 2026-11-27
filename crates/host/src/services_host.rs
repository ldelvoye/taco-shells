//! The services host: config, agent setup, the hook listener and the updater, wired to the runtime, the workspace and the window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
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
const SERVICES_NOT_STARTED: &str =
    "Agent setups and updates did not start: the thread that runs them did not start.";
const SERVICES_STOPPED: &str =
    "Agent setups and updates stopped after an internal error. Quit and reopen Taco Shells.";
// A runtime that is alive but never answers must not hold a hook, a setup check or quitting for ever.
const RUNTIME_ANSWER_WAIT: Duration = Duration::from_secs(2);
const FORWARD_WAIT: Duration = Duration::from_secs(1);
const SLOW_SETUP: Duration = Duration::from_millis(300);
const SERVICES_PATIENCE: Duration = Duration::from_secs(2);

type Slot<T> = Mutex<Option<T>>;
type Work = Box<dyn FnOnce() + Send>;
type SendSettings = Arc<dyn Fn(&Config) + Send + Sync>;
type OnConfigChange = Box<dyn Fn(Config) + Send + Sync>;

enum Routed {
    Post(HookMessage),
    Question(Question),
}

enum ServicesCommand {
    Settle(Config),
    SetSystemDark(bool),
    ConfigChanged {
        activity_on: bool,
    },
    AgentAction(HarnessId, AgentMenuAction),
    DismissNotice(HarnessId),
    ApplyUpdate,
    CheckForUpdate,
    AppActivated,
    Submitted(TerminalId),
    Exited(TerminalId),
    Stop,
    #[cfg(test)]
    Run(Box<dyn FnOnce() + Send>),
}

#[derive(Clone)]
pub struct ServicesHost {
    inner: Arc<Inner>,
}

struct Inner {
    commands: Sender<ServicesCommand>,
    ended: Slot<Receiver<()>>,
    store: Arc<Slot<ConfigStore>>,
    writer: Arc<Slot<ConfigWriter>>,
    listener: Slot<HookListener>,
    hook_inbox: Arc<Slot<Sender<Routed>>>,
}

struct ServicesThread {
    setups: AgentSetups,
    updater: Updater,
    store: Arc<Slot<ConfigStore>>,
    send_order: Arc<Mutex<()>>,
    send_settings: SendSettings,
    window: WindowSender,
    activity_on: Arc<AtomicBool>,
    ask_notes_on: Arc<AtomicBool>,
}

fn locked<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

fn taken<T>(slot: &Slot<T>) -> Option<T> {
    let mut held = locked(slot);
    held.take()
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

/// The store's change callback. It runs while the store holds its refresh lock, sometimes on the services thread itself, so it only stores, sends and posts.
fn config_callback(
    activity_on: Arc<AtomicBool>,
    ask_notes_on: Arc<AtomicBool>,
    send_order: Arc<Mutex<()>>,
    send_settings: SendSettings,
    window: WindowSender,
    commands: Sender<ServicesCommand>,
) -> OnConfigChange {
    let on_change = move |config: Config| {
        let is_on = config.settings.agent_activity;
        activity_on.store(is_on, Ordering::SeqCst);
        let asks_notes = config.settings.ask_for_activity_notes;
        ask_notes_on.store(asks_notes, Ordering::SeqCst);
        {
            let _order = locked(&send_order);
            // New terminals start with these settings; the start's resend takes the same lock, so the runtime ends on the latest.
            send_settings(&config);
        }
        send_to_window(&window, ServicesOutput::Config(config));
        let changed = ServicesCommand::ConfigChanged { activity_on: is_on };
        let _ = commands.send(changed);
    };
    Box::new(on_change)
}

fn serve(commands: Receiver<ServicesCommand>, mut handle: impl FnMut(ServicesCommand)) {
    for command in commands {
        match command {
            ServicesCommand::Stop => return,
            #[cfg(test)]
            ServicesCommand::Run(work) => work(),
            other => handle(other),
        }
    }
}

/// The store's config if the files changed since `first` was read, with its settings resent and the window told.
fn settled_config(
    store: &Slot<ConfigStore>,
    send_order: &Mutex<()>,
    send_settings: &SendSettings,
    window: &WindowSender,
    first: &Config,
) -> Option<Config> {
    // Every path takes these in one order: the store slot, the store's own refresh lock, then send_order.
    let store = locked(store);
    let _order = locked(send_order);
    let store = store.as_ref()?;
    let current = store.current();
    if current == *first {
        return None;
    }
    send_settings(&current);
    let shown = current.clone();
    send_to_window(window, ServicesOutput::Config(shown));
    Some(current)
}

fn settle(services: &ServicesThread, first: Config) {
    let settled = settled_config(
        &services.store,
        &services.send_order,
        &services.send_settings,
        &services.window,
        &first,
    );
    let Some(current) = settled else {
        return;
    };
    let is_on = current.settings.agent_activity;
    services.activity_on.store(is_on, Ordering::SeqCst);
    let asks_notes = current.settings.ask_for_activity_notes;
    services.ask_notes_on.store(asks_notes, Ordering::SeqCst);
    services.setups.activity_changed(is_on);
    services.setups.refresh();
}

fn set_up_timed(setups: &AgentSetups, harness: &HarnessId, action: AgentMenuAction) {
    let began = Instant::now();
    setups.act(harness, action);
    let took = began.elapsed();
    if took > SLOW_SETUP {
        let millis = took.as_millis();
        eprintln!("the agent setup took {millis} ms");
    }
}

fn handle(services: &ServicesThread, command: ServicesCommand) {
    let setups = &services.setups;
    match command {
        ServicesCommand::Settle(first) => settle(services, first),
        ServicesCommand::SetSystemDark(dark) => {
            let store = locked(&services.store);
            if let Some(store) = store.as_ref() {
                store.set_system_dark(dark);
            }
        }
        ServicesCommand::ConfigChanged { activity_on } => {
            setups.activity_changed(activity_on);
            setups.refresh();
        }
        ServicesCommand::AgentAction(harness, action) => set_up_timed(setups, &harness, action),
        ServicesCommand::DismissNotice(harness) => setups.dismiss(&harness),
        ServicesCommand::ApplyUpdate => services.updater.apply_ready(),
        ServicesCommand::CheckForUpdate => services.updater.check_now(),
        ServicesCommand::AppActivated => {
            setups.recheck_if_too_old();
            setups.refresh();
        }
        ServicesCommand::Submitted(terminal) => setups.note_submit(terminal),
        ServicesCommand::Exited(terminal) => setups.note_exit(&terminal),
        // serve handles these two itself.
        ServicesCommand::Stop => {}
        #[cfg(test)]
        ServicesCommand::Run(_) => {}
    }
}

fn start_services_thread(
    services: ServicesThread,
    commands: Receiver<ServicesCommand>,
    window: &WindowSender,
) -> Receiver<()> {
    // Never sent on: the thread's end, or its panic's unwind, drops `alive`, which closes `ended`.
    let (alive, ended) = bounded::<()>(0);
    let body = move || {
        serve(commands, |command| handle(&services, command));
        let ServicesThread {
            setups, updater, ..
        } = services;
        updater.stop();
        setups.stop();
        drop(alive);
    };
    let spawned = spawn_supervised("services", SERVICES_STOPPED, window.clone(), body);
    if spawned.is_err() {
        let problem = SERVICES_NOT_STARTED.to_string();
        send_to_window(window, ServicesOutput::Problem(problem));
    }
    ended
}

impl ServicesHost {
    pub fn start(
        directory: PathBuf,
        system_is_dark: bool,
        first: Config,
        runtime: RuntimeClient,
        workspace: WorkspaceHost,
        window: WindowSender,
    ) -> ServicesHost {
        // Unbounded: the store's callback posts here while the store holds its lock, sometimes from the services thread itself.
        let (commands, inbox) = unbounded();
        let _ = commands.send(ServicesCommand::Settle(first));
        // Copied from each config, so the setups can read it without calling back into the store.
        let activity_on = Arc::new(AtomicBool::new(false));
        let ask_notes_on = Arc::new(AtomicBool::new(false));
        let send_order = Arc::new(Mutex::new(()));
        let settings_runtime = runtime.clone();
        let send_settings: SendSettings =
            Arc::new(move |config: &Config| send_terminal_settings(&settings_runtime, config));

        let changed_activity_on = Arc::clone(&activity_on);
        let changed_ask_notes_on = Arc::clone(&ask_notes_on);
        let changed_send_order = Arc::clone(&send_order);
        let changed_send_settings = Arc::clone(&send_settings);
        let on_change = config_callback(
            changed_activity_on,
            changed_ask_notes_on,
            changed_send_order,
            changed_send_settings,
            window.clone(),
            commands.clone(),
        );
        let store = ConfigStore::start(directory.clone(), system_is_dark, on_change);
        let current = store.current();
        activity_on.store(current.settings.agent_activity, Ordering::SeqCst);
        ask_notes_on.store(current.settings.ask_for_activity_notes, Ordering::SeqCst);

        let store: Arc<Slot<ConfigStore>> = Arc::new(Mutex::new(Some(store)));
        let started_writer = start_writer(&directory, &store, &window);
        let writer: Arc<Slot<ConfigWriter>> = Arc::new(Mutex::new(Some(started_writer)));

        let setups = start_setups(&directory, &activity_on, &writer, &runtime, &window);
        let plugin_heard = setups.plugin_heard();
        let updater = start_updater(&directory, &window);
        let services = ServicesThread {
            setups,
            updater,
            store: Arc::clone(&store),
            send_order,
            send_settings,
            window: window.clone(),
            activity_on: Arc::clone(&activity_on),
            ask_notes_on: Arc::clone(&ask_notes_on),
        };
        let ended = start_services_thread(services, inbox, &window);

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
        let checked_commands = commands.clone();
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
                let _ = checked_commands.send(ServicesCommand::CheckForUpdate);
            }
        };
        let port_file = focus_port_path(&directory);
        let listener = HookListener::start(port_file, Box::new(on_event));
        if listener.port().is_none() {
            send_to_window(&window, ServicesOutput::Problem(PORTS_TAKEN.to_string()));
        }

        let inner = Inner {
            commands,
            ended: Mutex::new(Some(ended)),
            store,
            writer,
            listener: Mutex::new(Some(listener)),
            hook_inbox,
        };
        ServicesHost {
            inner: Arc::new(inner),
        }
    }

    /// Queues a command for the services thread; never waits.
    fn post(&self, command: ServicesCommand) {
        let _ = self.inner.commands.send(command);
    }

    pub fn set_system_dark(&self, dark: bool) {
        self.post(ServicesCommand::SetSystemDark(dark));
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
        self.post(ServicesCommand::Submitted(terminal));
    }

    pub fn exited(&self, terminal: &TerminalId) {
        let exited = ServicesCommand::Exited(terminal.clone());
        self.post(exited);
    }

    pub fn agent_action(&self, harness: &HarnessId, action: AgentMenuAction) {
        let acted = ServicesCommand::AgentAction(harness.clone(), action);
        self.post(acted);
    }

    pub fn dismiss_notice(&self, harness: &HarnessId) {
        let dismissed = ServicesCommand::DismissNotice(harness.clone());
        self.post(dismissed);
    }

    pub fn apply_update(&self) {
        self.post(ServicesCommand::ApplyUpdate);
    }

    pub fn app_activated(&self) {
        self.post(ServicesCommand::AppActivated);
    }

    // The listener first, so nothing feeds the hook route; then the services thread, which finishes what was queued; the hook inbox last, which ends the route's thread.
    pub fn stop(&self) {
        let listener = taken(&self.inner.listener);
        if let Some(listener) = listener {
            listener.stop();
        }
        self.post(ServicesCommand::Stop);
        let ended = taken(&self.inner.ended);
        if let Some(ended) = ended {
            let waited = ended.recv_timeout(SERVICES_PATIENCE);
            if let Err(RecvTimeoutError::Timeout) = waited {
                eprintln!("the services thread had not stopped; stopping the rest anyway");
            }
        }
        let writer = taken(&self.inner.writer);
        if let Some(writer) = writer {
            writer.stop();
        }
        let store = taken(&self.inner.store);
        if let Some(store) = store {
            store.stop();
        }
        let inbox = taken(&self.inner.hook_inbox);
        drop(inbox);
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
    use futures::channel::mpsc::UnboundedReceiver;
    use taco_protocol::{
        ConfigFile, HarnessId, HookCall, HookEvent, HookMessage, RuntimeAnswer, TerminalId,
    };
    use taco_services::{
        AgentMenuAction, AnswerPart, ConfigStore, ConfigWriter, Question, QuestionKind,
        config_file_path,
    };
    use taco_settings::{Config, ConfigTexts, SettingChange, resolve_config};

    use super::{
        Inner, Routed, SERVICES_PATIENCE, SendSettings, ServicesCommand, ServicesHost, Slot, Work,
        config_callback, locked, route_hooks, serve, settled_config, start_writer, stop_work,
        taken,
    };
    use crate::ports::{ServicesOutput, WindowOutput, WindowSender};

    const WAIT: Duration = Duration::from_secs(2);
    const SHORT_ANSWER_WAIT: Duration = Duration::from_millis(50);
    const AT_ONCE: Duration = Duration::from_millis(250);

    struct TempFolder {
        directory: PathBuf,
    }

    impl Drop for TempFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn temp_folder(name: &str) -> TempFolder {
        let folder = format!("taco-services-host-{}-{name}", std::process::id());
        let directory = std::env::temp_dir().join(folder);
        std::fs::create_dir_all(&directory).unwrap();
        TempFolder { directory }
    }

    fn host_serving(
        commands: Sender<ServicesCommand>,
        inbox: Receiver<ServicesCommand>,
        handle: impl FnMut(ServicesCommand) + Send + 'static,
        store: Arc<Slot<ConfigStore>>,
        writer: Arc<Slot<ConfigWriter>>,
    ) -> ServicesHost {
        let (alive, ended) = bounded::<()>(0);
        thread::spawn(move || {
            serve(inbox, handle);
            drop(alive);
        });
        let inner = Inner {
            commands,
            ended: Mutex::new(Some(ended)),
            store,
            writer,
            listener: Mutex::new(None),
            hook_inbox: Arc::new(Mutex::new(None)),
        };
        ServicesHost {
            inner: Arc::new(inner),
        }
    }

    fn config_services(
        directory: &Path,
        window: &WindowSender,
        commands: &Sender<ServicesCommand>,
    ) -> (Arc<Slot<ConfigStore>>, Arc<Slot<ConfigWriter>>) {
        let activity_on = Arc::new(AtomicBool::new(false));
        let ask_notes_on = Arc::new(AtomicBool::new(false));
        let send_order = Arc::new(Mutex::new(()));
        let send_settings: SendSettings = Arc::new(|_: &Config| {});
        let on_change = config_callback(
            activity_on,
            ask_notes_on,
            send_order,
            send_settings,
            window.clone(),
            commands.clone(),
        );
        let started_store = ConfigStore::start(directory.to_path_buf(), false, on_change);
        let store = Arc::new(Mutex::new(Some(started_store)));
        let started_writer = start_writer(directory, &store, window);
        let writer = Arc::new(Mutex::new(Some(started_writer)));
        (store, writer)
    }

    fn hold(commands: &Sender<ServicesCommand>) -> mpsc::Sender<()> {
        let (held, holding) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let work: Work = Box::new(move || {
            held.send(()).unwrap();
            let _ = released.recv();
        });
        commands.send(ServicesCommand::Run(work)).unwrap();
        holding.recv_timeout(WAIT).unwrap();
        release
    }

    fn wait_until_queued(commands: &Sender<ServicesCommand>, count: usize) {
        let deadline = Instant::now() + WAIT;
        while commands.len() < count {
            assert!(
                Instant::now() < deadline,
                "{count} commands were never queued"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn assert_at_once(what: &str, call: impl FnOnce()) {
        let began = Instant::now();
        call();
        let took = began.elapsed();
        assert!(took < AT_ONCE, "{what} took {took:?}");
    }

    fn label_of(command: &ServicesCommand) -> &'static str {
        match command {
            ServicesCommand::AgentAction(..) => "agent action",
            ServicesCommand::DismissNotice(_) => "dismiss notice",
            ServicesCommand::ApplyUpdate => "apply update",
            ServicesCommand::SetSystemDark(_) => "system dark",
            ServicesCommand::AppActivated => "app activated",
            ServicesCommand::Submitted(_) => "submitted",
            ServicesCommand::Exited(_) => "exited",
            _ => "unexpected",
        }
    }

    fn shows_font_size(shown: &mut UnboundedReceiver<WindowOutput>, size: f64) -> bool {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            let next = shown.try_recv();
            match next {
                Ok(WindowOutput::Services(ServicesOutput::Config(config))) => {
                    if config.settings.font_size == size {
                        return true;
                    }
                }
                Ok(_) => {}
                Err(_) => thread::sleep(Duration::from_millis(5)),
            }
        }
        false
    }

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
    fn a_question_is_asked_in_its_terminal_only_when_placed_and_while_agent_activity_is_on() {
        // Whether Agent Activity is on, which is also whether the placed question is this app's; what the workspace is asked.
        let cases = [
            (true, vec![(terminal("terminal-11"), vec![11])]),
            (false, Vec::new()),
        ];
        for (activity_on, expected_asked) in cases {
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
                move || activity_on,
            );

            let first = placed_replies.recv_timeout(WAIT).unwrap();
            let AnswerPart::Placed(mine) = first else {
                panic!("the placed question is answered with whether it is this app's");
            };
            assert_eq!(mine, activity_on, "activity on: {activity_on}");
            let second = unplaced_replies.recv_timeout(WAIT).unwrap();
            assert!(matches!(second, AnswerPart::Placed(false)));
            let asked = asked.lock().unwrap().clone();
            assert_eq!(asked, expected_asked, "activity on: {activity_on}");
        }
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
    fn a_stop_is_nudged_only_with_the_notes_switch_on_and_its_reason_is_recorded_once_delivered() {
        // Off: answered at once with no nudge, and the workspace is not asked.
        let (reply, replies) = mpsc::channel();
        let request = || -> mpsc::Receiver<Option<String>> { panic!("the workspace was asked") };
        let work = stop_work(false, reply, request, || {});
        assert!(work.is_none());
        let part = replies.recv_timeout(WAIT).unwrap();
        assert!(matches!(part, AnswerPart::Nudge(None, None)));

        // On: the workspace's reason is forwarded, and recorded only once delivered.
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

    #[test]
    fn calls_never_wait_on_a_busy_services_thread_and_a_stop_drains_them_in_order() {
        let (commands, inbox) = unbounded();
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let recording = Arc::clone(&recorded);
        let handle = move |command: ServicesCommand| {
            let label = label_of(&command);
            let mut labels = recording.lock().unwrap();
            labels.push(label);
        };
        let no_store = Arc::new(Mutex::new(None));
        let no_writer = Arc::new(Mutex::new(None));
        let host = host_serving(commands.clone(), inbox, handle, no_store, no_writer);
        let release = hold(&commands);

        let harness = HarnessId("claude".to_string());
        let shell = terminal("terminal-1");
        assert_at_once("an agent action", || {
            host.agent_action(&harness, AgentMenuAction::SetUp)
        });
        assert_at_once("a dismissal", || host.dismiss_notice(&harness));
        assert_at_once("an update", || host.apply_update());
        assert_at_once("the system's appearance", || host.set_system_dark(true));
        assert_at_once("an activation", || host.app_activated());
        assert_at_once("a submit", || host.submitted(shell.clone()));
        assert_at_once("an exit", || host.exited(&shell));

        let stopping = host.clone();
        let stopper = thread::spawn(move || stopping.stop());
        wait_until_queued(&commands, 8);
        thread::sleep(Duration::from_millis(50));
        assert!(
            !stopper.is_finished(),
            "the stop returned while the services thread was still busy"
        );
        release.send(()).unwrap();
        let released = Instant::now();
        stopper.join().unwrap();
        let took = released.elapsed();
        assert!(
            took < SERVICES_PATIENCE / 2,
            "the services thread did not end on the stop: {took:?}"
        );

        let expected = [
            "agent action",
            "dismiss notice",
            "apply update",
            "system dark",
            "app activated",
            "submitted",
            "exited",
        ];
        assert_eq!(*recorded.lock().unwrap(), expected);

        assert_at_once("a call after the stop", || host.apply_update());
        thread::sleep(Duration::from_millis(100));
        assert_eq!(*recorded.lock().unwrap(), expected);
    }

    #[test]
    fn a_config_changed_while_everything_started_is_resent_and_shown_and_an_unchanged_one_is_not() {
        let folder = temp_folder("settled");
        let settings_path = config_file_path(&folder.directory, ConfigFile::Settings);
        std::fs::write(&settings_path, "{\"fontSize\": 19}").unwrap();
        let no_files = ConfigTexts {
            settings: None,
            keybindings: None,
        };
        let read_before_the_edit = resolve_config(&no_files, false);
        let started_store = ConfigStore::start(folder.directory.clone(), false, Box::new(|_| {}));
        let current = started_store.current();
        let store = Mutex::new(Some(started_store));
        let send_order = Mutex::new(());
        let sent = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&sent);
        let send_settings: SendSettings = Arc::new(move |config: &Config| {
            let mut configs = recorded.lock().unwrap();
            configs.push(config.clone());
        });
        let (window, mut shown) = futures::channel::mpsc::unbounded();

        // The files changed after the first read: the settings are resent and the window is told.
        let settled = settled_config(
            &store,
            &send_order,
            &send_settings,
            &window,
            &read_before_the_edit,
        );
        assert_eq!(settled.as_ref(), Some(&current));
        let resent = sent.lock().unwrap().clone();
        assert_eq!(resent, std::slice::from_ref(&current));
        assert!(shows_font_size(&mut shown, 19.0), "the window was not told");

        // Nothing changed: nothing is sent.
        let unchanged = settled_config(&store, &send_order, &send_settings, &window, &current);
        assert_eq!(unchanged, None);
        let sends = sent.lock().unwrap().len();
        assert_eq!(sends, 1);

        let stopped_store = taken(&store);
        if let Some(stopped_store) = stopped_store {
            stopped_store.stop();
        }
    }

    #[test]
    fn a_setting_written_while_the_services_thread_is_held_reaches_the_window_without_it() {
        let folder = temp_folder("written-while-held");
        let (window, mut shown) = futures::channel::mpsc::unbounded();
        let (commands, inbox) = unbounded();
        let (store, writer) = config_services(&folder.directory, &window, &commands);
        let host = host_serving(commands.clone(), inbox, |_| {}, store, writer);
        let release = hold(&commands);

        host.write_setting(SettingChange::FontSize(19.0));
        let arrived = shows_font_size(&mut shown, 19.0);
        assert!(arrived, "the written font size never reached the window");

        release.send(()).unwrap();
        host.stop();
    }

    #[test]
    fn a_write_queued_at_quit_lands_before_stop_returns() {
        let folder = temp_folder("queued-at-quit");
        let (window, _shown) = futures::channel::mpsc::unbounded();
        let (commands, inbox) = unbounded();
        let (store, writer) = config_services(&folder.directory, &window, &commands);
        let host = host_serving(commands.clone(), inbox, |_| {}, store, writer);

        let held_store = locked(&host.inner.store);
        host.write_setting(SettingChange::FontSize(14.0));
        host.write_setting(SettingChange::FontSize(15.0));
        let stopping = host.clone();
        let stopper = thread::spawn(move || stopping.stop());
        thread::sleep(Duration::from_millis(50));
        drop(held_store);
        stopper.join().unwrap();

        let settings_path = config_file_path(&folder.directory, ConfigFile::Settings);
        let written = std::fs::read_to_string(&settings_path).unwrap();
        assert!(written.contains("\"fontSize\": 15"), "{written}");
        assert_at_once("a write after the stop", || {
            host.write_setting(SettingChange::FontSize(16.0))
        });
    }
}
