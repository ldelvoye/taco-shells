use std::collections::{BTreeMap, BTreeSet};

use taco_agents::{HARNESSES, default_harness, harness_of};
use taco_protocol::{
    ActivityRequest, AgentLabel, AgentSignal, Arrangement, ConfigFile, DrawnState, HarnessId,
    HookCall, HookEvent, Panels, RequestReply, Row, RowState, RuntimeCommand, RuntimeReport,
    TabGroup, TabGroupId, TerminalId, TerminalSize, TerminalStart, WorkspaceRequest,
};
use taco_rules::{
    add_row, add_row_to, apply_drop, can_fork, close_panel, close_row, create_tab_group,
    default_panels, empty_arrangement, fork_name_for, fork_row, locate_row, move_panel, open_panel,
    rename_row, rename_tab_group, resize_panel, select_row, selected_terminal, toggle_tab_group,
    ungroup_tab_group,
};

use crate::activity::Activity;
use crate::outbound::Outbound;
use crate::rows::{
    AgentRows, Signalled, apply_idle_title, apply_on_screen, apply_question, apply_signal,
};
use crate::save::{SaveScheduler, Saved, read_saved, saved_from};
use crate::sessions::{AgentSession, AgentSessions};
use crate::start::{ShellStart, editor_command, fork_start, resume_start};
use crate::storage::Storage;

const INITIAL_SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

/// What the workspace is given at launch.
pub struct Options {
    pub settings_path: String,
    pub keybindings_path: String,
    /// Mints the id of each new terminal; it must never repeat one from an earlier launch, since restored rows keep theirs.
    pub new_terminal_id: Box<dyn FnMut() -> TerminalId + Send>,
    pub storage: Box<dyn Storage>,
    /// The `restoreOnLaunch` setting: when false, the saved terminals are not reopened.
    pub restore_on_launch: bool,
}

/// The owner of the arrangement: it applies what the window asks for and what the runtime and the hooks report.
pub struct Workspace {
    options: Options,
    arrangement: Option<Arrangement>,
    rows: AgentRows,
    directories: BTreeMap<TerminalId, String>,
    /// The terminals the runtime has reported started, for the window to attach to.
    started: BTreeSet<TerminalId>,
    raw_titles: BTreeMap<TerminalId, String>,
    sessions: AgentSessions,
    panels: Panels,
    /// The terminal the app jumps to when it is next activated.
    waiting: Option<TerminalId>,
    refocus: u64,
    focus_count: u64,
    problem: Option<String>,
    runtime_gone: bool,
    next_tab_group: u64,
    revision: u64,
    published: Option<DrawnState>,
    agents: BTreeMap<HarnessId, AgentLabel>,
    save: SaveScheduler,
    frozen: bool,
    activity: Activity,
}

fn agent_labels() -> BTreeMap<HarnessId, AgentLabel> {
    let labels = HARNESSES.iter().map(|harness| {
        let label = AgentLabel {
            name: harness.name().to_string(),
            forks: harness.forks(),
        };
        (harness.id(), label)
    });
    labels.collect()
}

fn runtime_problem(reason: &str) -> String {
    format!(
        "The terminals stopped because their background process exited ({reason}). Quit and reopen Taco Shells to start again."
    )
}

fn accepted(tab_group: Option<TabGroupId>) -> (bool, Option<TabGroupId>) {
    (true, tab_group)
}

fn refused() -> (bool, Option<TabGroupId>) {
    (false, None)
}

fn requested_terminal(request: &ActivityRequest) -> Option<&TerminalId> {
    match request {
        ActivityRequest::Watch { terminal } => terminal.as_ref(),
        ActivityRequest::Older { terminal, .. } => Some(terminal),
        ActivityRequest::Body { terminal, .. } => Some(terminal),
    }
}

impl Workspace {
    pub fn new(options: Options) -> Self {
        Workspace {
            options,
            arrangement: None,
            rows: AgentRows::new(),
            directories: BTreeMap::new(),
            started: BTreeSet::new(),
            raw_titles: BTreeMap::new(),
            sessions: AgentSessions::default(),
            panels: default_panels(),
            waiting: None,
            refocus: 0,
            focus_count: 0,
            problem: None,
            runtime_gone: false,
            next_tab_group: 1,
            revision: 0,
            published: None,
            agents: agent_labels(),
            save: SaveScheduler::default(),
            frozen: false,
            activity: Activity::default(),
        }
    }

    /// Reopens the saved terminals, or opens a first one. Called once, after the app can carry what comes out.
    pub fn start(&mut self, now_ms: u64) -> Vec<Outbound> {
        let mut out = Vec::new();
        let mut saved = None;
        if self.options.restore_on_launch
            && let Some(text) = self.options.storage.read_save()
        {
            saved = read_saved(&text);
        }
        let restored = match saved {
            Some(saved) => self.restore(saved, now_ms, &mut out),
            None => false,
        };
        if !restored {
            self.open_fresh(None, &mut out);
        }
        // a failed restore deletes no log, since its save was kept aside and can be put back
        let rows_are_known = restored || !self.options.restore_on_launch;
        if rows_are_known {
            self.drop_orphan_logs();
        }
        self.finish(now_ms, &mut out);
        out
    }

    /// When `tick` next has something to do: a save to write, or a restored session to forget.
    pub fn next_deadline(&self) -> Option<u64> {
        let mut expiry = None;
        if !self.frozen {
            expiry = self.sessions.next_expiry();
        }
        let deadlines = [self.save.due(), expiry];
        deadlines.into_iter().flatten().min()
    }

    /// Forgets restored sessions whose agent never started again, and writes the save once it is due.
    pub fn tick(&mut self, now_ms: u64) {
        let expiry = self.sessions.next_expiry();
        let expired = expiry.is_some_and(|expiry| expiry <= now_ms);
        if expired && self.sessions.drop_expired(now_ms) {
            self.schedule_save(now_ms);
        }
        let due = self.save.due().is_some_and(|due| due <= now_ms);
        if due {
            self.write_save(now_ms);
        }
    }

    /// The last write at quit, after `freeze`: the terminals as they were when the quit began.
    pub fn flush(&mut self, now_ms: u64) {
        self.write_save(now_ms);
    }

    /// Everything the window draws, as it is now.
    pub fn drawn(&self) -> DrawnState {
        let rows = self.rows.iter().map(|(terminal, row)| {
            let mut shown = row.clone();
            if shown.state != RowState::Needs {
                shown.question = None;
            }
            (terminal.clone(), shown)
        });
        DrawnState {
            revision: self.revision,
            arrangement: self.arrangement.clone(),
            agent_rows: rows.collect(),
            directories: self.directories.clone(),
            started: self.started.clone(),
            refocus: self.refocus,
            focus_count: self.focus_count,
            agents: self.agents.clone(),
            default_agent_name: default_harness().name().to_string(),
            panels: self.panels.clone(),
            problem: self.problem.clone(),
        }
    }

    pub fn request(
        &mut self,
        request: WorkspaceRequest,
        now_ms: u64,
    ) -> (RequestReply, Vec<Outbound>) {
        let mut out = Vec::new();
        let (ok, tab_group) = self.answer(request, &mut out);
        self.finish(now_ms, &mut out);
        let reply = RequestReply {
            ok,
            tab_group,
            revision: self.revision,
        };
        (reply, out)
    }

    pub fn runtime_report(&mut self, report: RuntimeReport, now_ms: u64) -> Vec<Outbound> {
        let mut out = Vec::new();
        match report {
            RuntimeReport::Started { id, directory } => {
                if self.holds(&id) {
                    self.started.insert(id.clone());
                    self.directories.insert(id, directory);
                }
            }
            RuntimeReport::DirectoryChanged { id, directory } => {
                if self.holds(&id) {
                    self.directories.insert(id, directory);
                }
            }
            RuntimeReport::Exited { id, .. } => self.terminal_exited(&id),
            RuntimeReport::Titled { id, title } => self.titled(&id, title, now_ms),
            RuntimeReport::Submitted { .. } | RuntimeReport::Answer { .. } => {}
        }
        self.finish(now_ms, &mut out);
        out
    }

    pub fn hook(&mut self, event: HookEvent, now_ms: u64) -> Vec<Outbound> {
        let mut out = Vec::new();
        if self.holds(&event.terminal) {
            self.apply_hook(&event.terminal, event.call, now_ms, &mut out);
        }
        self.finish(now_ms, &mut out);
        out
    }

    /// A session of a known agent starting in a terminal the workspace holds: the log marks how it began, and a session
    /// that was just compacted is handed back its now line and its newest notes. None for any other start.
    pub fn session_context(
        &mut self,
        terminal: &TerminalId,
        harness: &HarnessId,
        session_id: &str,
        source: &str,
        now_ms: u64,
    ) -> (Option<String>, Vec<Outbound>) {
        let mut out = Vec::new();
        if !self.holds_known_agent(terminal, harness) {
            return (None, out);
        }
        let storage = self.options.storage.as_mut();
        let context = self
            .activity
            .session_context(storage, terminal, session_id, source, now_ms, &mut out);
        (context, out)
    }

    /// The ask for a note an agent is about to stop without, when its steps since the last one hold real work.
    /// Asking records nothing: `asked_for_note` does, once the ask has reached the agent.
    pub fn stop_nudge(
        &mut self,
        terminal: &TerminalId,
        harness: &HarnessId,
        session_id: &str,
    ) -> Option<String> {
        if !self.may_ask(terminal, harness, session_id) {
            return None;
        }
        let directory = self.session_directory(terminal);
        let storage = self.options.storage.as_ref();
        self.activity
            .stop_nudge(storage, terminal, directory.as_deref())
    }

    /// The log's record that the app asked the agent for a note, which leaves the stretch for that note.
    pub fn asked_for_note(
        &mut self,
        terminal: &TerminalId,
        harness: &HarnessId,
        session_id: &str,
        now_ms: u64,
    ) -> Vec<Outbound> {
        let mut out = Vec::new();
        if !self.may_ask(terminal, harness, session_id) {
            return out;
        }
        let storage = self.options.storage.as_mut();
        self.activity
            .asked_for_note(storage, terminal, now_ms, &mut out);
        out
    }

    fn holds_known_agent(&self, terminal: &TerminalId, harness: &HarnessId) -> bool {
        let known = harness_of(&harness.0).is_some();
        known && self.holds(terminal)
    }

    // A known agent in a held terminal, and the terminal's own session; one the workspace never heard start is let
    // through, and an older one that has since been replaced is not.
    fn may_ask(&self, terminal: &TerminalId, harness: &HarnessId, session_id: &str) -> bool {
        if !self.holds_known_agent(terminal, harness) {
            return false;
        }
        let Some(current) = self.sessions.session_of(terminal) else {
            return true;
        };
        current.session_id == session_id
    }

    fn session_directory(&self, terminal: &TerminalId) -> Option<String> {
        let session = self.sessions.session_of(terminal)?;
        Some(session.cwd.clone())
    }

    fn current_session(&self, terminal: &TerminalId) -> Option<String> {
        let session = self.sessions.session_of(terminal)?;
        Some(session.session_id.clone())
    }

    /// What the window asks of Agent Activity: the terminal it shows, and older entries of it.
    pub fn activity(&mut self, request: ActivityRequest) -> Vec<Outbound> {
        let mut held = false;
        let mut current_session = None;
        if let Some(terminal) = requested_terminal(&request) {
            held = self.holds(terminal);
            current_session = self.current_session(terminal);
        }
        let storage = self.options.storage.as_ref();
        self.activity
            .request(storage, request, held, current_session.as_deref())
    }

    /// The app came to the front: jump to the terminal that last asked for attention.
    pub fn app_activated(&mut self, now_ms: u64) -> Vec<Outbound> {
        let mut out = Vec::new();
        if let Some(waiting) = self.waiting.clone() {
            self.change_arrangement(|arrangement| select_row(arrangement, &waiting));
            self.refocus += 1;
        }
        self.finish(now_ms, &mut out);
        out
    }

    /// Closes every terminal and tells the window why; the save is frozen first.
    pub fn runtime_died(&mut self, reason: &str, now_ms: u64) -> Vec<Outbound> {
        let mut out = Vec::new();
        self.freeze();
        self.runtime_gone = true;
        self.problem = Some(runtime_problem(reason));
        let open = self.open_terminals();
        for terminal in &open {
            self.terminal_exited(terminal);
        }
        self.finish(now_ms, &mut out);
        out
    }

    /// The start of a quit: from here on nothing changes what is saved, so the terminals closing as the app quits stay in the save.
    pub fn freeze(&mut self) {
        self.frozen = true;
        self.sessions.freeze();
    }

    fn base_arrangement(&self) -> Arrangement {
        match &self.arrangement {
            Some(arrangement) => arrangement.clone(),
            None => empty_arrangement(),
        }
    }

    fn holds(&self, terminal: &TerminalId) -> bool {
        let Some(arrangement) = &self.arrangement else {
            return false;
        };
        locate_row(arrangement, terminal).is_some()
    }

    fn open_terminals(&self) -> Vec<TerminalId> {
        let Some(arrangement) = &self.arrangement else {
            return Vec::new();
        };
        let terminals = arrangement.rows.iter().map(|row| row.terminal.clone());
        terminals.collect()
    }

    /// Applies a change to the arrangement and says whether it changed it. Before the first terminal there is nothing to change.
    fn change_arrangement(
        &mut self,
        change: impl FnOnce(&Arrangement) -> Option<Arrangement>,
    ) -> bool {
        let Some(arrangement) = &self.arrangement else {
            return false;
        };
        let Some(changed) = change(arrangement) else {
            return false;
        };
        self.arrangement = Some(changed);
        true
    }

    fn change_panels(&mut self, change: impl FnOnce(&Panels) -> Option<Panels>) {
        if let Some(changed) = change(&self.panels) {
            self.panels = changed;
        }
    }

    fn answer(
        &mut self,
        request: WorkspaceRequest,
        out: &mut Vec<Outbound>,
    ) -> (bool, Option<TabGroupId>) {
        let opens = matches!(
            request,
            WorkspaceRequest::OpenTerminal { .. }
                | WorkspaceRequest::OpenTerminalIn { .. }
                | WorkspaceRequest::ForkTerminal { .. }
        );
        if self.runtime_gone && opens {
            return refused();
        }

        match request {
            WorkspaceRequest::OpenTerminal { file } => {
                let command = file.map(|file| {
                    let path = match file {
                        ConfigFile::Settings => &self.options.settings_path,
                        ConfigFile::Keybindings => &self.options.keybindings_path,
                    };
                    editor_command(path)
                });
                self.open_fresh(command, out);
                accepted(None)
            }
            WorkspaceRequest::OpenTerminalIn { tab_group } => {
                let terminal = (self.options.new_terminal_id)();
                let base = self.base_arrangement();
                if let Some(added) = add_row_to(&base, terminal.clone(), tab_group) {
                    self.arrangement = Some(added);
                }
                self.open(terminal, ShellStart::default(), out);
                accepted(None)
            }
            WorkspaceRequest::ForkTerminal { terminal } => self.fork(&terminal, out),
            WorkspaceRequest::CloseTerminal { terminal } => {
                if self.holds(&terminal) {
                    out.push(Outbound::Runtime(RuntimeCommand::Close { id: terminal }));
                }
                accepted(None)
            }
            WorkspaceRequest::FocusReported { terminal, count } => {
                // always recorded: the window holds its own selection until this count catches up
                if count > self.focus_count {
                    self.focus_count = count;
                }
                self.change_arrangement(|arrangement| select_row(arrangement, &terminal));
                accepted(None)
            }
            WorkspaceRequest::Dropped { dragged, place } => {
                if self.change_arrangement(|arrangement| apply_drop(arrangement, &dragged, &place))
                {
                    self.refocus += 1;
                }
                accepted(None)
            }
            WorkspaceRequest::TabGroupCreated { terminal } => {
                let id = self.peek_tab_group_id();
                let created = self.change_arrangement(|arrangement| {
                    create_tab_group(arrangement, &terminal, id.clone())
                });
                if !created {
                    return accepted(None);
                }
                self.next_tab_group += 1;
                accepted(Some(id))
            }
            WorkspaceRequest::TabGroupRenamed { id, name } => {
                self.change_arrangement(|arrangement| rename_tab_group(arrangement, &id, &name));
                accepted(None)
            }
            WorkspaceRequest::TabGroupUngrouped { id } => {
                self.change_arrangement(|arrangement| ungroup_tab_group(arrangement, &id));
                accepted(None)
            }
            WorkspaceRequest::TabGroupToggled { id } => {
                if self.change_arrangement(|arrangement| toggle_tab_group(arrangement, &id)) {
                    self.refocus += 1;
                }
                accepted(None)
            }
            WorkspaceRequest::PanelOpened { panel } => {
                self.change_panels(|panels| open_panel(panels, panel));
                accepted(None)
            }
            WorkspaceRequest::PanelClosed { panel } => {
                self.change_panels(|panels| close_panel(panels, panel));
                accepted(None)
            }
            WorkspaceRequest::PanelMoved { panel, edge } => {
                self.change_panels(|panels| move_panel(panels, panel, edge));
                accepted(None)
            }
            WorkspaceRequest::PanelResized { panel, size } => {
                self.change_panels(|panels| resize_panel(panels, panel, size));
                accepted(None)
            }
        }
    }

    fn open(&self, terminal: TerminalId, shell: ShellStart, out: &mut Vec<Outbound>) {
        let start = TerminalStart {
            id: terminal,
            directory: shell.directory,
            directory_of: shell.directory_of,
            command: shell.command,
            size: INITIAL_SIZE,
        };
        out.push(Outbound::Runtime(RuntimeCommand::Open(start)));
    }

    fn open_fresh(&mut self, command: Option<String>, out: &mut Vec<Outbound>) {
        let terminal = (self.options.new_terminal_id)();
        let base = self.base_arrangement();
        if let Some(added) = add_row(&base, terminal.clone()) {
            self.arrangement = Some(added);
        }
        let shell = ShellStart {
            command,
            ..ShellStart::default()
        };
        self.open(terminal, shell, out);
    }

    // Refused by the same rule the window greys Fork out with, so a stale click opens nothing rather than something that is not a fork.
    fn fork(&mut self, source: &TerminalId, out: &mut Vec<Outbound>) -> (bool, Option<TabGroupId>) {
        if !can_fork(&self.rows, &self.agents, source) {
            return refused();
        }
        let Some(arrangement) = &self.arrangement else {
            return refused();
        };
        let Some(index) = locate_row(arrangement, source) else {
            return refused();
        };
        let titles: Vec<&str> = arrangement
            .rows
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        let name = fork_name_for(&arrangement.rows[index].title, &titles);
        let Some(shell) = fork_start(&self.sessions, source, &name) else {
            return refused();
        };

        let terminal = (self.options.new_terminal_id)();
        self.change_arrangement(|arrangement| fork_row(arrangement, source, terminal.clone()));
        self.open(terminal, shell, out);
        accepted(None)
    }

    fn terminal_exited(&mut self, terminal: &TerminalId) {
        let storage = self.options.storage.as_mut();
        self.activity
            .terminal_exited(storage, terminal, self.frozen);
        self.started.remove(terminal);
        self.sessions.forget(terminal);
        self.directories.remove(terminal);
        self.raw_titles.remove(terminal);
        self.rows.remove(terminal);
        if self.waiting.as_ref() == Some(terminal) {
            self.waiting = None;
        }
        self.change_arrangement(|arrangement| close_row(arrangement, terminal));
    }

    fn titled(&mut self, terminal: &TerminalId, title: String, now_ms: u64) {
        if !self.holds(terminal) {
            return;
        }
        self.raw_titles.insert(terminal.clone(), title);
        self.read_title(terminal, now_ms);
    }

    // A title can arrive before the agent's start, so the last one is read again when an agent starts.
    fn read_title(&mut self, terminal: &TerminalId, now_ms: u64) {
        let Some(raw) = self.raw_titles.get(terminal).cloned() else {
            return;
        };
        let agent = match self.rows.get(terminal) {
            Some(row) => harness_of(&row.harness.0),
            None => None,
        };
        let Some(harness) = agent else {
            self.change_arrangement(|arrangement| rename_row(arrangement, terminal, &raw));
            return;
        };
        let read = harness.title_of(&raw);
        self.change_arrangement(|arrangement| rename_row(arrangement, terminal, &read.name));
        if read.idle {
            apply_idle_title(&mut self.rows, terminal, now_ms);
        }
    }

    fn signal(
        &mut self,
        terminal: &TerminalId,
        harness: &HarnessId,
        session_id: &str,
        signal: AgentSignal,
        now_ms: u64,
    ) {
        let selected = selected_terminal(self.arrangement.as_ref());
        let on_screen = selected == Some(terminal);
        let signalled = Signalled {
            terminal,
            harness,
            session_id,
            signal,
            on_screen,
            now_ms,
        };
        apply_signal(&mut self.rows, signalled);
        if signal == AgentSignal::Start || signal == AgentSignal::Resume {
            self.read_title(terminal, now_ms);
        }
    }

    fn apply_hook(
        &mut self,
        terminal: &TerminalId,
        call: HookCall,
        now_ms: u64,
        out: &mut Vec<Outbound>,
    ) {
        match call {
            HookCall::Attention => {
                // asking while you are already in that terminal is not a request to go anywhere
                let selected = selected_terminal(self.arrangement.as_ref());
                if selected != Some(terminal) {
                    self.waiting = Some(terminal.clone());
                }
            }
            HookCall::AgentStart {
                harness,
                session_id,
                cwd,
                has_conversation,
            } => {
                let session = AgentSession {
                    harness: harness.clone(),
                    session_id: session_id.clone(),
                    cwd,
                };
                let session_before = self.current_session(terminal);
                let storage = self.options.storage.as_ref();
                let shown_before =
                    self.activity
                        .watched_board(storage, terminal, session_before.as_deref());
                self.sessions.record_start(terminal, session);
                let session_after = self.current_session(terminal);
                let storage = self.options.storage.as_ref();
                self.activity.agent_started(
                    storage,
                    terminal,
                    &session_id,
                    session_after.as_deref(),
                    shown_before,
                    out,
                );
                let mut signal = AgentSignal::Start;
                if has_conversation {
                    signal = AgentSignal::Resume;
                }
                self.signal(terminal, &harness, &session_id, signal, now_ms);
            }
            HookCall::AgentEnd {
                harness,
                session_id,
            } => {
                self.sessions.record_end(terminal, &harness, &session_id);
                self.signal(terminal, &harness, &session_id, AgentSignal::End, now_ms);
            }
            HookCall::AgentState {
                harness,
                session_id,
                signal,
            } => {
                self.signal(terminal, &harness, &session_id, signal, now_ms);
            }
            HookCall::AgentHook {
                harness,
                session_id,
                event,
                payload,
            } => {
                let Some(agent) = harness_of(&harness.0) else {
                    return;
                };
                if let Some(question) = agent.question_of(&event, &payload) {
                    apply_question(&mut self.rows, terminal, &session_id, question);
                }
                let reads = agent.entries_of(&event, &payload);
                let current_session = self.current_session(terminal);
                for read in reads {
                    let storage = self.options.storage.as_mut();
                    let logged = self.activity.read_into_activity(
                        storage,
                        terminal,
                        &session_id,
                        current_session.as_deref(),
                        read,
                        now_ms,
                    );
                    out.extend(logged);
                }
            }
        }
    }

    fn drop_orphan_logs(&mut self) {
        let held = self.open_terminals();
        let storage = self.options.storage.as_mut();
        self.activity.drop_orphan_logs(storage, &held);
    }

    // Every input ends here: what follows from the selection, and a publish only when the state changed.
    fn finish(&mut self, now_ms: u64, out: &mut Vec<Outbound>) {
        let selected = selected_terminal(self.arrangement.as_ref()).cloned();
        apply_on_screen(&mut self.rows, selected.as_ref(), now_ms);
        if self.waiting.is_some() && self.waiting == selected {
            self.waiting = None;
        }

        self.schedule_save(now_ms);

        // drawn() carries the current revision, which is the published one's, so equal means unchanged
        let mut state = self.drawn();
        if self.published.as_ref() == Some(&state) {
            return;
        }
        self.revision += 1;
        state.revision = self.revision;
        self.published = Some(state.clone());
        out.push(Outbound::State(state));
    }

    /// The save as the state now is, or none before the first terminal.
    fn saved_now(&self) -> Option<Saved> {
        let arrangement = self.arrangement.as_ref()?;
        let saved = saved_from(arrangement, &self.directories, &self.sessions, &self.panels);
        Some(saved)
    }

    fn schedule_save(&mut self, now_ms: u64) {
        if self.frozen {
            return;
        }
        let saved = self.saved_now();
        self.save.schedule(saved, now_ms);
    }

    fn write_save(&mut self, now_ms: u64) {
        if !self.frozen {
            self.sessions.drop_expired(now_ms);
            let saved = self.saved_now();
            self.save.want(saved);
        }
        let storage = self.options.storage.as_mut();
        self.save.write(storage, now_ms);
    }

    /// Rebuilds the saved terminals under their saved ids and opens each where it was. False when the save holds none.
    fn restore(&mut self, saved: Saved, now_ms: u64, out: &mut Vec<Outbound>) -> bool {
        if saved.rows.is_empty() {
            return false;
        }
        let tab_group_ids: Vec<TabGroupId> = saved
            .tab_groups
            .iter()
            .map(|_| self.next_tab_group_id())
            .collect();

        let mut rows = Vec::new();
        let mut starts = Vec::new();
        for saved_row in saved.rows {
            let already_used = |id: &TerminalId| rows.iter().any(|row: &Row| row.terminal == *id);
            let terminal = match saved_row.id {
                Some(id) if !already_used(&id) => id,
                _ => (self.options.new_terminal_id)(),
            };
            let tab_group = match saved_row.tab_group {
                Some(index) => tab_group_ids.get(index).cloned(),
                None => None,
            };
            rows.push(Row {
                terminal: terminal.clone(),
                title: String::new(),
                tab_group,
            });
            let shell =
                self.restored_shell(&terminal, saved_row.directory, saved_row.agent, now_ms);
            starts.push((terminal, shell));
        }

        let records = saved.tab_groups.into_iter().zip(tab_group_ids);
        let tab_groups = records.map(|(record, id)| TabGroup {
            id,
            name: record.name,
            collapsed: record.collapsed,
        });
        let mut arrangement = Arrangement {
            selected: saved.selected,
            rows,
            tab_groups: tab_groups.collect(),
        };
        // the selected row's tab group opens, as selecting it would
        if let Some(selected) = selected_terminal(Some(&arrangement)).cloned()
            && let Some(opened) = select_row(&arrangement, &selected)
        {
            arrangement = opened;
        }
        self.arrangement = Some(arrangement);
        if let Some(panels) = saved.panels {
            self.panels = panels;
        }
        for (terminal, shell) in starts {
            self.open(terminal, shell, out);
        }
        true
    }

    // Seeded before the shell starts, since the first save can come first.
    fn restored_shell(
        &mut self,
        terminal: &TerminalId,
        directory: Option<String>,
        agent: Option<AgentSession>,
        now_ms: u64,
    ) -> ShellStart {
        let shell = match &agent {
            Some(agent) => resume_start(agent),
            None => ShellStart {
                directory,
                ..ShellStart::default()
            },
        };
        if let Some(directory) = &shell.directory {
            self.directories.insert(terminal.clone(), directory.clone());
        }
        if let Some(agent) = agent {
            self.sessions.seed(terminal, agent, now_ms);
        }
        shell
    }

    fn peek_tab_group_id(&self) -> TabGroupId {
        TabGroupId(format!("tab-group-{}", self.next_tab_group))
    }

    fn next_tab_group_id(&mut self) -> TabGroupId {
        let id = self.peek_tab_group_id();
        self.next_tab_group += 1;
        id
    }
}
