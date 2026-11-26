use taco_agents::harness_of;
use taco_protocol::TerminalId;

use crate::sessions::AgentSessions;

/// Where a new shell starts and what is typed into it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellStart {
    pub directory: Option<String>,
    /// A terminal whose directory to start in, read by the runtime when the shell starts; used only without `directory`.
    pub directory_of: Option<TerminalId>,
    pub command: Option<String>,
}

/// A fork of the terminal's agent session, or a plain shell beside it when it has none to fork.
pub fn fork_start(sessions: &AgentSessions, source: &TerminalId, name: &str) -> ShellStart {
    let beside_source = ShellStart {
        directory_of: Some(source.clone()),
        ..ShellStart::default()
    };
    let Some(session) = sessions.session_of(source) else {
        return beside_source;
    };
    let Some(harness) = harness_of(&session.harness.0) else {
        return beside_source;
    };
    if !harness.forks() {
        return beside_source;
    }
    // an id the agent will not fork still names where its session runs
    ShellStart {
        directory: Some(session.cwd.clone()),
        directory_of: None,
        command: harness.fork_command(&session.session_id, name),
    }
}

/// The line that opens a file in the user's editor; the shell resolves EDITOR, since a GUI launch does not have it.
pub fn editor_command(path: &str) -> String {
    let quoted = path.replace('\'', "'\\''");
    format!("${{EDITOR:-vi}} '{quoted}'\n")
}
