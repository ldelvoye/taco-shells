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

/// A fork of the terminal's newest agent session, in that session's directory, or none when it has no session the agent forks.
pub fn fork_start(sessions: &AgentSessions, source: &TerminalId, name: &str) -> Option<ShellStart> {
    let session = sessions.session_of(source)?;
    let harness = harness_of(&session.harness.0)?;
    let command = harness.fork_command(&session.session_id, name)?;
    let start = ShellStart {
        directory: Some(session.cwd.clone()),
        directory_of: None,
        command: Some(command),
    };
    Some(start)
}

/// The line that opens a file in the user's editor; the shell resolves EDITOR, since a GUI launch does not have it.
pub fn editor_command(path: &str) -> String {
    let quoted = path.replace('\'', "'\\''");
    format!("${{EDITOR:-vi}} '{quoted}'\n")
}
