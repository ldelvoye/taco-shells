//! The services host: config, agent setup, the hook listener and the updater, wired to the runtime, the workspace and the window.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, unbounded};
use taco_protocol::{
    ConfigFile, HarnessId, HookEvent, HookMessage, RuntimeAnswer, RuntimeQuery, TerminalId,
};
use taco_services::{
    AgentMenuAction, AgentSetups, AgentSetupsHost, ConfigChange, ConfigStore, ConfigWriter,
    HookListener, ListenerEvent, Refusal, UpdateEvent, Updater, UpdaterHost, app_version,
    config_file_path, focus_port_path,
};
use taco_settings::{Config, KeymapChange, SettingChange};

use crate::link::{ServicesOutput, WindowOutput, WindowSender};
use crate::runtime_client::RuntimeClient;
use crate::workspace_host::WorkspaceHost;

const PORTS_TAKEN: &str = "Every focus port is taken, so agents' hooks cannot reach this app.";
const ROUTE_NOT_STARTED: &str =
    "Agents' hooks cannot reach the terminals: the thread that routes them did not start.";
// A runtime that is alive but never answers must not hold a hook, a setup check or quitting for ever.
const RUNTIME_ANSWER_WAIT: Duration = Duration::from_secs(2);
const PLUGIN_PROGRAM_NAME: &str = "taco-shells-plugin";

type Slot<T> = Mutex<Option<T>>;

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
    hook_inbox: Arc<Slot<Sender<HookMessage>>>,
    hooks_directory: PathBuf,
    last_config: Mutex<Config>,
}

fn locked<T>(slot: &Mutex<T>) -> MutexGuard<'_, T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner)
}

fn send_to_window(window: &WindowSender, output: ServicesOutput) {
    let _ = window.unbounded_send(WindowOutput::Services(output));
}

fn is_named(path: Option<&Path>, name: &str) -> bool {
    let Some(path) = path else {
        return false;
    };
    let file_name = path.file_name();
    let Some(file_name) = file_name else {
        return false;
    };
    file_name == name
}

fn is_bundle(path: Option<&Path>) -> bool {
    let Some(path) = path else {
        return false;
    };
    let extension = path.extension();
    extension.is_some_and(|found| found == "app")
}

// A packaged app carries its own copy in Resources; a build run from target/ uses the checkout's.
fn hook_scripts_source_for(executable: Option<&Path>, checkout: &Path) -> PathBuf {
    let Some(executable) = executable else {
        return checkout.to_path_buf();
    };
    let macos = executable.parent();
    let contents = macos.and_then(Path::parent);
    let bundle = contents.and_then(Path::parent);
    let in_macos = is_named(macos, "MacOS");
    let in_contents = is_named(contents, "Contents");
    let in_bundle = is_bundle(bundle);
    match contents {
        Some(contents) if in_macos && in_contents && in_bundle => {
            contents.join("Resources").join("hooks")
        }
        _ => checkout.to_path_buf(),
    }
}

fn hook_scripts_source() -> PathBuf {
    let checkout = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../hooks"));
    let executable = std::env::current_exe();
    let found = executable.ok();
    hook_scripts_source_for(found.as_deref(), &checkout)
}

fn plugin_program_beside(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    Some(directory.join(PLUGIN_PROGRAM_NAME))
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
    let source = hook_scripts_source();
    let found = std::env::current_exe();
    let mut executable = PathBuf::new();
    let mut plugin_program = None;
    if let Ok(path) = found {
        plugin_program = plugin_program_beside(&path);
        executable = path;
    }
    let version = app_version(&executable);
    let (setups, problems) = AgentSetups::start(
        directory.to_path_buf(),
        source,
        plugin_program,
        version,
        host,
    );
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
    messages: Receiver<HookMessage>,
    answer_wait: Duration,
    resolve: impl Fn(Vec<u32>) -> mpsc::Receiver<Option<RuntimeAnswer>>,
    deliver: impl Fn(HookEvent),
) {
    for message in messages {
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
}

fn start_hook_route(
    runtime: RuntimeClient,
    workspace: WorkspaceHost,
    window: &WindowSender,
) -> Sender<HookMessage> {
    let (inbox, messages) = unbounded();
    let resolve = move |pids| {
        let query = RuntimeQuery::TerminalForPids { pids };
        runtime.ask(query)
    };
    let deliver = move |event| workspace.hook(event);
    let spawned = thread::Builder::new()
        .name("hook-route".into())
        .spawn(move || route_hooks(messages, RUNTIME_ANSWER_WAIT, resolve, deliver));
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
        let setups: Arc<Slot<AgentSetups>> = Arc::new(Mutex::new(None));

        let changed_activity_on = Arc::clone(&activity_on);
        let changed_setups = Arc::clone(&setups);
        let changed_window = window.clone();
        let on_change = move |config: Config| {
            let is_on = config.settings.agent_activity;
            changed_activity_on.store(is_on, Ordering::SeqCst);
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

        let store: Arc<Slot<ConfigStore>> = Arc::new(Mutex::new(Some(store)));
        let started_writer = start_writer(&directory, &store, &window);
        let writer: Arc<Slot<ConfigWriter>> = Arc::new(Mutex::new(Some(started_writer)));

        let started = start_setups(&directory, &activity_on, &writer, &runtime, &window);
        let hooks_directory = started.hooks_directory();
        *locked(&setups) = Some(started);

        let updater = start_updater(&directory, &window);
        let updater: Arc<Slot<Updater>> = Arc::new(Mutex::new(Some(updater)));

        let inbox = start_hook_route(runtime, workspace, &window);
        let hook_inbox: Arc<Slot<Sender<HookMessage>>> = Arc::new(Mutex::new(Some(inbox)));
        let routed_inbox = Arc::clone(&hook_inbox);
        let checked_updater = Arc::clone(&updater);
        let on_event = move |event| match event {
            ListenerEvent::Hook(message) => {
                let inbox = locked(&routed_inbox);
                if let Some(inbox) = inbox.as_ref() {
                    let _ = inbox.send(message);
                }
            }
            // Not yet routed: every question is answered as not one of the app's terminals, which prints nothing.
            ListenerEvent::Question(question) => {
                let _ = question
                    .reply
                    .send(taco_services::AnswerPart::Placed(false));
            }
            ListenerEvent::UpdateCheck => {
                let updater = locked(&checked_updater);
                if let Some(updater) = updater.as_ref() {
                    updater.check_now();
                }
            }
        };
        let port_file = focus_port_path(&directory);
        let listener = HookListener::start(port_file, activity_on, Box::new(on_event));
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
            hooks_directory,
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

    pub fn hooks_directory(&self) -> PathBuf {
        self.inner.hooks_directory.clone()
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
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use crossbeam_channel::unbounded;
    use taco_protocol::{HarnessId, HookCall, HookEvent, HookMessage, RuntimeAnswer, TerminalId};

    use super::{hook_scripts_source_for, route_hooks};

    const WAIT: Duration = Duration::from_secs(2);
    const SHORT_ANSWER_WAIT: Duration = Duration::from_millis(50);

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
            inbox.send(next).unwrap();
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

        let route =
            thread::spawn(move || route_hooks(messages, SHORT_ANSWER_WAIT, resolve, deliver));
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
    fn a_packaged_app_takes_its_own_hook_scripts_and_any_other_build_the_checkout() {
        let checkout = Path::new("/checkout/hooks");
        let packaged = Path::new("/Applications/Taco Shells.app/Contents/MacOS/taco-shells");
        let from_target = Path::new("/checkout/target/debug/taco-shells");
        let not_a_bundle = Path::new("/tmp/Taco Shells/Contents/MacOS/taco-shells");

        let bundled = hook_scripts_source_for(Some(packaged), checkout);
        let built = hook_scripts_source_for(Some(from_target), checkout);
        let plain = hook_scripts_source_for(Some(not_a_bundle), checkout);
        let unknown = hook_scripts_source_for(None, checkout);

        let resources = PathBuf::from("/Applications/Taco Shells.app/Contents/Resources/hooks");
        assert_eq!(bundled, resources);
        assert_eq!(built, checkout);
        assert_eq!(plain, checkout);
        assert_eq!(unknown, checkout);
    }
}
