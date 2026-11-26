//! The window's side of its contracts: requests out to the workspace, and everything sent back to the window's thread.

use std::path::PathBuf;
use std::rc::Rc;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use taco_protocol::{
    ActivityEvent, ActivityRequest, DrawnState, HarnessId, RequestReply, TerminalId,
    WorkspaceRequest,
};
use taco_rules::{locate_row, select_row};
use taco_services::{AgentMenuAction, AgentSetupView};
use taco_settings::Config;

pub type WindowSender = UnboundedSender<WindowOutput>;
pub type WindowReceiver = UnboundedReceiver<WindowOutput>;

/// Where the window sends what the user did. The workspace answers by publishing a new state.
pub trait WorkspacePort {
    /// `token`, when given, comes back in `WindowOutput::Replied`, after the state that holds the request's effect.
    fn send(&self, request: WorkspaceRequest, token: Option<u64>);
    fn activity(&self, request: ActivityRequest);
}

/// Where the window sends what the user did about the services: answered by `ServicesOutput`.
pub trait ServicesPort {
    fn agent_action(&self, harness: &HarnessId, action: AgentMenuAction);
    fn dismiss_notice(&self, harness: &HarnessId);
    fn apply_update(&self);
    fn hooks_directory(&self) -> PathBuf;
}

/// What the window sends to, receives from, and tells of a new config.
pub struct Ports {
    pub workspace: Rc<dyn WorkspacePort>,
    pub services: Rc<dyn ServicesPort>,
    pub outputs: WindowReceiver,
    /// Runs on the window's thread for each new config, after the window has taken it.
    pub config_changed: Box<dyn Fn(&Config)>,
}

/// What the services send the window.
pub enum ServicesOutput {
    Config(Config),
    Setups(AgentSetupView),
    UpdateReady {
        version: String,
    },
    UpdateProblem(String),
    /// A problem the window shows as a banner, such as every focus port being taken.
    Problem(String),
    /// The downloaded update is waiting for this app to quit, which goes the way `cmd+q` does.
    QuitForUpdate,
}

/// Everything sent to the window's thread, in the order it is applied.
pub enum WindowOutput {
    State(DrawnState),
    Replied {
        token: u64,
        reply: RequestReply,
    },
    Activity(ActivityEvent),
    /// The last terminal has gone, which ends the app.
    Emptied,
    Services(ServicesOutput),
}

/// A focus move the window made that the workspace has not applied yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ahead {
    pub terminal: TerminalId,
    pub count: u64,
}

/// The number for the window's next focus move: past its own last one and past every move the workspace has applied.
pub fn next_move(last_move: u64, applied: u64) -> u64 {
    let mut latest = last_move;
    if applied > latest {
        latest = applied;
    }
    latest + 1
}

/// The published state with the window's own focus move laid over it, and that move again while the workspace has still not applied it.
pub fn drawn_with(state: &DrawnState, ahead: Option<&Ahead>) -> (DrawnState, Option<Ahead>) {
    let Some(ahead) = ahead else {
        return (state.clone(), None);
    };
    if state.focus_count >= ahead.count {
        return (state.clone(), None);
    }
    let Some(arrangement) = &state.arrangement else {
        return (state.clone(), None);
    };
    if locate_row(arrangement, &ahead.terminal).is_none() {
        return (state.clone(), None);
    }
    let mut drawn = state.clone();
    if let Some(moved) = select_row(arrangement, &ahead.terminal) {
        drawn.arrangement = Some(moved);
    }
    (drawn, Some(ahead.clone()))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use taco_protocol::{Arrangement, DrawnState, Row, TerminalId};
    use taco_rules::{default_panels, selected_terminal};

    use super::{Ahead, drawn_with, next_move};

    #[test]
    fn a_new_focus_move_is_numbered_past_every_move_already_applied() {
        let cases = [
            ("the window is ahead", 5, 3, 6),
            ("the workspace's count passed the window's", 2, 7, 8),
            ("both at the same count", 4, 4, 5),
        ];
        for (case, last_move, applied, next) in cases {
            assert_eq!(next_move(last_move, applied), next, "{case}");
        }
    }

    fn state(focus_count: u64, terminals: &[&str]) -> DrawnState {
        let rows = terminals.iter().map(|name| Row {
            terminal: TerminalId(name.to_string()),
            title: String::new(),
            tab_group: None,
        });
        let arrangement = Arrangement {
            rows: rows.collect(),
            selected: 0,
            tab_groups: Vec::new(),
        };
        DrawnState {
            revision: 1,
            arrangement: Some(arrangement),
            agent_rows: BTreeMap::new(),
            directories: BTreeMap::new(),
            started: BTreeSet::new(),
            refocus: 0,
            focus_count,
            agents: BTreeMap::new(),
            default_agent_name: String::new(),
            panels: default_panels(),
            problem: None,
        }
    }

    fn ahead(terminal: &str, count: u64) -> Ahead {
        Ahead {
            terminal: TerminalId(terminal.to_string()),
            count,
        }
    }

    #[test]
    fn the_window_keeps_its_own_focus_move_until_the_workspace_has_applied_it() {
        let cases = [
            (
                "not applied yet: drawn on b",
                state(2, &["a", "b"]),
                ahead("b", 3),
                "b",
                true,
            ),
            (
                "applied: the workspace's choice wins",
                state(3, &["a", "b"]),
                ahead("b", 3),
                "a",
                false,
            ),
            (
                "its terminal is gone: dropped",
                state(2, &["a", "c"]),
                ahead("b", 3),
                "a",
                false,
            ),
        ];
        for (case, published, move_made, selected, still_ahead) in cases {
            let (drawn, kept) = drawn_with(&published, Some(&move_made));
            let shown = selected_terminal(drawn.arrangement.as_ref()).cloned();
            assert_eq!(shown, Some(TerminalId(selected.to_string())), "{case}");
            assert_eq!(kept.is_some(), still_ahead, "{case}");
        }
    }
}
