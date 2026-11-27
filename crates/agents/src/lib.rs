//! What the app knows about each coding agent.

mod claude;
mod harness;

pub use claude::CLAUDE;
pub use harness::{
    AgentTitle, EntryRead, Found, Harness, HarnessSetup, PullRequestSeen, StepContext,
};

pub static HARNESSES: &[&Harness] = &[&CLAUDE];

/// The agent the menu names when a row has none: "Fork with Claude".
pub fn default_harness() -> &'static Harness {
    &CLAUDE
}

/// The agent a save file from before harnesses means: it names none, and held only Claude sessions.
pub fn legacy_harness() -> &'static Harness {
    &CLAUDE
}

pub fn harness_of(id: &str) -> Option<&'static Harness> {
    let mut known = HARNESSES.iter();
    let found = known.find(|harness| harness.id == id)?;
    Some(*found)
}
