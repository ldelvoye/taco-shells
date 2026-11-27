use taco_protocol::{ActivityEvent, DrawnState, RuntimeCommand};

/// What has to go out after an input: to the runtime, to the window, or to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Outbound {
    Runtime(RuntimeCommand),
    State(DrawnState),
    /// For Agent Activity, on its own stream rather than in the state published whole.
    Activity(ActivityEvent),
}
