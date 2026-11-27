mod notices;
mod timer;
mod version;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use taco_agents::Version;
use taco_protocol::{HarnessId, TerminalId};

use crate::agent_flow::{Agent, agents_of, read_state};
use crate::agent_plugin_dirs::{add_plugin_folder, hides_plugin_folder, remove_plugin_folder};
use crate::agent_setup_state::{AgentMenuAction, AgentSetupState, agent_menu_of};
use crate::config_files::plugin_directory_path;
use crate::plugin_folder::refresh_plugin_folder;
use crate::sync::lock;
pub use notices::{
    AgentMenuEntry, AgentSetupNotice, AgentSetupNoticeKind, AgentSetupView, changed_file_text,
    done_text, offer_text, plugin_dirs_hidden_text, too_old_reason,
};
use notices::{Recorded, not_loaded_notice_of, notice_of, recorded_of};
use taco_protocol::TOKEN_FILE_NAME;
use timer::{TimerMessage, TimerWork, timer_loop};
use version::{check_version, current_version, installed_version, probed_harness};

const SUBMIT_CHECK_DELAY: Duration = Duration::from_secs(1);
const NOT_LOADED_DELAY: Duration = Duration::from_secs(5);
const NO_TOKEN_PROBLEM: &str =
    "The app couldn't write its token file, so agents' activity can't reach it.";
const NO_PROGRAM_PROBLEM: &str = "The app's own plugin program could not be found.";
const SCRIPT_RUNNER: &str = "node";

type ForegroundCommands = Box<dyn Fn(&TerminalId) -> Vec<String> + Send + Sync>;
type VersionFinder = Box<dyn Fn() -> Option<Version> + Send + Sync>;

pub struct AgentSetupsHost {
    pub is_on: Box<dyn Fn() -> bool + Send + Sync>,
    pub turn_on: Box<dyn Fn() + Send + Sync>,
    pub foreground_commands: ForegroundCommands,
    // Called on the timer thread too, so it must not call back into the service.
    pub on_change: Box<dyn Fn(AgentSetupView) + Send + Sync>,
}

pub(crate) struct SetupFiles {
    pub(crate) config_directory: PathBuf,
    pub(crate) plugin_program: Option<PathBuf>,
    pub(crate) app_version: String,
}

struct SetUpOutcome {
    problems: Vec<String>,
    changed_file: Option<String>,
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
    watching: bool,
    heard: BTreeSet<TerminalId>,
    unloaded: BTreeMap<HarnessId, BTreeSet<TerminalId>>,
}

struct Shared {
    agents: Vec<Agent>,
    host: AgentSetupsHost,
    files: SetupFiles,
    find_version: VersionFinder,
    version: Mutex<Option<Version>>,
    checking_version: AtomicBool,
    prepared_for_activity: AtomicBool,
    state: Mutex<State>,
    // Held from computing the view to publishing it, so two refreshes never publish out of order.
    refresh_order: Mutex<()>,
    stopping: AtomicBool,
}

pub struct AgentSetups {
    shared: Arc<Shared>,
    submit_delay: Duration,
    not_loaded_delay: Duration,
    timer: Option<Sender<TimerMessage>>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct PluginHeard {
    shared: Arc<Shared>,
    timer: Option<Sender<TimerMessage>>,
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn is_awaiting(state: &AgentSetupState) -> bool {
    matches!(
        state,
        AgentSetupState::Off | AgentSetupState::PluginDirsHidden { .. }
    )
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

fn compute(shared: &Shared) -> (AgentSetupView, bool, bool) {
    let readings = read_all(shared);
    let (seen, recorded, unloaded) = {
        let state = lock(&shared.state);
        (
            state.seen.clone(),
            state.recorded.clone(),
            state.unloaded.clone(),
        )
    };

    let mut menus = Vec::new();
    let mut notices = Vec::new();
    let mut awaiting = false;
    let mut watching = false;
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
        if reading.state == AgentSetupState::On {
            watching = true;
        }
        let id = &reading.agent.harness_id;
        let notice = notice_of(reading, recorded.get(id), seen.get(id));
        if let Some(notice) = notice {
            notices.push(notice);
        }
        let not_loaded = not_loaded_notice_of(reading, recorded.get(id), unloaded.get(id));
        if let Some(not_loaded) = not_loaded {
            notices.push(not_loaded);
        }
    }
    (AgentSetupView { menus, notices }, awaiting, watching)
}

fn refresh(shared: &Shared) {
    let _order = lock(&shared.refresh_order);
    let (view, awaiting, watching) = compute(shared);
    {
        let mut state = lock(&shared.state);
        state.awaiting = awaiting;
        state.watching = watching;
        if state.view == view {
            return;
        }
        state.view = view.clone();
    }
    let on_change = &shared.host.on_change;
    on_change(view);
}

fn has_exited(shared: &Shared, terminal: &TerminalId) -> bool {
    let state = lock(&shared.state);
    state.exited.contains(terminal)
}

fn base_name_of(word: &str) -> &str {
    let slash = word.rfind('/');
    match slash {
        Some(index) => &word[index + 1..],
        None => word,
    }
}

fn runs_command(command_line: &str, commands: &[&str]) -> bool {
    let words: Vec<&str> = command_line.split_whitespace().collect();
    let Some(first) = words.first() else {
        return false;
    };
    let program = base_name_of(first);
    if commands.contains(&program) {
        return true;
    }
    // A CLI installed through npm shows in the process list as "node <path>/<name>".
    if program != SCRIPT_RUNNER {
        return false;
    }
    let Some(second) = words.get(1) else {
        return false;
    };
    let script = base_name_of(second);
    commands.contains(&script)
}

pub fn runs_agent(command_lines: &[String], commands: &[&str]) -> bool {
    for command_line in command_lines {
        if runs_command(command_line, commands) {
            return true;
        }
    }
    false
}

fn check_terminal(shared: &Shared, terminal: &TerminalId) {
    if has_exited(shared, terminal) {
        return;
    }
    let command_lines = (shared.host.foreground_commands)(terminal);
    let readings = read_all(shared);
    let mut changed = false;
    {
        let mut state = lock(&shared.state);
        if state.exited.contains(terminal) {
            return;
        }
        let heard = state.heard.contains(terminal);
        for reading in &readings {
            let commands = reading.agent.setup.commands;
            let runs = runs_agent(&command_lines, commands);
            let id = reading.agent.harness_id.clone();
            if reading.state == AgentSetupState::On && !runs {
                let unloaded = state.unloaded.get_mut(&id);
                let removed = unloaded.is_some_and(|terminals| terminals.remove(terminal));
                if removed {
                    changed = true;
                }
            }
            if !runs {
                continue;
            }
            if reading.state == AgentSetupState::On && !heard {
                let terminals = state.unloaded.entry(id.clone()).or_default();
                let is_new = terminals.insert(terminal.clone());
                if is_new {
                    changed = true;
                }
            }
            if !is_awaiting(&reading.state) {
                continue;
            }
            let terminals = state.seen.entry(id).or_default();
            let is_new = terminals.insert(terminal.clone());
            if is_new {
                changed = true;
            }
        }
    }
    if changed {
        refresh(shared);
    }
}

fn refresh_plugin(shared: &Shared) -> Vec<String> {
    let files = &shared.files;
    let mut problems = Vec::new();
    match &files.plugin_program {
        Some(program) => {
            let plugin_directory = plugin_directory_path(&files.config_directory);
            let refreshed = refresh_plugin_folder(&plugin_directory, program, &files.app_version);
            if let Err(problem) = refreshed {
                problems.push(format!("The agent plugin could not be written: {problem}"));
            }
        }
        None => problems.push(NO_PROGRAM_PROBLEM.to_string()),
    }
    problems
}

fn set_up(shared: &Arc<Shared>, agent: &Agent) -> SetUpOutcome {
    let mut problems = refresh_plugin(shared);
    if !problems.is_empty() {
        return SetUpOutcome {
            problems,
            changed_file: None,
        };
    }
    let mut changed_file = None;
    // The click is the user's own consent, so a development build may make this one write into the agent's settings.
    if hides_plugin_folder(agent) {
        // Claude Code applies a settings env block over the shell's, so the folder must be in the user's own value to load.
        let added = add_plugin_folder(agent, &shared.files.config_directory);
        match added {
            Ok(written) => changed_file = written,
            Err(problem) => {
                problems.push(problem);
                return SetUpOutcome {
                    problems,
                    changed_file: None,
                };
            }
        }
    }
    shared.prepared_for_activity.store(true, Ordering::SeqCst);
    (shared.host.turn_on)();
    let token = shared.files.config_directory.join(TOKEN_FILE_NAME);
    let has_token = token.exists();
    if !has_token {
        problems.push(NO_TOKEN_PROBLEM.to_string());
    }
    check_version(shared);
    SetUpOutcome {
        problems,
        changed_file,
    }
}

impl AgentSetups {
    pub fn start(
        config_directory: PathBuf,
        plugin_program: Option<PathBuf>,
        app_version: String,
        host: AgentSetupsHost,
    ) -> (AgentSetups, Vec<String>) {
        let home = std::env::home_dir();
        let agents = match &home {
            Some(home) => agents_of(&config_directory, &real_env, home),
            None => Vec::new(),
        };
        let find_version: VersionFinder = Box::new(move || {
            let harness = probed_harness()?;
            installed_version(home.as_deref(), harness)
        });
        let files = SetupFiles {
            config_directory,
            plugin_program,
            app_version,
        };
        AgentSetups::start_with(
            agents,
            files,
            find_version,
            host,
            SUBMIT_CHECK_DELAY,
            NOT_LOADED_DELAY,
        )
    }

    pub(crate) fn start_with(
        agents: Vec<Agent>,
        files: SetupFiles,
        find_version: VersionFinder,
        host: AgentSetupsHost,
        submit_delay: Duration,
        not_loaded_delay: Duration,
    ) -> (AgentSetups, Vec<String>) {
        let state = State {
            seen: BTreeMap::new(),
            exited: BTreeSet::new(),
            recorded: BTreeMap::new(),
            view: AgentSetupView::default(),
            awaiting: false,
            watching: false,
            heard: BTreeSet::new(),
            unloaded: BTreeMap::new(),
        };
        let activity_on = (host.is_on)();
        let shared = Shared {
            agents,
            host,
            files,
            find_version,
            version: Mutex::new(None),
            checking_version: AtomicBool::new(false),
            prepared_for_activity: AtomicBool::new(activity_on),
            state: Mutex::new(state),
            refresh_order: Mutex::new(()),
            stopping: AtomicBool::new(false),
        };

        let mut problems = Vec::new();
        if activity_on {
            let refreshed = refresh_plugin(&shared);
            problems.extend(refreshed);
        }
        let (view, awaiting, watching) = compute(&shared);
        {
            let mut state = lock(&shared.state);
            state.view = view;
            state.awaiting = awaiting;
            state.watching = watching;
        }

        let shared = Arc::new(shared);
        if activity_on {
            check_version(&shared);
        }
        let (sender, messages) = mpsc::channel();
        let for_stopping = Arc::clone(&shared);
        let for_check = Arc::clone(&shared);
        let for_refresh = Arc::clone(&shared);
        let work = TimerWork {
            stopping: Box::new(move || for_stopping.stopping.load(Ordering::SeqCst)),
            check: Box::new(move |terminal| check_terminal(&for_check, terminal)),
            refresh: Box::new(move || refresh(&for_refresh)),
        };
        let thread = thread::spawn(move || timer_loop(work, messages));
        let setups = AgentSetups {
            shared,
            submit_delay,
            not_loaded_delay,
            timer: Some(sender),
            thread: Some(thread),
        };
        (setups, problems)
    }

    pub fn view(&self) -> AgentSetupView {
        let state = lock(&self.shared.state);
        state.view.clone()
    }

    pub fn activity_changed(&self, on: bool) {
        let was_prepared = self.shared.prepared_for_activity.swap(on, Ordering::SeqCst);
        if was_prepared == on {
            return;
        }
        {
            let mut state = lock(&self.shared.state);
            state.heard.clear();
            state.unloaded.clear();
        }
        let mut problems = Vec::new();
        if on {
            let refreshed = refresh_plugin(&self.shared);
            problems.extend(refreshed);
            check_version(&self.shared);
        } else {
            // Only while the line is still exactly what this app wrote; otherwise it is the user's.
            let config_directory = &self.shared.files.config_directory;
            for agent in &self.shared.agents {
                let variable = agent.setup.plugin_variable;
                let taken_out = remove_plugin_folder(config_directory, variable);
                if let Err(problem) = taken_out {
                    problems.push(problem);
                }
            }
            let mut state = lock(&self.shared.state);
            state
                .recorded
                .retain(|_, recorded| !matches!(recorded, Recorded::Done { .. }));
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

    pub fn recheck_if_too_old(&self) {
        let activity_on = (self.shared.host.is_on)();
        let version = current_version(&self.shared);
        let mut is_too_old = false;
        for agent in &self.shared.agents {
            let state = read_state(agent, activity_on, version);
            if matches!(state, AgentSetupState::TooOld { .. }) {
                is_too_old = true;
            }
        }
        if is_too_old {
            check_version(&self.shared);
        }
    }

    pub fn note_submit(&self, terminal: TerminalId) {
        let (awaiting, watching) = {
            let state = lock(&self.shared.state);
            (state.awaiting, state.watching)
        };
        if !awaiting && !watching {
            return;
        }
        let mut delay = self.not_loaded_delay;
        if awaiting {
            delay = self.submit_delay;
        }
        let Some(timer) = &self.timer else {
            return;
        };
        let due = Instant::now() + delay;
        let _ = timer.send(TimerMessage::Schedule(terminal, due));
    }

    pub fn plugin_heard(&self) -> PluginHeard {
        PluginHeard {
            shared: Arc::clone(&self.shared),
            timer: self.timer.clone(),
        }
    }

    pub fn note_exit(&self, terminal: &TerminalId) {
        {
            let mut state = lock(&self.shared.state);
            state.exited.insert(terminal.clone());
            state.heard.remove(terminal);
            for terminals in state.seen.values_mut() {
                terminals.remove(terminal);
            }
            for terminals in state.unloaded.values_mut() {
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
            let outcome = set_up(&self.shared, agent);
            let recorded = recorded_of(outcome.problems, outcome.changed_file);
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
        if let Some(timer) = &self.timer {
            let _ = timer.send(TimerMessage::Stop);
        }
        self.timer = None;
        let thread = self.thread.take();
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

impl PluginHeard {
    pub fn note(&self, terminal: TerminalId) {
        let removed = {
            let mut state = lock(&self.shared.state);
            state.heard.insert(terminal.clone());
            let mut removed = false;
            for terminals in state.unloaded.values_mut() {
                if terminals.remove(&terminal) {
                    removed = true;
                }
            }
            removed
        };
        if !removed {
            return;
        }
        // Handed to the timer thread: the hook route must never wait on a refresh and its settings-file reads.
        if let Some(timer) = &self.timer {
            let _ = timer.send(TimerMessage::Refresh);
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
    use std::collections::HashMap;
    use std::fs;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::Receiver;

    use tempfile::TempDir;

    use crate::plugin_dirs::read_record;

    use super::notices::{NOT_LOADED_REASON, not_loaded_text};
    use super::*;

    const HIDDEN: &str = "{\n  // mine\n  \"env\": {\n    \"CLAUDE_CODE_PLUGIN_DIRS\": \"~/.claude/mods/face-band\"\n  },\n  \"model\": \"opus\"\n}\n";
    const UNEDITABLE: &str =
        "{\"env\": {\"A\": \"1\"}, \"env\": {\"CLAUDE_CODE_PLUGIN_DIRS\": \"/a\"}}";
    const WAIT: Duration = Duration::from_secs(2);

    struct Environment {
        _temp: TempDir,
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
            if name != variable_name {
                return None;
            }
            Some(variable.clone())
        };
        let mut agents = agents_of(&config, &env, &home);
        for agent in &mut agents {
            agent.paths.managed_settings_path = temp.path().join("managed-settings.json");
        }
        let agent = agents[0].clone();
        Environment {
            _temp: temp,
            agent,
            agents,
            config,
            program,
        }
    }

    fn files_of(environment: &Environment) -> SetupFiles {
        SetupFiles {
            config_directory: environment.config.clone(),
            plugin_program: Some(environment.program.clone()),
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
        not_loaded_delay: Duration,
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
            not_loaded_delay,
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
            NOT_LOADED_DELAY,
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

    fn is_not_loaded(notice: &AgentSetupNotice) -> bool {
        matches!(notice.kind, AgentSetupNoticeKind::Unavailable { .. })
    }

    fn next_not_loaded_covering(
        views: &Receiver<AgentSetupView>,
        covered: &TerminalId,
    ) -> AgentSetupNotice {
        loop {
            let received = views.recv_timeout(WAIT);
            let view = received.expect("no not-loaded notice arrived in time");
            let found = view
                .notices
                .iter()
                .find(|notice| is_not_loaded(notice) && notice.terminals.contains(covered));
            if let Some(notice) = found {
                return notice.clone();
            }
        }
    }

    fn wait_until_checked(checked: &Arc<Mutex<Vec<TerminalId>>>, id: &TerminalId) {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if lock(checked).contains(id) {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the check never ran");
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

    fn hidden_environment() -> Environment {
        let environment = environment();
        let settings_path = environment.agent.paths.settings_path.clone();
        fs::write(&settings_path, HIDDEN).unwrap();
        environment
    }

    fn appended_settings(environment: &Environment) -> String {
        let folder = plugin_directory_path(&environment.config);
        let shown = folder.display();
        HIDDEN.replace("face-band\"", &format!("face-band:{shown}\""))
    }

    #[test]
    fn a_hidden_plugin_is_offered_while_on_and_the_click_adds_the_folder_until_off() {
        let environment = hidden_environment();
        let settings_path = environment.agent.paths.settings_path.clone();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let id = terminal("one");

        setups.note_submit(id.clone());

        let offer_view = next_offer_covering(&views, &id);
        let product_name = environment.agent.setup.product_name;
        let shown = settings_path.display().to_string();
        let variable = environment.agent.setup.plugin_variable;
        let expected = plugin_dirs_hidden_text(product_name, variable, &shown);
        assert_eq!(offer_view.notices[0].text, expected);
        let menu = &offer_view.menus[0].menu;
        assert_eq!(menu.status, "Its settings hide the plugin");
        assert!(menu.enabled);

        setups.act(&claude(), AgentMenuAction::SetUp);

        let after_click = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(after_click, appended_settings(&environment));
        let folder = plugin_directory_path(&environment.config);
        let record = read_record(&environment.config).unwrap();
        assert_eq!(record.before, "~/.claude/mods/face-band");
        assert_eq!(
            record.after,
            format!("~/.claude/mods/face-band:{}", folder.display())
        );
        assert_eq!(switch.turned_on.load(Ordering::SeqCst), 1);
        let done = setups.view().notices[0].clone();
        assert_eq!(done.kind, AgentSetupNoticeKind::Done);
        assert_eq!(done.detail, Some(changed_file_text(&shown)));
        setups.refresh();
        assert_eq!(setups.view().menus[0].menu.status, "On");

        switch.on.store(false, Ordering::SeqCst);
        setups.activity_changed(false);

        let after_off = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(after_off, HIDDEN);
        assert_eq!(read_record(&environment.config), None);
    }

    #[test]
    fn off_leaves_a_line_the_user_edited_alone_and_drops_the_record() {
        let environment = hidden_environment();
        let settings_path = environment.agent.paths.settings_path.clone();
        let fixture = fixture_with(environment, false);
        seen_claude(&fixture, "one");
        fixture.setups.act(&claude(), AgentMenuAction::SetUp);
        let edited = HIDDEN.replace("face-band", "other");
        fs::write(&settings_path, &edited).unwrap();

        fixture.setups.activity_changed(false);

        let after = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(after, edited);
        assert_eq!(read_record(&fixture.environment.config), None);
    }

    #[test]
    fn a_refused_append_stops_the_click_before_turning_on() {
        let environment = environment();
        let settings_path = environment.agent.paths.settings_path.clone();
        fs::write(&settings_path, UNEDITABLE).unwrap();
        let fixture = fixture_with(environment, false);
        seen_claude(&fixture, "one");

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let turned_on = fixture.switch.turned_on.load(Ordering::SeqCst);
        assert_eq!(turned_on, 0);
        let problem = only_notice(&fixture);
        assert_eq!(problem.kind, AgentSetupNoticeKind::Problem);
        assert!(
            problem.text.contains("could not be added to"),
            "{}",
            problem.text
        );
        assert_eq!(read_record(&fixture.environment.config), None);
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
    fn a_stale_click_does_nothing() {
        let fixture = fixture_with(environment(), true);
        seen_claude(&fixture, "one");

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let turned_on = fixture.switch.turned_on.load(Ordering::SeqCst);
        assert_eq!(turned_on, 0);
        let notices = fixture.setups.view().notices;
        assert!(!notices.is_empty());
        assert!(notices.iter().all(is_not_loaded), "{notices:?}");
    }

    #[test]
    fn the_timer_checks_the_terminals_submitted_while_something_awaits() {
        let environment = environment();
        let switch = switch(false);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let first = terminal("first");
        let last = terminal("last");

        setups.note_submit(first.clone());
        let offer_view = next_offer_covering(&views, &first);
        let offer = &offer_view.notices[0];
        assert_eq!(offer.terminals, vec![first.clone()]);

        setups.act(&claude(), AgentMenuAction::SetUp);

        switch.on.store(false, Ordering::SeqCst);
        setups.activity_changed(false);
        setups.refresh();
        setups.note_submit(last.clone());
        let later_view = next_offer_covering(&views, &last);
        let later = &later_view.notices[0];
        assert_eq!(later.terminals, vec![first.clone(), last.clone()]);

        let log = lock(&checked);
        assert_eq!(*log, vec![first, last]);
    }

    #[test]
    fn a_submit_while_nothing_awaits_or_watches_is_never_checked() {
        let environment = environment();
        fs::remove_dir_all(&environment.agent.paths.agent_directory).unwrap();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, _views) = start_timed(&environment, &switch, running_claude(&checked));

        setups.note_submit(terminal("skipped"));
        thread::sleep(Duration::from_millis(200));

        let log = lock(&checked);
        assert!(log.is_empty(), "{log:?}");
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
                let _released = lock(&release).recv_timeout(WAIT);
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
    fn a_running_agent_that_never_reported_gets_a_notice_until_its_plugin_is_heard() {
        let environment = environment();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let id = terminal("one");

        setups.note_submit(id.clone());

        let notice = next_not_loaded_covering(&views, &id);
        let product_name = environment.agent.setup.product_name;
        let expected = not_loaded_text(product_name);
        assert_eq!(notice.text, expected);
        let reason = NOT_LOADED_REASON.to_string();
        assert_eq!(notice.kind, AgentSetupNoticeKind::Unavailable { reason });
        assert_eq!(notice.terminals, vec![id.clone()]);

        let held = lock(&setups.shared.refresh_order);
        let heard = setups.plugin_heard();
        let (returned, noted) = mpsc::channel();
        let noting = thread::spawn(move || {
            heard.note(id);
            let _ = returned.send(());
        });
        let came_back = noted.recv_timeout(WAIT);
        assert!(came_back.is_ok(), "note waited for a refresh in progress");
        drop(held);

        loop {
            let received = views.recv_timeout(WAIT);
            let view = received.expect("the notice was never cleared");
            if view.notices.is_empty() {
                break;
            }
        }
        noting.join().unwrap();
    }

    #[test]
    fn stopping_returns_while_the_route_still_holds_its_plugin_heard() {
        let environment = environment();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, _views) = start_timed(&environment, &switch, running_claude(&checked));
        let kept_by_the_route = setups.plugin_heard();

        let (stopped, came_back) = mpsc::channel();
        let stopping = thread::spawn(move || {
            setups.stop();
            let _ = stopped.send(());
        });
        let returned = came_back.recv_timeout(WAIT);
        assert!(returned.is_ok(), "stop never returned");
        stopping.join().unwrap();
        drop(kept_by_the_route);
    }

    #[test]
    fn a_plugin_heard_before_the_check_never_gets_a_notice() {
        let environment = environment();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let heard = terminal("heard");
        let silent = terminal("silent");

        setups.plugin_heard().note(heard.clone());
        setups.note_submit(heard.clone());
        wait_until_checked(&checked, &heard);
        setups.note_submit(silent.clone());

        let notice = next_not_loaded_covering(&views, &silent);
        assert_eq!(notice.terminals, vec![silent]);
        assert_eq!(setups.view().notices.len(), 1);
    }

    #[test]
    fn switching_off_clears_a_not_loaded_notice() {
        let environment = environment();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let id = terminal("one");
        setups.note_submit(id.clone());
        next_not_loaded_covering(&views, &id);

        switch.on.store(false, Ordering::SeqCst);
        setups.activity_changed(false);
        setups.refresh();
        assert!(setups.view().notices.is_empty());

        switch.on.store(true, Ordering::SeqCst);
        setups.activity_changed(true);
        setups.refresh();
        assert!(setups.view().notices.is_empty());
    }

    #[test]
    fn flipping_the_switch_forgets_which_plugins_were_heard() {
        let environment = environment();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let id = terminal("one");
        setups.plugin_heard().note(id.clone());

        switch.on.store(false, Ordering::SeqCst);
        setups.activity_changed(false);
        setups.refresh();
        switch.on.store(true, Ordering::SeqCst);
        setups.activity_changed(true);
        setups.refresh();
        setups.note_submit(id.clone());

        let notice = next_not_loaded_covering(&views, &id);
        assert_eq!(notice.terminals, vec![id]);
    }

    #[test]
    fn closing_a_terminal_clears_its_not_loaded_notice() {
        let environment = environment();
        let switch = switch(true);
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, &switch, running_claude(&checked));
        let id = terminal("one");
        setups.note_submit(id.clone());
        next_not_loaded_covering(&views, &id);

        setups.note_exit(&id);

        assert!(setups.view().notices.is_empty());
    }

    #[test]
    fn a_terminal_whose_agent_quit_loses_its_not_loaded_notice() {
        let environment = environment();
        let switch = switch(true);
        let running = Arc::new(AtomicBool::new(true));
        let still_running = Arc::clone(&running);
        let foreground_commands: ForegroundCommands = Box::new(move |_| {
            if still_running.load(Ordering::SeqCst) {
                return vec!["claude".to_string()];
            }
            Vec::new()
        });
        let (setups, views) = start_timed(&environment, &switch, foreground_commands);
        let id = terminal("one");
        setups.note_submit(id.clone());
        next_not_loaded_covering(&views, &id);

        running.store(false, Ordering::SeqCst);
        setups.note_submit(id);

        loop {
            let received = views.recv_timeout(WAIT);
            let view = received.expect("the notice never cleared");
            if view.notices.is_empty() {
                break;
            }
        }
    }

    #[test]
    fn a_finished_setup_hides_the_not_loaded_notice() {
        let fixture = fixture_with(environment(), false);
        seen_claude(&fixture, "one");
        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        seen_claude(&fixture, "two");

        let unloaded = lock(&fixture.setups.shared.state).unloaded.clone();
        assert!(unloaded.values().any(|terminals| !terminals.is_empty()));
        let notices = fixture.setups.view().notices;
        assert!(!notices.iter().any(is_not_loaded), "{notices:?}");
    }

    #[test]
    fn a_click_that_cannot_prepare_the_plugin_leaves_the_agents_settings_alone() {
        let environment = hidden_environment();
        let settings_path = environment.agent.paths.settings_path.clone();
        fs::remove_file(&environment.program).unwrap();
        let fixture = fixture_with(environment, false);
        seen_claude(&fixture, "one");

        fixture.setups.act(&claude(), AgentMenuAction::SetUp);

        let after = fs::read_to_string(&settings_path).unwrap();
        assert_eq!(after, HIDDEN);
        let turned_on = fixture.switch.turned_on.load(Ordering::SeqCst);
        assert_eq!(turned_on, 0);
        assert_eq!(only_notice(&fixture).kind, AgentSetupNoticeKind::Problem);
    }

    #[test]
    fn an_agent_runs_as_its_program_or_as_node_running_its_script() {
        let commands = ["claude"];
        let runs = [
            "claude --resume x",
            "node /usr/lib/node_modules/@anthropic-ai/claude-code/claude",
            "/opt/bin/claude",
            "node /x/claude",
        ];
        for line in runs {
            let lines = [line.to_string()];
            assert!(runs_agent(&lines, &commands), "{line}");
        }
        let does_not_run = ["vim claude.md", "node", "", "node /x/cli.js"];
        for line in does_not_run {
            let lines = [line.to_string()];
            assert!(!runs_agent(&lines, &commands), "{line}");
        }
    }
}
