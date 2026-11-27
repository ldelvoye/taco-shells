//! What runs behind the window: the runtime process, the workspace and the services, started and stopped in order.

mod ports;
mod runtime_client;
mod services_host;
mod supervised;
mod terminal_settings;
pub mod wiring;
mod workspace_host;

pub use ports::{
    ServicesOutput, ServicesPort, WindowOutput, WindowReceiver, WindowSender, WorkspacePort,
};
