use taco_agents::harness_of;
use taco_protocol::TerminalId;

use crate::sessions::AgentSessions;

/// Where a new shell starts and what is typed into it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellStart {
    pub directory: Option<String>,
    pub directory_of: Option<TerminalId>,
    pub command: Option<String>,
}

/// A fork of a terminal's agent session where that session lives, or a plain shell in the source's directory when it cannot fork.
pub fn fork_start(sessions: &AgentSessions, source: &TerminalId, name: &str) -> ShellStart {
    let plain = ShellStart {
        directory_of: Some(source.clone()),
        ..ShellStart::default()
    };
    let Some(session) = sessions.session_of(source) else {
        return plain;
    };
    let Some(harness) = harness_of(&session.harness.0) else {
        return plain;
    };
    let Some(command) = harness.fork_command(&session.session_id, name) else {
        return plain;
    };
    ShellStart {
        directory: Some(session.cwd.clone()),
        directory_of: None,
        command: Some(command),
    }
}

// The shell resolves EDITOR, not the app: a GUI launch does not have it, since it is set in the files an interactive shell reads.
pub fn editor_command(path: &str) -> String {
    let quoted = path.replace('\'', "'\\''");
    format!("${{EDITOR:-vi}} '{quoted}'\n")
}
