use taco_agents::harness_of;
use taco_protocol::TerminalId;

use crate::sessions::{AgentSession, AgentSessions};

/// Where a new shell starts and what is typed into it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellStart {
    pub directory: Option<String>,
    pub command: Option<String>,
}

/// A fork of the terminal's newest agent session, in that session's directory, or none when it has no session the agent forks.
pub fn fork_start(sessions: &AgentSessions, source: &TerminalId, name: &str) -> Option<ShellStart> {
    let session = sessions.session_of(source)?;
    let harness = harness_of(&session.harness.0)?;
    let command = harness.fork_command(&session.session_id, name)?;
    let start = ShellStart {
        directory: Some(session.cwd.clone()),
        command: Some(command),
    };
    Some(start)
}

/// A restored agent session resumed where it runs; a plain shell there when its agent is gone or will not resume the id.
pub fn resume_start(session: &AgentSession) -> ShellStart {
    let mut command = None;
    if let Some(harness) = harness_of(&session.harness.0) {
        command = harness.resume_command(&session.session_id);
    }
    ShellStart {
        directory: Some(session.cwd.clone()),
        command,
    }
}

/// The line that opens a file in the user's editor; the shell resolves EDITOR, since a GUI launch does not have it.
pub fn editor_command(path: &str) -> String {
    let quoted = path.replace('\'', "'\\''");
    format!("${{EDITOR:-vi}} '{quoted}'\n")
}
