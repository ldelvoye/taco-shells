//! What the window runs on until the services are wired to it: the real workspace kept in memory, fixture logs and setup notices, published as they would be.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use futures::channel::mpsc::unbounded;
use serde_json::Map;
use taco_host::{ServicesOutput, ServicesPort, WindowOutput, WindowSender, WorkspacePort};
use taco_protocol::{
    ActivityEntry, ActivityEvent, ActivityRequest, Board, DrawnState, HarnessId, RuntimeCommand,
    RuntimeReport, TerminalId, TerminalStart, WorkspaceRequest,
};
use taco_services::{AgentMenuAction, AgentSetupNoticeKind, AgentSetupView, done_text};
use taco_settings::{Appearance, Config, KeymapChange, SettingChange, apply_settings_layer};
use taco_workspace::{Options, Outbound, Storage, Workspace};

use crate::activity_panel::now_ms;
use crate::link::Ports;
use crate::window::Shown;

// Without a config to write into, the stand-in refuses every write.
const REFUSED_WRITE: &str = "settings.json is read-only, so nothing was changed.";
const STAND_IN_LABEL: &str = "stand-in";
/// The agent the stand-in and its fixtures speak of.
pub(crate) const PRODUCT_NAME: &str = "Claude Code";
const REFUSED_KEYMAP_WRITE: &str = "keybindings.json is read-only, so nothing was changed.";

#[derive(Default)]
struct Memory {
    save: Option<String>,
    logs: BTreeMap<TerminalId, Vec<String>>,
    states: BTreeMap<TerminalId, String>,
}

/// The agent logs and boards the stand-in answers from, by terminal.
pub struct StandInActivity {
    pub logs: BTreeMap<TerminalId, Vec<ActivityEntry>>,
    pub boards: BTreeMap<TerminalId, Board>,
    /// The body of each shared text, by the thing's id.
    pub bodies: BTreeMap<u64, String>,
    /// How many entries one page of a log holds.
    pub page: usize,
}

pub struct StandIn {
    workspace: RefCell<Workspace>,
    setups: RefCell<AgentSetupView>,
    activity: StandInActivity,
    window: WindowSender,
    /// The config a setting is written into and published from; none when every write is refused.
    config: Option<RefCell<Config>>,
}

impl Storage for Memory {
    fn read_save(&self) -> Option<String> {
        self.save.clone()
    }

    fn write_save(&mut self, text: &str) -> bool {
        self.save = Some(text.to_string());
        true
    }

    fn read_log(&self, terminal: &TerminalId) -> Vec<String> {
        match self.logs.get(terminal) {
            Some(lines) => lines.clone(),
            None => Vec::new(),
        }
    }

    fn append_log(&mut self, terminal: &TerminalId, line: &str) -> bool {
        let lines = self.logs.entry(terminal.clone()).or_default();
        lines.push(line.to_string());
        true
    }

    fn rewrite_log(&mut self, terminal: &TerminalId, lines: &[String]) -> bool {
        self.logs.insert(terminal.clone(), lines.to_vec());
        true
    }

    fn delete_log(&mut self, terminal: &TerminalId) {
        self.logs.remove(terminal);
    }

    fn read_state(&self, terminal: &TerminalId) -> Option<String> {
        self.states.get(terminal).cloned()
    }

    fn write_state(&mut self, terminal: &TerminalId, text: &str) -> bool {
        self.states.insert(terminal.clone(), text.to_string());
        true
    }

    fn delete_state(&mut self, terminal: &TerminalId) {
        self.states.remove(terminal);
    }

    fn activity_terminals(&self) -> Vec<TerminalId> {
        let mut terminals: Vec<TerminalId> = self.logs.keys().cloned().collect();
        for terminal in self.states.keys() {
            if !terminals.contains(terminal) {
                terminals.push(terminal.clone());
            }
        }
        terminals
    }
}

impl StandIn {
    /// With `writes` set to the appearance drawn in, a setting written is applied and published as a new config.
    pub fn start(
        shown: &Shown,
        workspace: Workspace,
        activity: StandInActivity,
        writes: Option<Appearance>,
    ) -> Ports {
        let (window, outputs) = unbounded();
        let mut config = None;
        if let Some(appearance) = writes {
            let written = Config {
                settings: shown.settings.clone(),
                appearance,
                bindings: shown.bindings.clone(),
                default_bindings: shown.default_bindings.clone(),
                problems: shown.notices.config_problems.clone(),
                unread_settings: shown.unread.clone(),
                unread_keybindings: shown.unread_keybindings,
            };
            config = Some(RefCell::new(written));
        }
        let stand_in = Rc::new(Self {
            workspace: RefCell::new(workspace),
            setups: RefCell::new(shown.setups.clone()),
            activity,
            window,
            config,
        });
        let workspace: Rc<dyn WorkspacePort> = stand_in.clone();
        let services: Rc<dyn ServicesPort> = stand_in;
        Ports {
            workspace,
            services,
            outputs,
            terminals: None,
        }
    }

    /// The newest page of a terminal's log before `before`, and whether nothing older is left.
    fn log_page(&self, terminal: &TerminalId, before: Option<u64>) -> (Vec<ActivityEntry>, bool) {
        let Some(log) = self.activity.logs.get(terminal) else {
            return (Vec::new(), true);
        };
        let mut kept = Vec::new();
        for entry in log {
            let older = match before {
                Some(before) => entry.seq < before,
                None => true,
            };
            if older {
                kept.push(entry.clone());
            }
        }
        let start = kept.len().saturating_sub(self.activity.page);
        let page = kept.split_off(start);
        let complete = kept.is_empty();
        (page, complete)
    }

    fn publish_setups(&self) {
        let view = self.setups.borrow().clone();
        let output = ServicesOutput::Setups(view);
        self.publish(WindowOutput::Services(output));
    }

    // A send fails only once the window is gone, when there is nothing left to draw.
    fn publish(&self, output: WindowOutput) {
        let _ = self.window.unbounded_send(output);
    }
}

impl WorkspacePort for StandIn {
    fn send(&self, request: WorkspaceRequest, token: Option<u64>) {
        let now = now_ms();
        let mut workspace = self.workspace.borrow_mut();
        let (reply, out) = workspace.request(request, now);
        let states = delivered(&mut workspace, out, now);
        drop(workspace);
        for state in states {
            self.publish(WindowOutput::State(state));
        }
        let Some(token) = token else {
            return;
        };
        self.publish(WindowOutput::Replied { token, reply });
    }

    fn activity(&self, request: ActivityRequest) {
        let event = match request {
            ActivityRequest::Watch { terminal: None } => return,
            ActivityRequest::Watch {
                terminal: Some(terminal),
            } => {
                let (entries, complete) = self.log_page(&terminal, None);
                let mut board = Board::default();
                if let Some(kept) = self.activity.boards.get(&terminal) {
                    board = kept.clone();
                }
                ActivityEvent::Loaded {
                    terminal,
                    entries,
                    complete,
                    board,
                }
            }
            ActivityRequest::Older { terminal, before } => {
                let (entries, complete) = self.log_page(&terminal, Some(before));
                ActivityEvent::Older {
                    terminal,
                    entries,
                    complete,
                }
            }
            ActivityRequest::Body { terminal, thing } => {
                let body = self.activity.bodies.get(&thing).cloned();
                ActivityEvent::Body {
                    terminal,
                    thing,
                    body,
                }
            }
        };
        self.publish(WindowOutput::Activity(event));
    }
}

impl ServicesPort for StandIn {
    fn agent_action(&self, harness: &HarnessId, action: AgentMenuAction) {
        if action != AgentMenuAction::SetUp {
            return;
        }
        let mut setups = self.setups.borrow_mut();
        for notice in &mut setups.notices {
            let is_offer = matches!(notice.kind, AgentSetupNoticeKind::Offer { .. });
            if &notice.harness == harness && is_offer {
                notice.kind = AgentSetupNoticeKind::Done;
                notice.text = done_text(PRODUCT_NAME);
            }
        }
        drop(setups);
        self.publish_setups();
    }

    fn dismiss_notice(&self, harness: &HarnessId) {
        let mut setups = self.setups.borrow_mut();
        setups.notices.retain(|notice| &notice.harness != harness);
        drop(setups);
        self.publish_setups();
    }

    fn apply_update(&self) {
        self.publish(WindowOutput::Services(ServicesOutput::QuitForUpdate));
    }

    fn write_setting(&self, change: SettingChange) {
        let key = change.key();
        let Some(config) = &self.config else {
            let reason = REFUSED_WRITE.to_string();
            let refused = ServicesOutput::SettingRefused { key, reason };
            self.publish(WindowOutput::Services(refused));
            return;
        };
        let mut layer = Map::new();
        layer.insert(key.to_string(), change.value());
        let mut current = config.borrow_mut();
        let applied = apply_settings_layer(&current.settings, &layer, STAND_IN_LABEL);
        current.settings = applied.settings;
        let written = current.clone();
        drop(current);
        let published = ServicesOutput::Config(written);
        self.publish(WindowOutput::Services(published));
    }

    fn write_keybinding(&self, change: KeymapChange) {
        let command = change.command();
        let reason = REFUSED_KEYMAP_WRITE.to_string();
        let refused = ServicesOutput::KeymapRefused { command, reason };
        self.publish(WindowOutput::Services(refused));
    }
}

/// The workspace the stand-in drives, held in memory; its terminals are numbered from t0, as the fixtures name them.
pub(crate) fn stand_in_workspace() -> Workspace {
    let mut next: u64 = 0;
    let new_terminal_id = move || {
        let id = TerminalId(format!("t{next}"));
        next += 1;
        id
    };
    let options = Options {
        settings_path: String::new(),
        keybindings_path: String::new(),
        new_terminal_id: Box::new(new_terminal_id),
        storage: Box::new(Memory::default()),
        restore_on_launch: true,
    };
    Workspace::new(options)
}

/// The states published after an input, each terminal it opened or closed reported started or exited as the runtime would.
pub(crate) fn delivered(
    workspace: &mut Workspace,
    out: Vec<Outbound>,
    now_ms: u64,
) -> Vec<DrawnState> {
    let mut states = Vec::new();
    let mut reports = Vec::new();
    for output in out {
        match output {
            Outbound::State(state) => states.push(state),
            Outbound::Runtime(RuntimeCommand::Open(start)) => {
                let started = started_where_asked(start);
                reports.push(started);
            }
            Outbound::Runtime(RuntimeCommand::Close { id }) => {
                let exited = RuntimeReport::Exited { id, code: 0 };
                reports.push(exited);
            }
            _ => {}
        }
    }
    for report in reports {
        let reported = workspace.runtime_report(report, now_ms);
        let more = delivered(workspace, reported, now_ms);
        states.extend(more);
    }
    states
}

fn started_where_asked(start: TerminalStart) -> RuntimeReport {
    let home = home();
    let mut directory = home.to_string_lossy().into_owned();
    if let Some(asked) = start.directory {
        directory = asked;
    }
    RuntimeReport::Started {
        id: start.id,
        directory,
    }
}

pub(crate) fn home() -> PathBuf {
    let Some(home) = std::env::home_dir() else {
        return PathBuf::from("/");
    };
    home
}
