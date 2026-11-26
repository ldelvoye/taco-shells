//! Made-up states the window can be drawn from.

use std::collections::BTreeMap;
use std::path::PathBuf;

use taco_protocol::{
    AgentLabel, AgentRow, Arrangement, DrawnState, HarnessId, Row, RowState, TabGroup, TabGroupId,
    TerminalId,
};
use taco_rules::default_panels;

use crate::banners::Banner;
use crate::window::Shown;

const CLAUDE: &str = "claude";
// Fixed so two images of one fixture are the same.
const SOME_MOMENT_MS: u64 = 1_764_100_000_000;

const PLACEHOLDER_SCREEN: [&str; 8] = [
    "~/code/getsentry % claude",
    "",
    "> rework the billing platform lock",
    "",
    "  Reading billing/platform/lock.py",
    "  Running the billing tests: 214 passed",
    "",
    "  Rebasing onto master",
];

const MIXED_ROWS: [FixtureRow; 7] = [
    FixtureRow {
        title: "claude: lock rework",
        tab_group: Some("billing"),
        state: Some(RowState::Working),
        directory: "code/getsentry",
    },
    FixtureRow {
        title: "zsh",
        tab_group: Some("billing"),
        state: None,
        directory: "code/getsentry",
    },
    FixtureRow {
        title: "claude: review PR",
        tab_group: None,
        state: Some(RowState::Needs),
        directory: "code/sentry",
    },
    FixtureRow {
        title: "claude: flaky test",
        tab_group: None,
        state: Some(RowState::Done),
        directory: "code/sentry",
    },
    FixtureRow {
        title: "zsh",
        tab_group: None,
        state: None,
        directory: "code",
    },
    FixtureRow {
        title: "aider",
        tab_group: None,
        state: None,
        directory: "code/smorg",
    },
    FixtureRow {
        title: "claude: docs",
        tab_group: None,
        state: Some(RowState::Idle),
        directory: "code/sentry-docs",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixture {
    /// A tab group, agents in each state, plain shells, and two banners.
    Mixed,
    /// The same, without the banners.
    Quiet,
    /// No terminals and no banners.
    Empty,
}

struct FixtureRow {
    title: &'static str,
    tab_group: Option<&'static str>,
    state: Option<RowState>,
    directory: &'static str,
}

impl Fixture {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "mixed" => Some(Self::Mixed),
            "quiet" => Some(Self::Quiet),
            "empty" => Some(Self::Empty),
            _ => None,
        }
    }

    pub(crate) fn shown(self) -> Shown {
        match self {
            Self::Mixed => mixed(),
            Self::Quiet => quiet(),
            Self::Empty => empty(),
        }
    }
}

fn mixed() -> Shown {
    let mut drawn = empty_drawn();
    let mut rows = Vec::new();
    for (index, fixture_row) in MIXED_ROWS.iter().enumerate() {
        let terminal = TerminalId(format!("t{index}"));
        let tab_group = fixture_row
            .tab_group
            .map(|name| TabGroupId(name.to_string()));
        rows.push(Row {
            terminal: terminal.clone(),
            title: fixture_row.title.to_string(),
            tab_group,
        });
        let directory = home().join(fixture_row.directory);
        let directory_text = directory.to_string_lossy().into_owned();
        drawn.directories.insert(terminal.clone(), directory_text);
        if let Some(state) = fixture_row.state {
            drawn.agent_rows.insert(terminal, agent_row(index, state));
        }
    }
    let billing = TabGroup {
        id: TabGroupId("billing".to_string()),
        name: "billing".to_string(),
        collapsed: false,
    };
    drawn.arrangement = Some(Arrangement {
        rows,
        selected: 0,
        tab_groups: vec![billing],
    });
    let update = Banner {
        text: "Taco Shells 0.27.0 is ready.".to_string(),
        action: Some("Restart to update".to_string()),
    };
    let problem = Banner {
        text: "Could not read settings.json: line 4.".to_string(),
        action: None,
    };
    let screen = PLACEHOLDER_SCREEN.iter().map(|line| line.to_string());
    Shown {
        drawn,
        banners: vec![update, problem],
        placeholder_screen: screen.collect(),
    }
}

fn quiet() -> Shown {
    let mut shown = mixed();
    shown.banners.clear();
    shown
}

fn empty() -> Shown {
    Shown {
        drawn: empty_drawn(),
        banners: Vec::new(),
        placeholder_screen: Vec::new(),
    }
}

fn empty_drawn() -> DrawnState {
    let claude = AgentLabel {
        name: "Claude Code".to_string(),
        forks: true,
    };
    let agents = BTreeMap::from([(HarnessId(CLAUDE.to_string()), claude)]);
    DrawnState {
        revision: 1,
        arrangement: None,
        agent_rows: BTreeMap::new(),
        directories: BTreeMap::new(),
        refocus: 0,
        focus_count: 0,
        agents,
        default_agent_name: "Claude Code".to_string(),
        panels: default_panels(),
        problem: None,
    }
}

fn agent_row(index: usize, state: RowState) -> AgentRow {
    let mut question = None;
    if state == RowState::Needs {
        question = Some("Can I force-push the rebased branch?".to_string());
    }
    AgentRow {
        harness: HarnessId(CLAUDE.to_string()),
        session_id: format!("session-{index}"),
        state,
        state_since_ms: SOME_MOMENT_MS,
        has_conversation: true,
        question,
    }
}

fn home() -> PathBuf {
    let Some(home) = std::env::home_dir() else {
        return PathBuf::from("/");
    };
    home
}
