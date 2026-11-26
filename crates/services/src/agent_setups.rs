use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use taco_protocol::{HarnessId, TerminalId};

use crate::agent_flow::{
    Agent, agents_of, can_write_for, copy_hook_scripts, hooks_directory, keep_current, read_state,
    remove_setup, set_up, switch_to_managed,
};
use crate::agent_setup_state::{
    AgentMenu, AgentMenuAction, AgentSetupState, NOTE_BUILD_MAY_NOT_WRITE, STATUS_OFF,
    agent_menu_of, runs_agent,
};
use crate::identity::IS_STABLE;
use crate::sync::lock;

const SUBMIT_CHECK_DELAY: Duration = Duration::from_secs(1);

type ForegroundCommands = Box<dyn Fn(&TerminalId) -> Vec<String> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSetupNoticeKind {
    Offer,
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
    pub is_managing: Box<dyn Fn() -> bool + Send + Sync>,
    pub foreground_commands: ForegroundCommands,
    // Called on the timer thread too, so it must not call back into the service.
    pub on_change: Box<dyn Fn(AgentSetupView) + Send + Sync>,
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
    may_write: bool,
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
    can_write: Box<dyn Fn(&Agent) -> bool + Send + Sync>,
    host: AgentSetupsHost,
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

fn is_awaiting(state: AgentSetupState) -> bool {
    matches!(
        state,
        AgentSetupState::NotSetUp | AgentSetupState::Declined | AgentSetupState::Off
    )
}

fn offer_text(product_name: &str) -> String {
    format!("{product_name} is running here but isn't set up, so its activity can't show.")
}

fn done_text(product_name: &str) -> String {
    format!("{product_name} is set up. Its activity shows here from its next session.")
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
    if is_done && reading.state == AgentSetupState::SetUp {
        return notice(AgentSetupNoticeKind::Done, done_text(product_name));
    }
    match reading.state {
        AgentSetupState::Off => {
            let kind = AgentSetupNoticeKind::Unavailable {
                reason: STATUS_OFF.to_string(),
            };
            notice(kind, offer_text(product_name))
        }
        AgentSetupState::NotSetUp | AgentSetupState::Declined => {
            if reading.may_write {
                notice(AgentSetupNoticeKind::Offer, offer_text(product_name))
            } else {
                let kind = AgentSetupNoticeKind::Unavailable {
                    reason: NOTE_BUILD_MAY_NOT_WRITE.to_string(),
                };
                notice(kind, offer_text(product_name))
            }
        }
        _ => None,
    }
}

fn read_all<'a>(shared: &'a Shared) -> Vec<Reading<'a>> {
    let managing = (shared.host.is_managing)();
    let mut readings = Vec::new();
    for agent in &shared.agents {
        let state = read_state(agent, managing);
        let may_write = (shared.can_write)(agent);
        readings.push(Reading {
            agent,
            state,
            may_write,
        });
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
        let menu = agent_menu_of(reading.state, product_name, reading.may_write);
        menus.push(AgentMenuEntry {
            harness: reading.agent.harness_id.clone(),
            product_name: product_name.to_string(),
            menu,
        });
        if is_awaiting(reading.state) {
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
            if !is_awaiting(reading.state) {
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

fn perform(agent: &Agent, action: AgentMenuAction) -> Result<(), String> {
    match action {
        AgentMenuAction::SetUp => set_up(agent),
        AgentMenuAction::Remove => remove_setup(agent),
        AgentMenuAction::Switch => switch_to_managed(agent),
    }
}

impl AgentSetups {
    pub fn start(
        config_directory: PathBuf,
        hook_scripts_source: PathBuf,
        host: AgentSetupsHost,
    ) -> (AgentSetups, Vec<String>) {
        let destination = hooks_directory(&config_directory);
        let mut problems = copy_hook_scripts(&hook_scripts_source, &destination);
        let home = std::env::home_dir();
        let agents = match home {
            Some(home) => agents_of(&config_directory, &real_env, &home),
            None => Vec::new(),
        };
        let can_write = Box::new(|agent: &Agent| can_write_for(agent.setup, IS_STABLE, &real_env));
        let (setups, startup_problems) = AgentSetups::start_with(
            agents,
            can_write,
            host,
            SUBMIT_CHECK_DELAY,
            config_directory,
        );
        problems.extend(startup_problems);
        (setups, problems)
    }

    pub(crate) fn start_with(
        agents: Vec<Agent>,
        can_write: Box<dyn Fn(&Agent) -> bool + Send + Sync>,
        host: AgentSetupsHost,
        submit_delay: Duration,
        config_directory: PathBuf,
    ) -> (AgentSetups, Vec<String>) {
        let state = State {
            seen: BTreeMap::new(),
            exited: BTreeSet::new(),
            recorded: BTreeMap::new(),
            view: AgentSetupView::default(),
            awaiting: false,
        };
        let shared = Shared {
            agents,
            can_write,
            host,
            state: Mutex::new(state),
            refresh_order: Mutex::new(()),
            stopping: AtomicBool::new(false),
        };

        let readings = read_all(&shared);
        let mut problems = Vec::new();
        for reading in &readings {
            if reading.may_write && reading.state == AgentSetupState::SetUp {
                let kept = keep_current(reading.agent);
                if let Err(problem) = kept {
                    problems.push(problem);
                }
            }
        }
        drop(readings);
        let (view, awaiting) = compute(&shared);
        {
            let mut state = lock(&shared.state);
            state.view = view;
            state.awaiting = awaiting;
        }

        let shared = Arc::new(shared);
        let (sender, messages) = mpsc::channel();
        let for_thread = Arc::clone(&shared);
        let thread = thread::spawn(move || timer_loop(for_thread, messages));
        let setups = AgentSetups {
            shared,
            hooks_directory: hooks_directory(&config_directory),
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
        let managing = (self.shared.host.is_managing)();
        let state = read_state(agent, managing);
        let may_write = (self.shared.can_write)(agent);
        let product_name = agent.setup.product_name;
        let fresh = agent_menu_of(state, product_name, may_write);
        // The menu may have changed since it was drawn, so a click acts only if it would still be offered.
        if fresh.enabled && fresh.action == clicked {
            let performed = perform(agent, clicked);
            let mut shared_state = lock(&self.shared.state);
            match performed {
                Err(text) => {
                    shared_state
                        .recorded
                        .insert(harness.clone(), Recorded::Problem(text));
                }
                Ok(()) if clicked == AgentMenuAction::Remove => {
                    shared_state.recorded.remove(harness);
                }
                Ok(()) => {
                    shared_state
                        .recorded
                        .insert(harness.clone(), Recorded::Done);
                }
            }
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

    use tempfile::TempDir;

    use crate::agent_hooks::entry_key;
    use crate::agent_record::{RECORD_FILE, read_agent_record, write_agent_record};

    use super::*;

    const WAIT: Duration = Duration::from_secs(2);

    struct Environment {
        temp: TempDir,
        agent: Agent,
        agents: Vec<Agent>,
        config: PathBuf,
    }

    struct Fixture {
        _temp: TempDir,
        agent: Agent,
        setups: AgentSetups,
        commands: Arc<Mutex<HashMap<TerminalId, Vec<String>>>>,
        changes: Arc<Mutex<Vec<AgentSetupView>>>,
        managing: Arc<AtomicBool>,
        may_write: Arc<AtomicBool>,
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
        }
    }

    fn fixture() -> Fixture {
        let environment = environment();
        let commands: Arc<Mutex<HashMap<TerminalId, Vec<String>>>> = Arc::default();
        let changes: Arc<Mutex<Vec<AgentSetupView>>> = Arc::default();
        let managing = Arc::new(AtomicBool::new(true));
        let may_write = Arc::new(AtomicBool::new(true));

        let managing_flag = Arc::clone(&managing);
        let commands_map = Arc::clone(&commands);
        let changes_log = Arc::clone(&changes);
        let may_write_flag = Arc::clone(&may_write);
        let host = AgentSetupsHost {
            is_managing: Box::new(move || managing_flag.load(Ordering::SeqCst)),
            foreground_commands: Box::new(move |id| {
                let map = lock(&commands_map);
                let found = map.get(id);
                match found {
                    Some(lines) => lines.clone(),
                    None => Vec::new(),
                }
            }),
            on_change: Box::new(move |view| lock(&changes_log).push(view)),
        };
        let can_write = Box::new(move |_: &Agent| may_write_flag.load(Ordering::SeqCst));
        let (setups, _) = AgentSetups::start_with(
            environment.agents.clone(),
            can_write,
            host,
            SUBMIT_CHECK_DELAY,
            environment.config.clone(),
        );
        Fixture {
            _temp: environment.temp,
            agent: environment.agent,
            setups,
            commands,
            changes,
            managing,
            may_write,
        }
    }

    fn start_timed(
        environment: &Environment,
        foreground_commands: ForegroundCommands,
    ) -> (AgentSetups, Receiver<AgentSetupView>) {
        let (sender, views) = mpsc::channel();
        let sender = Mutex::new(sender);
        let host = AgentSetupsHost {
            is_managing: Box::new(|| true),
            foreground_commands,
            on_change: Box::new(move |view| {
                let _ = lock(&sender).send(view);
            }),
        };
        let can_write = Box::new(|_: &Agent| true);
        let (setups, _) = AgentSetups::start_with(
            environment.agents.clone(),
            can_write,
            host,
            Duration::ZERO,
            environment.config.clone(),
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

    fn next_offer_covering(
        views: &Receiver<AgentSetupView>,
        covered: &TerminalId,
    ) -> AgentSetupView {
        loop {
            let received = views.recv_timeout(WAIT);
            let view = received.expect("no offer arrived in time");
            let offered = view.notices.iter().any(|notice| {
                let is_offer = notice.kind == AgentSetupNoticeKind::Offer;
                is_offer && notice.terminals.contains(covered)
            });
            if offered {
                return view;
            }
        }
    }

    fn run_in(fixture: &Fixture, name: &str, line: &str) -> TerminalId {
        let id = terminal(name);
        let mut map = lock(&fixture.commands);
        map.insert(id.clone(), vec![line.to_string()]);
        id
    }

    fn only_notice(fixture: &Fixture) -> AgentSetupNotice {
        let view = fixture.setups.view();
        assert_eq!(view.notices.len(), 1);
        view.notices[0].clone()
    }

    #[test]
    fn the_line_follows_the_agent() {
        let fixture = fixture();
        let setups = &fixture.setups;
        let claude_terminal = run_in(&fixture, "one", "claude --resume");
        let vim_terminal = run_in(&fixture, "two", "vim notes.md");

        setups.check_terminal(&vim_terminal);
        assert!(setups.view().notices.is_empty());

        setups.check_terminal(&claude_terminal);
        let offer = only_notice(&fixture);
        assert_eq!(offer.kind, AgentSetupNoticeKind::Offer);
        assert_eq!(offer.terminals, vec![claude_terminal.clone()]);
        let changes_so_far = lock(&fixture.changes).len();
        assert_eq!(changes_so_far, 1);

        setups.refresh();
        assert_eq!(lock(&fixture.changes).len(), changes_so_far);

        setups.act(&claude(), AgentMenuAction::SetUp);
        let settings = fs::read_to_string(&fixture.agent.paths.settings_path).unwrap();
        assert!(settings.contains("SessionStart"));
        let done = only_notice(&fixture);
        assert_eq!(done.kind, AgentSetupNoticeKind::Done);
        assert_eq!(done.terminals, vec![claude_terminal.clone()]);

        setups.dismiss(&claude());
        assert!(setups.view().notices.is_empty());

        setups.act(&claude(), AgentMenuAction::Remove);
        assert_eq!(only_notice(&fixture).kind, AgentSetupNoticeKind::Offer);
        setups.note_exit(&claude_terminal);
        assert!(setups.view().notices.is_empty());

        setups.check_terminal(&claude_terminal);
        assert!(setups.view().notices.is_empty());

        let other_terminal = run_in(&fixture, "three", "claude");
        setups.check_terminal(&other_terminal);
        fixture.managing.store(false, Ordering::SeqCst);
        setups.refresh();
        let off = only_notice(&fixture);
        let turned_off = AgentSetupNoticeKind::Unavailable {
            reason: STATUS_OFF.to_string(),
        };
        assert_eq!(off.kind, turned_off);

        fixture.managing.store(true, Ordering::SeqCst);
        fixture.may_write.store(false, Ordering::SeqCst);
        setups.refresh();
        let development = only_notice(&fixture);
        let leaves_alone = AgentSetupNoticeKind::Unavailable {
            reason: NOTE_BUILD_MAY_NOT_WRITE.to_string(),
        };
        assert_eq!(development.kind, leaves_alone);
    }

    #[test]
    fn a_stale_click_does_nothing() {
        let fixture = fixture();
        let setups = &fixture.setups;
        let before_menu = setups.view().menus[0].menu.action;
        assert_eq!(before_menu, AgentMenuAction::SetUp);

        assert_eq!(set_up(&fixture.agent), Ok(()));
        let path = &fixture.agent.paths.settings_path;
        let before = fs::read(path).unwrap();

        setups.act(&claude(), AgentMenuAction::SetUp);

        let after = fs::read(path).unwrap();
        assert_eq!(before, after);
        assert!(setups.view().notices.is_empty());
        let menu = setups.view().menus[0].menu.action;
        assert_eq!(menu, AgentMenuAction::Remove);

        setups.act(&claude(), AgentMenuAction::Switch);

        let after_switch = fs::read(path).unwrap();
        assert_eq!(before, after_switch);
    }

    #[test]
    fn startup_keeps_current_only_where_writing_is_allowed() {
        for may_write in [false, true] {
            let environment = environment();
            let agent = &environment.agent;
            assert_eq!(set_up(agent), Ok(()));
            let dropped = agent.setup.entries.last().unwrap();
            let dropped_key = entry_key(dropped);

            let text = fs::read_to_string(&agent.paths.settings_path).unwrap();
            let mut root: serde_json::Value = serde_json::from_str(&text).unwrap();
            let hooks = root.get_mut("hooks").unwrap().as_object_mut().unwrap();
            hooks.remove(dropped.event);
            let pruned = serde_json::to_string_pretty(&root).unwrap();
            fs::write(&agent.paths.settings_path, &pruned).unwrap();
            let id = &agent.harness_id.0;
            let mut record = read_agent_record(&agent.paths.config_directory, id);
            record.written.retain(|key| *key != dropped_key);
            write_agent_record(&agent.paths.config_directory, id, &record).unwrap();
            let before = fs::read(&agent.paths.settings_path).unwrap();

            let host = AgentSetupsHost {
                is_managing: Box::new(|| true),
                foreground_commands: Box::new(|_| Vec::new()),
                on_change: Box::new(|_| {}),
            };
            let can_write = Box::new(move |_: &Agent| may_write);
            let (setups, problems) = AgentSetups::start_with(
                environment.agents.clone(),
                can_write,
                host,
                SUBMIT_CHECK_DELAY,
                environment.config.clone(),
            );
            setups.stop();

            assert!(problems.is_empty(), "{problems:?}");
            let after = fs::read(&agent.paths.settings_path).unwrap();
            if may_write {
                assert_ne!(before, after);
                let restored = fs::read_to_string(&agent.paths.settings_path).unwrap();
                assert!(restored.contains(dropped.event));
            } else {
                assert_eq!(before, after);
            }
        }
    }

    #[test]
    fn a_startup_problem_is_returned_not_shown_as_a_notice() {
        let environment = environment();
        let agent = &environment.agent;
        assert_eq!(set_up(agent), Ok(()));
        fs::remove_dir_all(&environment.config).unwrap();
        fs::write(&environment.config, "in the way").unwrap();
        let state = read_state(agent, true);
        assert_eq!(state, AgentSetupState::SetUp);

        let host = AgentSetupsHost {
            is_managing: Box::new(|| true),
            foreground_commands: Box::new(|_| Vec::new()),
            on_change: Box::new(|_| {}),
        };
        let can_write = Box::new(|_: &Agent| true);
        let (setups, problems) = AgentSetups::start_with(
            environment.agents.clone(),
            can_write,
            host,
            SUBMIT_CHECK_DELAY,
            environment.config.clone(),
        );

        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains(RECORD_FILE), "{}", problems[0]);
        assert!(setups.view().notices.is_empty());
    }

    #[test]
    fn the_timer_checks_only_terminals_submitted_while_something_awaits() {
        let environment = environment();
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, views) = start_timed(&environment, running_claude(&checked));
        let first = terminal("first");
        let skipped = terminal("skipped");
        let last = terminal("last");

        setups.note_submit(first.clone());
        let offer_view = next_offer_covering(&views, &first);
        let offer = &offer_view.notices[0];
        assert_eq!(offer.terminals, vec![first.clone()]);

        setups.act(&claude(), AgentMenuAction::SetUp);
        setups.note_submit(skipped.clone());

        assert_eq!(remove_setup(&environment.agent), Ok(()));
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
        let (setups, views) = start_timed(&environment, foreground_commands);

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
        let bad = terminal("bad");
        let good = terminal("good");
        let panicking = bad.clone();
        let foreground_commands: ForegroundCommands = Box::new(move |id| {
            if *id == panicking {
                panic!("the foreground lookup failed");
            }
            vec!["claude".to_string()]
        });
        let (setups, views) = start_timed(&environment, foreground_commands);

        setups.note_submit(bad.clone());
        setups.note_submit(good.clone());

        let view = next_offer_covering(&views, &good);
        let offer = &view.notices[0];
        assert_eq!(offer.terminals, vec![good]);
    }

    #[test]
    fn the_hooks_directory_is_the_one_the_scripts_are_copied_to() {
        let environment = environment();
        let checked: Arc<Mutex<Vec<TerminalId>>> = Arc::default();
        let (setups, _views) = start_timed(&environment, running_claude(&checked));

        let directory = setups.hooks_directory();

        let expected = hooks_directory(&environment.config);
        assert_eq!(directory, expected);
        setups.stop();
    }
}
