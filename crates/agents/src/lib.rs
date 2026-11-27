//! What the app knows about each coding agent.

mod claude;
mod harness;

pub use claude::CLAUDE;
pub use claude::{
    ACTIVITY_SERVER, ACTIVITY_TOOLS, NOTE_TOOL, NOW_TOOL, SHARE_KINDS, SHARE_TOOL,
    activity_tool_of, activity_tool_prefix, reduced_payload,
};
pub use harness::{
    AgentTitle, EntryRead, Found, Harness, HarnessSetup, HookFields, PullRequestSeen, StepContext,
};

/// What ends a text the app cut to fit, in the log and in what it hands an agent.
pub const CUT_MARK: &str = "… (cut)";

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
