//! A stand-in for the workspace, so a fixture can be clicked through: it applies the list's requests with `rules` and publishes the result.

use std::cell::{Cell, RefCell};

use futures::channel::mpsc::UnboundedSender;
use taco_protocol::{Arrangement, DrawnState, TerminalId, WorkspaceRequest};
use taco_rules::{add_row_to, apply_drop, close_row, select_row, toggle_tab_group};

use crate::link::WorkspacePort;

pub struct StandIn {
    state: RefCell<DrawnState>,
    published: UnboundedSender<DrawnState>,
    opened: Cell<u64>,
}

impl StandIn {
    pub fn new(state: DrawnState, published: UnboundedSender<DrawnState>) -> Self {
        Self {
            state: RefCell::new(state),
            published,
            opened: Cell::new(0),
        }
    }

    fn new_terminal(&self) -> TerminalId {
        let number = self.opened.get() + 1;
        self.opened.set(number);
        TerminalId(format!("opened-{number}"))
    }
}

impl WorkspacePort for StandIn {
    fn send(&self, request: WorkspaceRequest) {
        let mut state = self.state.borrow_mut();
        let changed = match request {
            WorkspaceRequest::FocusReported { terminal, count } => {
                if count > state.focus_count {
                    state.focus_count = count;
                }
                rearrange(&mut state, |before| select_row(before, &terminal));
                true
            }
            WorkspaceRequest::CloseTerminal { terminal } => {
                state.agent_rows.remove(&terminal);
                state.directories.remove(&terminal);
                rearrange(&mut state, |before| close_row(before, &terminal))
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
            _ => false,
        };
        if !changed {
            return;
        }
        state.revision += 1;
        let snapshot = state.clone();
        // A send fails only once the window is gone, when there is nothing left to draw.
        let _ = self.published.unbounded_send(snapshot);
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
