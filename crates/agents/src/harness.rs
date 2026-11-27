use taco_protocol::{AgentSignal, HarnessId, Peer, PrState, SharedTarget, StartSource, Step};

/// A version as major, minor and patch: (2, 1, 280).
pub type Version = (u32, u32, u32);

/// Where a step was taken: the agent's turn, by its id, and whether one of the agent's subagents took it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepContext {
    pub turn: Option<String>,
    pub subagent: bool,
}

/// What a hook's payload adds to Agent Activity.
#[derive(Clone, Debug, PartialEq)]
pub enum EntryRead {
    Asked {
        text: String,
        /// The id of the agent's turn; a prompt sent while a turn runs carries that turn's.
        turn: Option<String>,
    },
    MessageIn {
        from: Peer,
        text: String,
        turn: Option<String>,
    },
    Step {
        step: Step,
        context: StepContext,
    },
    MessageOut {
        to: Peer,
        text: String,
        context: StepContext,
    },
    Note {
        text: String,
    },
    /// The agent's line on what it is doing now; it replaces the last one and is not logged.
    Now {
        text: String,
    },
    /// A thing the agent shared, with a target to open, a text to show, or both.
    Share {
        kind: String,
        title: String,
        target: Option<SharedTarget>,
        body: Option<String>,
    },
    /// A thing the app found in what the agent did, for Shared.
    Found(Found),
}

/// What the app found to share from one of the agent's tools.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    Thing {
        kind: String,
        title: String,
        target: SharedTarget,
    },
    PullRequest(PullRequestSeen),
}

/// A pull request a gh command touched. `url` is its plain link, which Claude Code gives for a created or edited
/// pull request and not for a merged or closed one, and `state` is none when the action says nothing of it, as an
/// edit does. `kind` and `title` label it in Shared if it is new there.
#[derive(Clone, Debug, PartialEq)]
pub struct PullRequestSeen {
    pub url: Option<String>,
    pub number: u64,
    pub state: Option<PrState>,
    pub kind: String,
    pub title: String,
}

/// A terminal title read as a row name, and whether the agent said it is idle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentTitle {
    pub name: String,
    pub idle: bool,
}

/// Where an agent keeps its settings, and the names the app shows it by and finds it running under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarnessSetup {
    /// The agent's name in the Agents menu and the setup line: "Claude Code".
    pub product_name: &'static str,
    /// The names the agent's program runs under in a terminal: "claude".
    pub commands: &'static [&'static str],
    /// The environment variable that moves the agent's config directory.
    pub config_directory_variable: &'static str,
    /// The config directory's name under home when the variable is unset.
    pub default_config_directory: &'static str,
    pub settings_file: &'static str,
    pub plugin_variable: &'static str,
    pub version_flag: &'static str,
    pub minimum_version: Version,
}

/// What the listener reads from every hook payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookFields {
    pub session_id: String,
    pub event: String,
    pub source: Option<String>,
    pub tool_name: Option<String>,
}

/// What the app knows about one coding agent.
pub struct Harness {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) session_id_check: fn(&str) -> bool,
    pub(crate) signal_reader: fn(&str, Option<&str>) -> Option<AgentSignal>,
    pub(crate) title_reader: Option<fn(&str) -> AgentTitle>,
    pub(crate) resume_line: fn(&str) -> Option<String>,
    pub(crate) fork_line: Option<fn(&str, &str) -> Option<String>>,
    pub(crate) question_reader: Option<fn(&str, &str) -> Option<String>>,
    pub(crate) entry_reader: Option<fn(&str, &str) -> Vec<EntryRead>>,
    pub(crate) version_reader: Option<fn(&str) -> Option<Version>>,
    pub(crate) hook_reader: Option<fn(&str) -> Option<HookFields>>,
    pub(crate) start_source_reader: Option<fn(&str) -> StartSource>,
    pub(crate) setup: Option<&'static HarnessSetup>,
}

impl Harness {
    /// The agent's name on the wire and in the save file: "claude".
    pub fn id(&self) -> HarnessId {
        HarnessId(self.id.to_string())
    }

    /// The agent's name in the menu: "Fork with Claude".
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Whether a value is one of this agent's session ids. An id is typed into a shell, so nothing else may pass.
    pub fn is_session_id(&self, value: &str) -> bool {
        (self.session_id_check)(value)
    }

    /// The signal a hook event stands for, or none for an event that changes nothing.
    pub fn signal_for(&self, event: &str, detail: Option<&str>) -> Option<AgentSignal> {
        (self.signal_reader)(event, detail)
    }

    /// A terminal title as a row name; the raw title, never idle, when the agent's titles carry nothing to read.
    pub fn title_of(&self, title: &str) -> AgentTitle {
        match self.title_reader {
            Some(reader) => reader(title),
            None => AgentTitle {
                name: title.to_string(),
                idle: false,
            },
        }
    }

    /// The line typed into a shell to resume a session, or none to type nothing.
    pub fn resume_command(&self, session_id: &str) -> Option<String> {
        (self.resume_line)(session_id)
    }

    /// Whether the agent's sessions can be forked at all.
    pub fn forks(&self) -> bool {
        self.fork_line.is_some()
    }

    /// The line typed into a shell to fork a session under a name, or none.
    pub fn fork_command(&self, session_id: &str, name: &str) -> Option<String> {
        let line = self.fork_line?;
        line(session_id, name)
    }

    /// The question a hook's payload asks the user, or none when the payload asks nothing.
    pub fn question_of(&self, event: &str, payload: &str) -> Option<String> {
        let reader = self.question_reader?;
        reader(event, payload)
    }

    /// The Agent Activity entries a hook's payload makes, oldest first: one prompt can hold several messages.
    pub fn entries_of(&self, event: &str, payload: &str) -> Vec<EntryRead> {
        let Some(reader) = self.entry_reader else {
            return Vec::new();
        };
        reader(event, payload)
    }

    /// The environment variable the agent reads its plugin folders from: `CLAUDE_CODE_PLUGIN_DIRS`.
    pub fn plugin_variable(&self) -> Option<&'static str> {
        let setup = self.setup?;
        Some(setup.plugin_variable)
    }

    /// The version the agent's version output states, or none when it states none.
    pub fn version_of(&self, output: &str) -> Option<Version> {
        let reader = self.version_reader?;
        reader(output)
    }

    /// The fields of a hook's payload the listener routes by, or none when it names no session.
    pub fn hook_fields(&self, payload: &str) -> Option<HookFields> {
        let reader = self.hook_reader?;
        reader(payload)
    }

    /// How a session started, from the agent's own word for it; a word it does not know counts as a new session.
    pub fn start_source(&self, source: &str) -> StartSource {
        let Some(reader) = self.start_source_reader else {
            return StartSource::New;
        };
        reader(source)
    }

    /// The hooks the app can wire; an agent without them has no Agents menu entry and no setup line.
    pub fn setup(&self) -> Option<&'static HarnessSetup> {
        self.setup
    }
}
