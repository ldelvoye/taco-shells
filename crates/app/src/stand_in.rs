//! What the window runs on until the workspace is wired to it: the list's requests applied with `rules`, and the result published as the workspace would.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use futures::channel::mpsc::unbounded;
use taco_protocol::{
    ActivityRequest, Arrangement, DrawnState, RequestReply, TerminalId, WorkspaceRequest,
};
use taco_rules::{add_row_to, apply_drop, close_row, select_row, toggle_tab_group};

use crate::link::{WindowOutput, WindowReceiver, WindowSender, WorkspacePort};

pub struct StandIn {
    state: RefCell<DrawnState>,
    window: WindowSender,
    opened: Cell<u64>,
}

impl StandIn {
    /// The port the window sends to, and what it is sent back.
    pub fn start(state: DrawnState) -> (Rc<dyn WorkspacePort>, WindowReceiver) {
        let (window, outputs) = unbounded();
        let stand_in = Self {
            state: RefCell::new(state),
            window,
            opened: Cell::new(0),
        };
        let port: Rc<dyn WorkspacePort> = Rc::new(stand_in);
        (port, outputs)
    }

    fn new_terminal(&self) -> TerminalId {
        let number = self.opened.get() + 1;
        self.opened.set(number);
        TerminalId(format!("opened-{number}"))
    }

    fn apply(&self, request: WorkspaceRequest) -> bool {
        let mut state = self.state.borrow_mut();
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
            // The stand-in answers only what the list sends.
            _ => false,
        };
        if changed {
            state.revision += 1;
            let snapshot = state.clone();
            self.publish(WindowOutput::State(snapshot));
        }
        changed
    }

    // A send fails only once the window is gone, when there is nothing left to draw.
    fn publish(&self, output: WindowOutput) {
        let _ = self.window.unbounded_send(output);
    }
}

impl WorkspacePort for StandIn {
    fn send(&self, request: WorkspaceRequest, token: Option<u64>) {
        let ok = self.apply(request);
        let Some(token) = token else {
            return;
        };
        let revision = self.state.borrow().revision;
        let reply = RequestReply {
            ok,
            tab_group: None,
            revision,
        };
        self.publish(WindowOutput::Replied { token, reply });
    }

    fn activity(&self, _request: ActivityRequest) {}
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
