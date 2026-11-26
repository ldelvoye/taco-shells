//! Made-up states the window can be drawn from.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use taco_protocol::{
    ActivityEntry, ActivityKind, AgentLabel, AgentRow, Arrangement, DrawnState, HarnessId, Row,
    RowState, TabGroup, TabGroupId, TerminalId,
};
use taco_rules::default_panels;
use taco_services::{AgentSetupNotice, AgentSetupNoticeKind, AgentSetupView};
use taco_settings::{
    Binding, ConfigTexts, SettingChange, Settings, UnreadSettings, resolve_config,
};

use crate::activity::{MS_PER_MINUTE, now_ms};
use crate::banners::Notices;
use crate::window::Shown;

const CLAUDE: &str = "claude";
const STATE_MINUTES_AGO: u64 = 52;
const FONT_SIZE_PROBLEM: &str = "settings.json: \"fontSize\" must be a number between 1 and 72";
const OFFER_TEXT: &str =
    "Claude Code is running here but isn't set up, so its activity can't show.";

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

const ASKED: &str = "asked";
const CHANGED: &str = "changed";

const WORKING_LOG: [FixtureEntry; 10] = [
    entry(42, ASKED, "rework the billing platform lock"),
    entry(41, "Grep", "acquire_lock"),
    entry(40, "Read", "billing/platform/lock.py"),
    entry(40, "Read", "billing/platform/models.py"),
    entry(39, "Read", "billing/tests/test_lock.py"),
    entry(31, CHANGED, "billing/platform/lock.py"),
    entry(30, CHANGED, "billing/tests/test_lock.py"),
    entry(22, "Bash", "pytest billing/tests"),
    entry(4, ASKED, "now rebase it onto master"),
    entry(3, "Bash", "git rebase master"),
];

const NEEDS_LOG: [FixtureEntry; 3] = [
    entry(18, ASKED, "address the review comments on the PR"),
    entry(12, CHANGED, "src/sentry/api/endpoints/project_details.py"),
    entry(9, "Bash", "git rebase origin/master"),
];

const DONE_LOG: [FixtureEntry; 3] = [
    entry(75, ASKED, "find why test_flaky_retry fails on CI"),
    entry(70, "Glob", "tests/**/test_retry*.py"),
    entry(52, CHANGED, "tests/sentry/test_retry.py"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixture {
    /// A tab group, agents in each state, plain shells, and two banners.
    Mixed,
    /// The same, without the banners.
    Quiet,
    /// No terminals and no banners.
    Empty,
    /// The quiet one with a settings file whose font size could not be read.
    Broken,
}

struct FixtureRow {
    title: &'static str,
    tab_group: Option<&'static str>,
    state: Option<RowState>,
    directory: &'static str,
}

/// One log entry: `tool` is a tool's name, or `ASKED` or `CHANGED`.
struct FixtureEntry {
    minutes_ago: u64,
    tool: &'static str,
    text: &'static str,
}

impl Fixture {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "mixed" => Some(Self::Mixed),
            "quiet" => Some(Self::Quiet),
            "empty" => Some(Self::Empty),
            "broken" => Some(Self::Broken),
            _ => None,
        }
    }

    pub(crate) fn shown(self) -> Shown {
        match self {
            Self::Mixed => mixed(),
            Self::Quiet => quiet(),
            Self::Empty => empty(),
            Self::Broken => broken(),
        }
    }

    /// The agent logs the stand-in answers from, by terminal.
    pub(crate) fn logs(self) -> BTreeMap<TerminalId, Vec<ActivityEntry>> {
        if self == Self::Empty {
            return BTreeMap::new();
        }
        let working = log_of(&WORKING_LOG);
        let needs = log_of(&NEEDS_LOG);
        let done = log_of(&DONE_LOG);
        BTreeMap::from([
            (TerminalId("t0".to_string()), working),
            (TerminalId("t2".to_string()), needs),
            (TerminalId("t3".to_string()), done),
        ])
    }
}

const fn entry(minutes_ago: u64, tool: &'static str, text: &'static str) -> FixtureEntry {
    FixtureEntry {
        minutes_ago,
        tool,
        text,
    }
}

fn log_of(lines: &[FixtureEntry]) -> Vec<ActivityEntry> {
    let now = now_ms();
    let mut entries = Vec::new();
    let numbered = (1..).zip(lines);
    for (seq, line) in numbered {
        let ago = line.minutes_ago * MS_PER_MINUTE;
        let text = line.text.to_string();
        let kind = match line.tool {
            ASKED => ActivityKind::Asked { text },
            CHANGED => ActivityKind::Changed {
                tool: "Edit".to_string(),
                path: text,
            },
            tool => ActivityKind::Did {
                tool: tool.to_string(),
                summary: text,
            },
        };
        entries.push(ActivityEntry {
            seq,
            at_ms: now.saturating_sub(ago),
            kind,
        });
    }
    entries
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
        drawn.started.insert(terminal.clone());
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
    let notices = Notices {
        config_problems: vec!["Could not read settings.json: line 4.".to_string()],
        update: Some("0.27.0".to_string()),
        ..Notices::default()
    };
    let screen = PLACEHOLDER_SCREEN.iter().map(|line| line.to_string());
    Shown {
        drawn,
        notices,
        placeholder_screen: screen.collect(),
        setups: offered_setup(),
        bindings: default_bindings(),
        settings: Settings::default(),
        unread: UnreadSettings::None,
    }
}

fn quiet() -> Shown {
    let mut shown = mixed();
    shown.notices = Notices::default();
    shown
}

fn broken() -> Shown {
    let mut shown = quiet();
    let font_size = SettingChange::FontSize(Settings::default().font_size);
    shown.notices.config_problems = vec![FONT_SIZE_PROBLEM.to_string()];
    shown.unread = UnreadSettings::Keys(vec![font_size.key()]);
    shown
}

fn empty() -> Shown {
    Shown {
        drawn: empty_drawn(),
        notices: Notices::default(),
        placeholder_screen: Vec::new(),
        setups: AgentSetupView::default(),
        bindings: default_bindings(),
        settings: Settings::default(),
        unread: UnreadSettings::None,
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
        started: BTreeSet::new(),
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
        state_since_ms: now_ms().saturating_sub(STATE_MINUTES_AGO * MS_PER_MINUTE),
        has_conversation: true,
        question,
    }
}

fn default_bindings() -> Vec<Binding> {
    let no_files = ConfigTexts {
        settings: None,
        keybindings: None,
    };
    let config = resolve_config(&no_files, false);
    config.bindings
}

fn offered_setup() -> AgentSetupView {
    let notice = AgentSetupNotice {
        harness: HarnessId(CLAUDE.to_string()),
        kind: AgentSetupNoticeKind::Offer,
        text: OFFER_TEXT.to_string(),
        terminals: vec![TerminalId("t4".to_string())],
    };
    AgentSetupView {
        menus: Vec::new(),
        notices: vec![notice],
    }
}

fn home() -> PathBuf {
    let Some(home) = std::env::home_dir() else {
        return PathBuf::from("/");
    };
    home
}
