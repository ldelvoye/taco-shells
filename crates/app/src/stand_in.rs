//! What the window runs on until the workspace and the services are wired to it: requests applied with `rules`, fixture logs and setup notices, published as they would be.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use futures::channel::mpsc::unbounded;
use taco_protocol::{
    ActivityEntry, ActivityEvent, ActivityRequest, Arrangement, DrawnState, HarnessId, Panels,
    RequestReply, TabGroupId, TerminalId, WorkspaceRequest,
};
use taco_rules::{
    add_row_to, apply_drop, close_panel, close_row, create_tab_group, fork_row, move_panel,
    open_panel, rename_tab_group, resize_panel, select_row, toggle_tab_group, ungroup_tab_group,
};
use taco_services::{AgentMenuAction, AgentSetupNoticeKind, AgentSetupView};

use crate::link::{
    ServicesOutput, ServicesPort, WindowOutput, WindowReceiver, WindowSender, WorkspacePort,
};
use crate::window::Shown;

const LOG_PAGE: usize = 6;
const SET_UP_TEXT: &str = "Claude Code is set up. Its activity shows here from its next session.";

pub struct StandIn {
    state: RefCell<DrawnState>,
    setups: RefCell<AgentSetupView>,
    logs: BTreeMap<TerminalId, Vec<ActivityEntry>>,
    window: WindowSender,
    opened: Cell<u64>,
}

/// The ports the window sends to, and what it is sent back.
pub struct StandInPorts {
    pub workspace: Rc<dyn WorkspacePort>,
    pub services: Rc<dyn ServicesPort>,
    pub outputs: WindowReceiver,
}

impl StandIn {
    pub fn start(shown: &Shown, logs: BTreeMap<TerminalId, Vec<ActivityEntry>>) -> StandInPorts {
        let (window, outputs) = unbounded();
        let stand_in = Rc::new(Self {
            state: RefCell::new(shown.drawn.clone()),
            setups: RefCell::new(shown.setups.clone()),
            logs,
            window,
            opened: Cell::new(0),
        });
        let workspace: Rc<dyn WorkspacePort> = stand_in.clone();
        let services: Rc<dyn ServicesPort> = stand_in;
        StandInPorts {
            workspace,
            services,
            outputs,
        }
    }

    /// The newest page of a terminal's log before `before`, and whether nothing older is left.
    fn log_page(&self, terminal: &TerminalId, before: Option<u64>) -> (Vec<ActivityEntry>, bool) {
        let Some(log) = self.logs.get(terminal) else {
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
        let start = kept.len().saturating_sub(LOG_PAGE);
        let page = kept.split_off(start);
        let complete = kept.is_empty();
        (page, complete)
    }

    fn publish_setups(&self) {
        let view = self.setups.borrow().clone();
        let output = ServicesOutput::Setups(view);
        self.publish(WindowOutput::Services(output));
    }

    fn new_terminal(&self) -> TerminalId {
        let number = self.opened.get() + 1;
        self.opened.set(number);
        TerminalId(format!("opened-{number}"))
    }

    fn new_tab_group(&self) -> TabGroupId {
        let number = self.opened.get() + 1;
        self.opened.set(number);
        TabGroupId(format!("group-{number}"))
    }

    /// Whether the request changed anything, and the tab group it made, if it made one.
    fn apply(&self, request: WorkspaceRequest) -> (bool, Option<TabGroupId>) {
        let mut state = self.state.borrow_mut();
        let mut made = None;
        let changed = match request {
            WorkspaceRequest::FocusReported { terminal, count } => {
                if count > state.focus_count {
                    state.focus_count = count;
                }
                rearrange(&mut state, |before| select_row(before, &terminal));
                // Published even when the selection stood still: the window waits on the focus count.
                true
            }
            WorkspaceRequest::CloseTerminal { terminal } => {
                let closed = rearrange(&mut state, |before| close_row(before, &terminal));
                if closed {
                    state.agent_rows.remove(&terminal);
                    state.directories.remove(&terminal);
                }
                closed
            }
            WorkspaceRequest::TabGroupToggled { id } => {
                rearrange(&mut state, |before| toggle_tab_group(before, &id))
            }
            WorkspaceRequest::OpenTerminalIn { tab_group } => {
                let terminal = self.new_terminal();
                rearrange(&mut state, |before| add_row_to(before, terminal, tab_group))
            }
            WorkspaceRequest::Dropped { dragged, place } => {
                rearrange(&mut state, |before| apply_drop(before, &dragged, &place))
            }
            WorkspaceRequest::ForkTerminal { terminal } => {
                let fork = self.new_terminal();
                rearrange(&mut state, |before| fork_row(before, &terminal, fork))
            }
            WorkspaceRequest::TabGroupCreated { terminal } => {
                let id = self.new_tab_group();
                let created = rearrange(&mut state, |before| {
                    create_tab_group(before, &terminal, id.clone())
                });
                if created {
                    made = Some(id);
                }
                created
            }
            WorkspaceRequest::TabGroupRenamed { id, name } => {
                rearrange(&mut state, |before| rename_tab_group(before, &id, &name))
            }
            WorkspaceRequest::TabGroupUngrouped { id } => {
                rearrange(&mut state, |before| ungroup_tab_group(before, &id))
            }
            WorkspaceRequest::PanelOpened { panel } => {
                repanel(&mut state, |before| open_panel(before, panel))
            }
            WorkspaceRequest::PanelClosed { panel } => {
                repanel(&mut state, |before| close_panel(before, panel))
            }
            WorkspaceRequest::PanelMoved { panel, edge } => {
                repanel(&mut state, |before| move_panel(before, panel, edge))
            }
            WorkspaceRequest::PanelResized { panel, size } => {
                repanel(&mut state, |before| resize_panel(before, panel, size))
            }
            // The stand-in answers only what the window sends.
            _ => false,
        };
        if changed {
            state.revision += 1;
            let snapshot = state.clone();
            self.publish(WindowOutput::State(snapshot));
        }
        (changed, made)
    }

    // A send fails only once the window is gone, when there is nothing left to draw.
    fn publish(&self, output: WindowOutput) {
        let _ = self.window.unbounded_send(output);
    }
}

impl WorkspacePort for StandIn {
    fn send(&self, request: WorkspaceRequest, token: Option<u64>) {
        let (ok, tab_group) = self.apply(request);
        let Some(token) = token else {
            return;
        };
        let revision = self.state.borrow().revision;
        let reply = RequestReply {
            ok,
            tab_group,
            revision,
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
                ActivityEvent::Loaded {
                    terminal,
                    entries,
                    complete,
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
            if &notice.harness == harness && notice.kind == AgentSetupNoticeKind::Offer {
                notice.kind = AgentSetupNoticeKind::Done;
                notice.text = SET_UP_TEXT.to_string();
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
}

fn rearrange(
    state: &mut DrawnState,
    change: impl FnOnce(&Arrangement) -> Option<Arrangement>,
) -> bool {
    let Some(before) = &state.arrangement else {
        return false;
    };
    let Some(after) = change(before) else {
        return false;
    };
    state.arrangement = Some(after);
    true
}

fn repanel(state: &mut DrawnState, change: impl FnOnce(&Panels) -> Option<Panels>) -> bool {
    let Some(after) = change(&state.panels) else {
        return false;
    };
    state.panels = after;
    true
}
