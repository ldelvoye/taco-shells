//! The window's contracts with the hosts: requests out to the workspace and the services, and everything sent back to the window's thread.

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use taco_protocol::{
    ActivityEvent, ActivityRequest, DrawnState, HarnessId, RequestReply, WorkspaceRequest,
};
use taco_services::{AgentMenuAction, AgentSetupView};
use taco_settings::{CommandId, Config, KeymapChange, SettingChange};

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
    /// Answered by the next `Config` when it lands, or by `SettingRefused` when it does not.
    fn write_setting(&self, change: SettingChange);
    /// Answered by the next `Config` when it lands, or by `KeymapRefused` when it is refused.
    fn write_keybinding(&self, change: KeymapChange);
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
    /// A setting the page asked to write that was not written, named as in `settings.json`, with a reason the page can show.
    SettingRefused {
        key: &'static str,
        reason: String,
    },
    /// A shortcut change the page asked for that was not written, with a reason the page can show.
    KeymapRefused {
        command: CommandId,
        reason: String,
    },
}

/// Everything sent to the window's thread, in the order it is applied.
pub enum WindowOutput {
    State(DrawnState),
    Replied { token: u64, reply: RequestReply },
    Activity(ActivityEvent),
    Services(ServicesOutput),
}
