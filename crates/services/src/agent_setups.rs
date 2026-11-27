use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use taco_protocol::{HarnessId, TerminalId};

use crate::agent_flow::{
    Agent, agents_of, copy_hook_scripts, hooks_directory, move_old_setup, read_state,
    stub_hook_scripts,
};
use crate::agent_setup_state::{
    AgentMenu, AgentMenuAction, AgentSetupState, agent_menu_of, runs_agent,
};
use crate::claude_version::{MINIMUM_VERSION, claude_version, find_claude};
use crate::config_files::plugin_directory_path;
use crate::hook_listener::TOKEN_FILE_NAME;
use crate::plugin_folder::refresh_plugin_folder;
use crate::sync::lock;

const SUBMIT_CHECK_DELAY: Duration = Duration::from_secs(1);
const DONE_TEXT: &str =
    "Agents already running stop reporting until you start them in a new terminal.";
const OLD_HOOKS_PROBLEM: &str = "settings.json still has the app's old hooks";
const NO_TOKEN_PROBLEM: &str =
    "The app couldn't write its token file, so agents' activity can't reach it.";
const VERSION_THREAD_NAME: &str = "agent-version";

type ForegroundCommands = Box<dyn Fn(&TerminalId) -> Vec<String> + Send + Sync>;
type Version = (u32, u32, u32);
type VersionFinder = Box<dyn Fn() -> Option<Version> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSetupNoticeKind {
    Offer { action: AgentMenuAction },
    Unavailable { reason: String },
    Done,
    Problem,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSetupNotice {
    pub harness: HarnessId,
    pub kind: AgentSetupNoticeKind,
    pub text: String,
    pub terminals: Vec<TerminalId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMenuEntry {
    pub harness: HarnessId,
    pub product_name: String,
    pub menu: AgentMenu,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AgentSetupView {
    pub menus: Vec<AgentMenuEntry>,
    pub notices: Vec<AgentSetupNotice>,
}

pub struct AgentSetupsHost {
    pub is_on: Box<dyn Fn() -> bool + Send + Sync>,
    pub turn_on: Box<dyn Fn() + Send + Sync>,
    pub foreground_commands: ForegroundCommands,
    // Called on the timer thread too, so it must not call back into the service.
    pub on_change: Box<dyn Fn(AgentSetupView) + Send + Sync>,
}

pub(crate) struct SetupFiles {
    pub(crate) config_directory: PathBuf,
    pub(crate) hook_scripts_source: PathBuf,
    pub(crate) plugin_program: PathBuf,
    pub(crate) app_version: String,
}

#[derive(Clone)]
enum Recorded {
    Problem(String),
    Done,
}

enum TimerMessage {
    Schedule(TerminalId, Instant),
    Cancel(TerminalId),
}

enum Next {
    Message(TimerMessage),
    Due,
    Closed,
}

struct Reading<'a> {
    agent: &'a Agent,
    state: AgentSetupState,
}

struct State {
    seen: BTreeMap<HarnessId, BTreeSet<TerminalId>>,
    exited: BTreeSet<TerminalId>,
    recorded: BTreeMap<HarnessId, Recorded>,
    view: AgentSetupView,
    awaiting: bool,
}

struct Shared {
    agents: Vec<Agent>,
    host: AgentSetupsHost,
    files: SetupFiles,
    find_version: VersionFinder,
    version: Mutex<Option<Version>>,
    checking_version: AtomicBool,
    prepared_for: Mutex<bool>,
    state: Mutex<State>,
    // Held from computing the view to publishing it, so two refreshes never publish out of order.
    refresh_order: Mutex<()>,
    stopping: AtomicBool,
}

pub struct AgentSetups {
    shared: Arc<Shared>,
    hooks_directory: PathBuf,
    submit_delay: Duration,
    timer: Option<Sender<TimerMessage>>,
    thread: Option<JoinHandle<()>>,
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn installed_claude_version(home: Option<&Path>) -> Option<Version> {
    let home = home?;
    let claude = find_claude(home)?;
    claude_version(&claude)
}

fn is_awaiting(state: &AgentSetupState) -> bool {
    matches!(state, AgentSetupState::Off | AgentSetupState::OldSetup)
}

fn offer_text(product_name: &str) -> String {
    format!("{product_name} is running here but isn't set up, so its activity can't show.")
}

fn too_old_reason(product_name: &str, version: &str) -> String {
    let (major, minor, patch) = MINIMUM_VERSION;
    format!(
        "Agent Activity needs {product_name} {major}.{minor}.{patch} or later; this Mac has {version}."
    )
}

fn recorded_of(problems: Vec<String>) -> Recorded {
    if problems.is_empty() {
        return Recorded::Done;
    }
    let text = problems.join(" ");
    Recorded::Problem(text)
}

fn notice_of(
    reading: &Reading,
    recorded: Option<&Recorded>,
    seen: Option<&BTreeSet<TerminalId>>,
) -> Option<AgentSetupNotice> {
    let seen = seen?;
    if seen.is_empty() {
        return None;
    }
    let product_name = reading.agent.setup.product_name;
    let harness = reading.agent.harness_id.clone();
    let terminals: Vec<TerminalId> = seen.iter().cloned().collect();
    let notice = |kind: AgentSetupNoticeKind, text: String| {
        Some(AgentSetupNotice {
            harness: harness.clone(),
            kind,
            text,
            terminals: terminals.clone(),
        })
    };

    if let Some(Recorded::Problem(problem)) = recorded {
        return notice(AgentSetupNoticeKind::Problem, problem.clone());
    }
    let is_done = matches!(recorded, Some(Recorded::Done));
    if is_done && reading.state == AgentSetupState::On {
        return notice(AgentSetupNoticeKind::Done, DONE_TEXT.to_string());
    }
    match &reading.state {
        AgentSetupState::Off | AgentSetupState::OldSetup => {
            let kind = AgentSetupNoticeKind::Offer {
                action: AgentMenuAction::SetUp,
            };
            notice(kind, offer_text(product_name))
        }
        AgentSetupState::TooOld { version } => {
            let reason = too_old_reason(product_name, version);
            let kind = AgentSetupNoticeKind::Unavailable { reason };
            notice(kind, offer_text(product_name))
        }
        _ => None,
    }
}

fn current_version(shared: &Shared) -> Option<Version> {
    let version = lock(&shared.version);
    *version
}

fn read_all(shared: &Shared) -> Vec<Reading<'_>> {
    let activity_on = (shared.host.is_on)();
    let version = current_version(shared);
    let mut readings = Vec::new();
    for agent in &shared.agents {
        let state = read_state(agent, activity_on, version);
        readings.push(Reading { agent, state });
    }
    readings
}

fn compute(shared: &Shared) -> (AgentSetupView, bool) {
    let readings = read_all(shared);
    let (seen, recorded) = {
        let state = lock(&shared.state);
        (state.seen.clone(), state.recorded.clone())
    };

    let mut menus = Vec::new();
    let mut notices = Vec::new();
    let mut awaiting = false;
    for reading in &readings {
        let product_name = reading.agent.setup.product_name;
        let menu = agent_menu_of(&reading.state, product_name);
        menus.push(AgentMenuEntry {
            harness: reading.agent.harness_id.clone(),
            product_name: product_name.to_string(),
            menu,
        });
        if is_awaiting(&reading.state) {
            awaiting = true;
        }
        let id = &reading.agent.harness_id;
        let notice = notice_of(reading, recorded.get(id), seen.get(id));
        if let Some(notice) = notice {
            notices.push(notice);
        }
    }
    (AgentSetupView { menus, notices }, awaiting)
}

fn refresh(shared: &Shared) {
    let _order = lock(&shared.refresh_order);
    let (view, awaiting) = compute(shared);
    let changed = {
        let mut state = lock(&shared.state);
        state.awaiting = awaiting;
        if state.view == view {
            false
        } else {
            state.view = view.clone();
            true
        }
    };
    if changed {
        let on_change = &shared.host.on_change;
        on_change(view);
    }
}

fn has_exited(shared: &Shared, terminal: &TerminalId) -> bool {
    let state = lock(&shared.state);
    state.exited.contains(terminal)
}

fn check_terminal(shared: &Shared, terminal: &TerminalId) {
    if has_exited(shared, terminal) {
        return;
    }
    let command_lines = (shared.host.foreground_commands)(terminal);
    let readings = read_all(shared);
    let mut added = false;
    {
        let mut state = lock(&shared.state);
        if state.exited.contains(terminal) {
            return;
        }
        for reading in &readings {
            if !is_awaiting(&reading.state) {
                continue;
            }
            let commands = reading.agent.setup.commands;
            if !runs_agent(&command_lines, commands) {
                continue;
            }
            let id = reading.agent.harness_id.clone();
            let terminals = state.seen.entry(id).or_default();
            let is_new = terminals.insert(terminal.clone());
            if is_new {
                added = true;
            }
        }
    }
    if added {
        refresh(shared);
    }
}

fn next_message(messages: &Receiver<TimerMessage>, earliest: Option<Instant>) -> Next {
    let Some(due) = earliest else {
        return match messages.recv() {
            Ok(message) => Next::Message(message),
            Err(_) => Next::Closed,
        };
    };
    let now = Instant::now();
    let wait = due.saturating_duration_since(now);
    match messages.recv_timeout(wait) {
        Ok(message) => Next::Message(message),
        Err(RecvTimeoutError::Timeout) => Next::Due,
        Err(RecvTimeoutError::Disconnected) => Next::Closed,
    }
}

fn run_due_checks(shared: &Shared, pending: &mut HashMap<TerminalId, Instant>) {
    if shared.stopping.load(Ordering::SeqCst) {
        return;
    }
    let now = Instant::now();
    let mut due = Vec::new();
    for (terminal, at) in pending.iter() {
        if *at <= now {
            due.push(terminal.clone());
        }
    }
    for terminal in &due {
        pending.remove(terminal);
    }
    for terminal in &due {
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        let check = std::panic::AssertUnwindSafe(|| check_terminal(shared, terminal));
        let _ = std::panic::catch_unwind(check);
    }
}

fn timer_loop(shared: Arc<Shared>, messages: Receiver<TimerMessage>) {
    let mut pending: HashMap<TerminalId, Instant> = HashMap::new();
    loop {
        let earliest = pending.values().min().copied();
        let next = next_message(&messages, earliest);
        match next {
            Next::Message(TimerMessage::Schedule(terminal, due)) => {
                let slot = pending.entry(terminal).or_insert(due);
                if due < *slot {
                    *slot = due;
                }
            }
            Next::Message(TimerMessage::Cancel(terminal)) => {
                pending.remove(&terminal);
            }
            Next::Due => run_due_checks(&shared, &mut pending),
            Next::Closed => return,
        }
    }
}

fn find_version_now(shared: &Shared) {
    let found = (shared.find_version)();
    {
        let mut version = lock(&shared.version);
        *version = found;
    }
    shared.checking_version.store(false, Ordering::SeqCst);
    if !shared.stopping.load(Ordering::SeqCst) {
        refresh(shared);
    }
}

fn check_version(shared: &Arc<Shared>) {
    let claimed =
        shared
            .checking_version
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst);
    if claimed.is_err() {
        return;
    }
    let for_thread = Arc::clone(shared);
    let builder = thread::Builder::new().name(VERSION_THREAD_NAME.to_string());
    // Off the caller's thread: this runs the user's login shell, and a slow startup file must cost a late line, never a late window.
    let spawned = builder.spawn(move || find_version_now(&for_thread));
    if spawned.is_err() {
        shared.checking_version.store(false, Ordering::SeqCst);
    }
}

fn prepare_hooks(shared: &Shared, activity_on: bool) -> Vec<String> {
    let files = &shared.files;
    let destination = hooks_directory(&files.config_directory);
    if !activity_on {
        return copy_hook_scripts(&files.hook_scripts_source, &destination);
    }
    let mut problems = Vec::new();
    let plugin_directory = plugin_directory_path(&files.config_directory);
    let refreshed =
        refresh_plugin_folder(&plugin_directory, &files.plugin_program, &files.app_version);
    if let Err(problem) = refreshed {
        problems.push(format!("The agent plugin could not be written: {problem}"));
    }
    let stubbed = stub_hook_scripts(&destination);
    problems.extend(stubbed);
    problems
}

fn set_up(shared: &Arc<Shared>, agent: &Agent, state: &AgentSetupState) -> Vec<String> {
    let mut problems = prepare_hooks(shared, true);
    // The click is the user's own consent, so a development build may make this one write into the agent's settings.
    if *state == AgentSetupState::OldSetup {
        let moved = move_old_setup(agent);
        if let Err(reason) = moved {
            problems.push(format!("{OLD_HOOKS_PROBLEM}: {reason}"));
        }
    }
    (shared.host.turn_on)();
    let token = shared.files.config_directory.join(TOKEN_FILE_NAME);
    let has_token = token.exists();
    if !has_token {
        problems.push(NO_TOKEN_PROBLEM.to_string());
    }
    check_version(shared);
    problems
}

impl AgentSetups {
    pub fn start(
        config_directory: PathBuf,
        hook_scripts_source: PathBuf,
        plugin_program: PathBuf,
        app_version: String,
        host: AgentSetupsHost,
    ) -> (AgentSetups, Vec<String>) {
        let home = std::env::home_dir();
        let agents = match &home {
            Some(home) => agents_of(&config_directory, &real_env, home),
            None => Vec::new(),
        };
        let find_version: VersionFinder =
            Box::new(move || installed_claude_version(home.as_deref()));
        let files = SetupFiles {
            config_directory,
            hook_scripts_source,
            plugin_program,
            app_version,
        };
        AgentSetups::start_with(agents, files, find_version, host, SUBMIT_CHECK_DELAY)
    }

    pub(crate) fn start_with(
        agents: Vec<Agent>,
        files: SetupFiles,
        find_version: VersionFinder,
        host: AgentSetupsHost,
        submit_delay: Duration,
    ) -> (AgentSetups, Vec<String>) {
        let hooks = hooks_directory(&files.config_directory);
        let state = State {
            seen: BTreeMap::new(),
            exited: BTreeSet::new(),
            recorded: BTreeMap::new(),
            view: AgentSetupView::default(),
            awaiting: false,
        };
        let activity_on = (host.is_on)();
        let shared = Shared {
            agents,
            host,
            files,
            find_version,
            version: Mutex::new(None),
            checking_version: AtomicBool::new(false),
            prepared_for: Mutex::new(activity_on),
            state: Mutex::new(state),
            refresh_order: Mutex::new(()),
            stopping: AtomicBool::new(false),
        };

        let problems = prepare_hooks(&shared, activity_on);
        let (view, awaiting) = compute(&shared);
        {
            let mut state = lock(&shared.state);
            state.view = view;
            state.awaiting = awaiting;
        }

        let shared = Arc::new(shared);
        if activity_on {
            check_version(&shared);
        }
        let (sender, messages) = mpsc::channel();
        let for_thread = Arc::clone(&shared);
        let thread = thread::spawn(move || timer_loop(for_thread, messages));
        let setups = AgentSetups {
            shared,
            hooks_directory: hooks,
            submit_delay,
            timer: Some(sender),
            thread: Some(thread),
        };
        (setups, problems)
    }

    pub fn hooks_directory(&self) -> PathBuf {
        self.hooks_directory.clone()
    }

    pub fn view(&self) -> AgentSetupView {
        let state = lock(&self.shared.state);
        state.view.clone()
    }

    pub fn activity_changed(&self, on: bool) {
        {
            let mut prepared_for = lock(&self.shared.prepared_for);
            if *prepared_for == on {
                return;
            }
            *prepared_for = on;
        }
        let problems = prepare_hooks(&self.shared, on);
        if on {
            check_version(&self.shared);
        }
        if problems.is_empty() {
            return;
        }
        let text = problems.join(" ");
        let mut state = lock(&self.shared.state);
        for agent in &self.shared.agents {
            let harness = agent.harness_id.clone();
            let problem = Recorded::Problem(text.clone());
            state.recorded.insert(harness, problem);
        }
    }

    pub fn note_submit(&self, terminal: TerminalId) {
        let awaiting = {
            let state = lock(&self.shared.state);
            state.awaiting
        };
        if !awaiting {
            return;
        }
        let Some(timer) = &self.timer else {
            return;
        };
        let due = Instant::now() + self.submit_delay;
        let _ = timer.send(TimerMessage::Schedule(terminal, due));
    }

    pub fn note_exit(&self, terminal: &TerminalId) {
        {
            let mut state = lock(&self.shared.state);
            state.exited.insert(terminal.clone());
            for terminals in state.seen.values_mut() {
                terminals.remove(terminal);
            }
        }
        if let Some(timer) = &self.timer {
            let _ = timer.send(TimerMessage::Cancel(terminal.clone()));
        }
        refresh(&self.shared);
    }

    #[cfg(test)]
    pub(crate) fn check_terminal(&self, terminal: &TerminalId) {
        check_terminal(&self.shared, terminal);
    }

    pub fn act(&self, harness: &HarnessId, clicked: AgentMenuAction) {
        let found = self
            .shared
            .agents
            .iter()
            .find(|candidate| &candidate.harness_id == harness);
        let Some(agent) = found else {
            return;
        };
        let activity_on = (self.shared.host.is_on)();
        let version = current_version(&self.shared);
        let state = read_state(agent, activity_on, version);
        let product_name = agent.setup.product_name;
        let fresh = agent_menu_of(&state, product_name);
        // The menu may have changed since it was drawn, so a click acts only if it would still be offered.
        if fresh.enabled && fresh.action == clicked {
            let problems = set_up(&self.shared, agent, &state);
            let recorded = recorded_of(problems);
            let mut shared_state = lock(&self.shared.state);
            shared_state.recorded.insert(harness.clone(), recorded);
        }
        refresh(&self.shared);
    }

    pub fn dismiss(&self, harness: &HarnessId) {
        {
            let mut state = lock(&self.shared.state);
            state.recorded.remove(harness);
        }
        refresh(&self.shared);
    }

    pub fn refresh(&self) {
        refresh(&self.shared);
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        self.timer = None;
        let thread = self.thread.take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

impl Drop for AgentSetups {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::AtomicUsize;

    use tempfile::TempDir;

    use crate::agent_hooks::{HookEdit, add_entries};

    use super::*;

    const WAIT: Duration = Duration::from_secs(2);
    const MINE: &str = "{\n  // mine\n  \"model\": \"opus\"\n}\n";
    const STUB: &str = "#!/bin/sh\nexit 0\n";

    struct Environment {
        temp: TempDir,
        agent: Agent,
        agents: Vec<Agent>,
        config: PathBuf,
        program: PathBuf,
    }

    struct Switch {
        on: Arc<AtomicBool>,
        turned_on: Arc<AtomicUsize>,
    }

    struct Fixture {
        environment: Environment,
        setups: AgentSetups,
        commands: Arc<Mutex<HashMap<TerminalId, Vec<String>>>>,
        switch: Switch,
    }

    fn terminal(name: &str) -> TerminalId {
        TerminalId(name.to_string())
    }

    fn claude() -> HarnessId {
        taco_agents::CLAUDE.id()
    }

    fn checkout_hooks() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../hooks"))
    }

    fn environment() -> Environment {
        let temp = TempDir::new().unwrap();
        let config = temp.path().join("config");
        let home = temp.path().join("home");
        let agent_directory = temp.path().join("claude");
        fs::create_dir_all(&agent_directory).unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join(TOKEN_FILE_NAME), "token").unwrap();
        let program = temp.path().join("plugin-program");
        fs::write(&program, "program").unwrap();
        let variable_name = taco_agents::CLAUDE
            .setup()
            .unwrap()
            .config_directory_variable;
        let variable = agent_directory.display().to_string();
        let env = move |name: &str| {
            if name == variable_name {
                Some(variable.clone())
            } else {
                None
            }
        };
        let agents = agents_of(&config, &env, &home);
        let agent = agents[0].clone();
        Environment {
            temp,
            agent,
            agents,
            config,
            program,
        }
    }

    fn files_of(environment: &Environment) -> SetupFiles {
        SetupFiles {
            config_directory: environment.config.clone(),
            hook_scripts_source: checkout_hooks(),
            plugin_program: environment.program.clone(),
            app_version: "1.0.0".to_string(),
        }
    }

    fn switch(on: bool) -> Switch {
        Switch {
            on: Arc::new(AtomicBool::new(on)),
            turned_on: Arc::default(),
        }
    }

    fn start(
        environment: &Environment,
        switch: &Switch,
        foreground_commands: ForegroundCommands,
        on_change: Box<dyn Fn(AgentSetupView) + Send + Sync>,
        submit_delay: Duration,
    ) -> AgentSetups {
        let is_on_flag = Arc::clone(&switch.on);
        let turn_on_flag = Arc::clone(&switch.on);
        let turned_on = Arc::clone(&switch.turned_on);
        let host = AgentSetupsHost {
            is_on: Box::new(move || is_on_flag.load(Ordering::SeqCst)),
            turn_on: Box::new(move || {
                turned_on.fetch_add(1, Ordering::SeqCst);
                turn_on_flag.store(true, Ordering::SeqCst);
            }),
            foreground_commands,
            on_change,
        };
        let files = files_of(environment);
        let find_version: VersionFinder = Box::new(|| None);
        let (setups, _) = AgentSetups::start_with(
            environment.agents.clone(),
            files,
            find_version,
            host,
            submit_delay,
        );
        setups
    }

    fn fixture_with(environment: Environment, on: bool) -> Fixture {
        let commands: Arc<Mutex<HashMap<TerminalId, Vec<String>>>> = Arc::default();
        let commands_map = Arc::clone(&commands);
        let foreground_commands: ForegroundCommands = Box::new(move |id| {
            let map = lock(&commands_map);
            let found = map.get(id);
            match found {
                Some(lines) => lines.clone(),
                None => Vec::new(),
            }
        });
        let switch = switch(on);
        let setups = start(
            &environment,
            &switch,
            foreground_commands,
            Box::new(|_| {}),
            SUBMIT_CHECK_DELAY,
        );
        Fixture {
            environment,
            setups,
            commands,
            switch,
        }
    }

    fn start_timed(
        environment: &Environment,
        switch: &Switch,
        foreground_commands: ForegroundCommands,
    ) -> (AgentSetups, Receiver<AgentSetupView>) {
        let (sender, views) = mpsc::channel();
        let sender = Mutex::new(sender);
        let on_change = Box::new(move |view| {
            let _ = lock(&sender).send(view);
        });
        let setups = start(
            environment,
            switch,
            foreground_commands,
            on_change,
            Duration::ZERO,
        );
        (setups, views)
    }

    fn running_claude(checked: &Arc<Mutex<Vec<TerminalId>>>) -> ForegroundCommands {
        let log = Arc::clone(checked);
        Box::new(move |id| {
            lock(&log).push(id.clone());
            vec!["claude".to_string()]
        })
    }

    fn is_offer(kind: &AgentSetupNoticeKind) -> bool {
        matches!(kind, AgentSetupNoticeKind::Offer { .. })
    }

    fn next_offer_covering(
        views: &Receiver<AgentSetupView>,
        covered: &TerminalId,
    ) -> AgentSetupView {
        loop {
            let received = views.recv_timeout(WAIT);
            let view = received.expect("no offer arrived in time");
            let offered = view.notices.iter().any(|notice| {
                let offers = is_offer(&notice.kind);
                offers && notice.terminals.contains(covered)
            });
            if offered {
                return view;
            }
        }
    }

    fn seen_claude(fixture: &Fixture, name: &str) -> TerminalId {
        let id = terminal(name);
        {
            let mut map = lock(&fixture.commands);
            map.insert(id.clone(), vec!["claude".to_string()]);
        }
        fixture.setups.check_terminal(&id);
        id
    }

    fn only_notice(fixture: &Fixture) -> AgentSetupNotice {
        let view = fixture.setups.view();
        assert_eq!(view.notices.len(), 1, "{:?}", view.notices);
        view.notices[0].clone()
    }

    fn with_entries(text: &str, hooks_directory: &Path) -> String {
        let entries = taco_agents::CLAUDE.setup().unwrap().entries;
        let edit = add_entries(Some(text), entries, hooks_directory, "settings.json");
        let HookEdit::Changed(changed) = edit else {
            panic!("expected the entries to be added, got {edit:?}");
        };
        changed
    }

    fn other_channel(environment: &Environment) -> String {
        let other_config = environment.temp.path().join("other-config");
        let other_hooks = hooks_directory(&other_config);
        with_entries(MINE, &other_hooks)
    }

    fn assert_stubbed(directory: &Path) {
        let checkout = checkout_hooks();
        for entry in fs::read_dir(&checkout).unwrap() {
            let name = entry.unwrap().file_name();
            let text = fs::read_to_string(directory.join(&name)).unwrap();
            assert_eq!(text, STUB, "{name:?}");
        }
    }

    #[test]
    fn the_click_on_an_old_setup_moves_off_only_this_builds_entries() {
        let environment = environment();
        let theirs = other_channel(&environment);
        let ours_too = with_entries(&theirs, &environment.agent.paths.hooks_directory);
        let settings_path = environment.agent.paths.settings_path.clone();
        fs::write(&settings_path, &ours_too).unwrap();
        let fixture = fixture_with(environment, false);
        let seen = seen_claude(&fixture, "one");
        assert!(is_offer(&only_notice(&fixture).kind));

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let after = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(after, theirs);
        let config = &fixture.environment.config;
        let plugin_directory = plugin_directory_path(config);
        let manifest = plugin_directory.join(".claude-plugin/plugin.json");
        assert!(manifest.is_file());
        assert_stubbed(&hooks_directory(config));
        let turned_on = fixture.switch.turned_on.load(Ordering::SeqCst);
        assert_eq!(turned_on, 1);
        let done = only_notice(&fixture);
        assert_eq!(done.kind, AgentSetupNoticeKind::Done);
        assert_eq!(done.terminals, vec![seen]);
    }

    #[test]
    fn the_click_leaves_a_file_without_this_builds_entries_byte_for_byte() {
        let environment = environment();
        let theirs = other_channel(&environment);
        let settings_path = environment.agent.paths.settings_path.clone();
        fs::write(&settings_path, &theirs).unwrap();
        let before = fs::read(&settings_path).unwrap();
        let fixture = fixture_with(environment, false);
        seen_claude(&fixture, "one");

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let after = fs::read(&settings_path).unwrap();
        assert_eq!(after, before);
        assert_eq!(only_notice(&fixture).kind, AgentSetupNoticeKind::Done);
    }

    #[test]
    fn a_missing_token_file_is_a_problem() {
        let environment = environment();
        fs::remove_file(environment.config.join(TOKEN_FILE_NAME)).unwrap();
        let fixture = fixture_with(environment, false);
        seen_claude(&fixture, "one");

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let problem = only_notice(&fixture);
        assert_eq!(problem.kind, AgentSetupNoticeKind::Problem);
        assert_eq!(problem.text, NO_TOKEN_PROBLEM);
    }

    #[test]
    fn turning_activity_on_silences_the_scripts_and_off_brings_them_back() {
        let fixture = fixture_with(environment(), false);
        let hooks = hooks_directory(&fixture.environment.config);

        fixture.setups.activity_changed(true);

        assert_stubbed(&hooks);
        let marker = hooks.join("taco-shells-attention.sh");
        fs::write(&marker, "left alone").unwrap();
        fixture.setups.activity_changed(true);
        assert_eq!(fs::read_to_string(&marker).unwrap(), "left alone");

        fixture.setups.activity_changed(false);

        for entry in fs::read_dir(checkout_hooks()).unwrap() {
            let entry = entry.unwrap();
            let real = fs::read(entry.path()).unwrap();
            let copied = fs::read(hooks.join(entry.file_name())).unwrap();
            assert_eq!(copied, real, "{:?}", entry.file_name());
        }
    }

    #[test]
    fn a_stale_click_does_nothing() {
        let fixture = fixture_with(environment(), true);
        seen_claude(&fixture, "one");

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let turned_on = fixture.switch.turned_on.load(Ordering::SeqCst);
        assert_eq!(turned_on, 0);
        assert!(fixture.setups.view().notices.is_empty());
    }

    #[test]
    fn the_timer_checks_only_terminals_submitted_while_something_awaits() {
        let environment = environment();
        let switch = switch(false);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let first = terminal("first");
        let skipped = terminal("skipped");
        let last = terminal("last");

        setups.note_submit(first.clone());
        let offer_view = next_offer_covering(&views, &first);
        let offer = &offer_view.notices[0];
        assert_eq!(offer.terminals, vec![first.clone()]);

        setups.act(&claude(), AgentMenuAction::SetUp);
        setups.note_submit(skipped.clone());

        switch.on.store(false, Ordering::SeqCst);
        setups.refresh();
        setups.note_submit(last.clone());
        let later_view = next_offer_covering(&views, &last);
        let later = &later_view.notices[0];
        assert_eq!(later.terminals, vec![first.clone(), last.clone()]);

        let log = lock(&checked);
        assert_eq!(*log, vec![first, last]);
    }

    #[test]
    fn a_check_already_scheduled_does_not_bring_a_closed_terminal_back() {
        let environment = environment();
        let switch = switch(false);
        let (started_sender, started) = mpsc::channel();
        let (release_sender, release) = mpsc::channel::<()>();
        let started_sender = Mutex::new(started_sender);
        let release = Mutex::new(release);
        let closed = terminal("closed");
        let open = terminal("open");
        let held = closed.clone();
        let foreground_commands: ForegroundCommands = Box::new(move |id| {
            if *id == held {
                let _ = lock(&started_sender).send(());
                let _ = lock(&release).recv_timeout(WAIT);
            }
            vec!["claude".to_string()]
        });
        let (setups, views) = start_timed(&environment, &switch, foreground_commands);

        setups.note_submit(closed.clone());
        started.recv_timeout(WAIT).expect("the check never started");
        setups.note_exit(&closed);
        release_sender.send(()).unwrap();
        setups.note_submit(open.clone());

        let view = next_offer_covering(&views, &open);
        let offer = &view.notices[0];
        assert_eq!(offer.terminals, vec![open.clone()]);
        let current = setups.view();
        assert_eq!(current.notices[0].terminals, vec![open]);
    }

    #[test]
    fn a_panicking_check_costs_that_check_only() {
        let environment = environment();
        let switch = switch(false);
        let bad = terminal("bad");
        let good = terminal("good");
        let panicking = bad.clone();
        let foreground_commands: ForegroundCommands = Box::new(move |id| {
            if *id == panicking {
                panic!("the foreground lookup failed");
            }
            vec!["claude".to_string()]
        });
        let (setups, views) = start_timed(&environment, &switch, foreground_commands);

        setups.note_submit(bad.clone());
        setups.note_submit(good.clone());

        let view = next_offer_covering(&views, &good);
        let offer = &view.notices[0];
        assert_eq!(offer.terminals, vec![good]);
    }

    #[test]
    fn the_hooks_directory_is_the_one_the_scripts_are_copied_to() {
        let environment = environment();
        let switch = switch(false);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, _views) = start_timed(&environment, &switch, running_claude(&checked));

        let directory = setups.hooks_directory();

        let expected = hooks_directory(&environment.config);
        assert_eq!(directory, expected);
        setups.stop();
    }
}
