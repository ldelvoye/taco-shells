//! Made-up states the window can be drawn from.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use taco_protocol::{
    ActivityEntry, ActivityKind, AgentLabel, AgentRow, Arrangement, Author, Board, DrawnState,
    HarnessId, Peer, PrState, Row, RowState, SharedTarget, SharedThing, StartSource, Step,
    TabGroup, TabGroupId, TerminalId,
};
use taco_rules::default_panels;
use taco_services::{AgentMenuAction, AgentSetupNotice, AgentSetupNoticeKind, AgentSetupView};
use taco_settings::{
    Binding, ConfigTexts, SettingChange, Settings, ShellRowName, UnreadSettings, resolve_config,
};

use crate::activity_panel::{MS_PER_MINUTE, now_ms};
use crate::banners::Notices;
use crate::stand_in::StandInActivity;
use crate::window::Shown;

const CLAUDE: &str = "claude";
const STATE_MINUTES_AGO: u64 = 52;
const FONT_SIZE_PROBLEM: &str = "settings.json: \"fontSize\" must be a number between 1 and 72";
const KEYBINDINGS_PROBLEM: &str = "keybindings.json: expected a comma at line 7";
const OFFER_TEXT: &str =
    "Claude Code is running here but isn't set up, so its activity can't show.";
const TOO_OLD_REASON: &str =
    "Agent Activity needs Claude Code 2.1.280 or later; this Mac has 2.1.212.";
const DONE_TEXT: &str =
    "Agents already running stop reporting until you start them in a new terminal.";
// The plain shell in the mixed rows that a setup line is drawn on.
const SHELL_ROW: usize = 4;

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
const SHORT_LOG_PAGE: usize = 6;
const FULL_LOG_PAGE: usize = 200;
// The newest page must start inside a noted stretch: here, the nudged fix's second step.
const CUT_LOG_PAGE: usize = 18;
const NARROW_PANEL: f32 = 140.;
const PLAN_NOTE: &str = "Wrote PLAN-sigkill.md. Today a closed terminal or a stopping runtime gets one SIGHUP and \
nothing after it, so a shell that ignores it keeps running with its children. The plan adds a SIGKILL to the whole \
shell session, 2 s after a terminal is closed and 500 ms after the runtime is told to stop, and a test for each. It \
also covers what could go wrong: a shell that traps SIGHUP to save history gets its 2 s, and a process that left the \
session on purpose, like a daemon started with setsid, is not touched. No code was changed.";
const NUDGED_NOTE: &str = "Hook script paths are now quoted safely for any characters, and plain paths stay \
unquoted as before. All 48 taco-services tests pass, including a new one that runs each quoted path through the \
shell. Nothing is committed.";
const REVIEW_MESSAGE: &str = "Review of #155: faint box lines are painted twice where a line's two halves meet, so a \
dim border reads as dotted. Fix before merging.";
const FIXED_MESSAGE: &str = "#155 is fixed: the arms are cut into rectangles that no longer overlap, so a faint line \
stays solid. Tests and clippy are clean.";
const HEADS_UP: &str = "Heads-up: main moved to 42bf29b while you were rebasing.";
const ADDRESS: &str = "uds:/tmp/cc-socks/9838.sock";
const NOW_LINE: &str = "Rebasing #179 onto main, then resolving the conflict in draw.rs.";
const CHECKOUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const CHECKOUT_IMAGE: &str = "branding/icons/taco-shells.svg";
const CHECKOUT_DOCUMENT: &str = "README.md";
const FINDING_ID: u64 = 1;
/// What the finding shared in the full board holds, sent when it is opened.
const FINDING_BODY: &str = "Closing a terminal sends its shell one SIGHUP and nothing after it.

A shell that ignores SIGHUP keeps running, and so do its children: in a test, `trap '' HUP; sleep 600` outlived the terminal by the full ten minutes.

The runtime does the same when it is told to stop. It sends SIGHUP to each shell's session and exits without waiting, so a process that ignores it is left behind with no terminal.

The plan in PLAN-sigkill.md adds a SIGKILL to the whole session after a short grace period.";

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
    /// The quiet one with plain shells named by their short path.
    Paths,
    /// The quiet one with some switches on and some off.
    Switches,
    /// The quiet one with a keybindings.json that could not be read.
    BrokenKeys,
    /// The switches one, whose settings writes are applied, so a click moves its control.
    Writable,
    /// The quiet one whose working agent has written every kind of log line.
    Activity,
    /// The same with Agent Activity at its narrowest.
    ActivityNarrow,
    /// The same log sent a page at a time, the newest page starting inside a noted stretch.
    Paged,
    /// The quiet one on a plain shell where Claude Code runs but Agent Activity is off.
    SetupOff,
    /// The activity one with the old hooks still set up, so the offer shows beside a running agent.
    SetupOld,
    /// The quiet one on a plain shell where Claude Code is too old for Agent Activity.
    SetupTooOld,
    /// The activity one just after Set up, with the hint above the running agent's log.
    SetupDone,
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
            "paths" => Some(Self::Paths),
            "switches" => Some(Self::Switches),
            "brokenkeys" => Some(Self::BrokenKeys),
            "writable" => Some(Self::Writable),
            "activity" => Some(Self::Activity),
            "activitynarrow" => Some(Self::ActivityNarrow),
            "paged" => Some(Self::Paged),
            "setupoff" => Some(Self::SetupOff),
            "setupold" => Some(Self::SetupOld),
            "setuptooold" => Some(Self::SetupTooOld),
            "setupdone" => Some(Self::SetupDone),
            _ => None,
        }
    }

    pub(crate) fn shown(self) -> Shown {
        match self {
            Self::Mixed => mixed(),
            Self::Quiet => quiet(),
            Self::Empty => empty(),
            Self::Broken => broken(),
            Self::Paths => paths(),
            Self::Switches => switches(),
            Self::BrokenKeys => broken_keys(),
            Self::Writable => switches(),
            Self::Activity | Self::Paged => quiet(),
            Self::ActivityNarrow => narrow_activity(),
            Self::SetupOff => setup_on_shell(offer_kind()),
            Self::SetupTooOld => setup_on_shell(too_old_kind()),
            Self::SetupOld => setup_beside_agent(offer_kind(), OFFER_TEXT),
            Self::SetupDone => setup_beside_agent(AgentSetupNoticeKind::Done, DONE_TEXT),
        }
    }

    pub(crate) fn applies_writes(self) -> bool {
        self == Self::Writable
    }

    /// The agent logs and boards the stand-in answers from.
    pub(crate) fn activity(self) -> StandInActivity {
        let full = matches!(
            self,
            Self::Activity | Self::ActivityNarrow | Self::Paged | Self::SetupOld | Self::SetupDone
        );
        let page = match self {
            Self::Paged => CUT_LOG_PAGE,
            Self::Activity | Self::ActivityNarrow | Self::SetupOld | Self::SetupDone => {
                FULL_LOG_PAGE
            }
            _ => SHORT_LOG_PAGE,
        };
        let mut boards = BTreeMap::new();
        if full {
            boards.insert(TerminalId("t0".to_string()), full_board());
        }
        let mut bodies = BTreeMap::new();
        if full {
            bodies.insert(FINDING_ID, FINDING_BODY.to_string());
        }
        StandInActivity {
            logs: self.logs(full),
            boards,
            bodies,
            page,
        }
    }

    fn logs(self, full: bool) -> BTreeMap<TerminalId, Vec<ActivityEntry>> {
        if self == Self::Empty {
            return BTreeMap::new();
        }
        let mut working = log_of(&WORKING_LOG);
        if full {
            working = full_log();
        }
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
        let at_ms = now.saturating_sub(ago);
        let step = match line.tool {
            ASKED => {
                entries.push(ActivityEntry {
                    seq,
                    at_ms,
                    author: Author::User,
                    kind: ActivityKind::Asked { text },
                });
                continue;
            }
            CHANGED => Step::Changed {
                tool: "Edit".to_string(),
                path: text,
            },
            tool => Step::Did {
                tool: tool.to_string(),
                summary: text,
                failed: None,
            },
        };
        entries.push(ActivityEntry {
            seq,
            at_ms,
            author: Author::App,
            kind: ActivityKind::Step { stretch: seq, step },
        });
    }
    entries
}

/// Writes a log as the workspace does: one open stretch, closed by a note and ended by a new turn.
struct LogWriter {
    entries: Vec<ActivityEntry>,
    open: Option<u64>,
    now: u64,
}

impl LogWriter {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
            open: None,
            now: now_ms(),
        }
    }

    fn next_seq(&self) -> u64 {
        let held = self.entries.len() as u64;
        held + 1
    }

    fn push(&mut self, minutes_ago: u64, author: Author, kind: ActivityKind) {
        let seq = self.next_seq();
        let ago = minutes_ago * MS_PER_MINUTE;
        let at_ms = self.now.saturating_sub(ago);
        self.entries.push(ActivityEntry {
            seq,
            at_ms,
            author,
            kind,
        });
    }

    fn asked(&mut self, minutes_ago: u64, text: &str) {
        self.open = None;
        let text = text.to_string();
        self.push(minutes_ago, Author::User, ActivityKind::Asked { text });
    }

    fn session(&mut self, minutes_ago: u64, source: StartSource) {
        self.open = None;
        let kind = ActivityKind::SessionStarted { source };
        self.push(minutes_ago, Author::App, kind);
    }

    fn heard(&mut self, minutes_ago: u64, from: Peer, text: &str) {
        let text = text.to_string();
        let kind = ActivityKind::MessageIn { text };
        self.push(minutes_ago, Author::OtherAgent(from), kind);
    }

    fn step(&mut self, minutes_ago: u64, step: Step) {
        let next = self.next_seq();
        let stretch = *self.open.get_or_insert(next);
        let kind = ActivityKind::Step { stretch, step };
        self.push(minutes_ago, Author::App, kind);
    }

    /// A step as an earlier build wrote it, a stretch of its own.
    fn old_step(&mut self, minutes_ago: u64, step: Step) {
        let stretch = self.next_seq();
        let kind = ActivityKind::Step { stretch, step };
        self.push(minutes_ago, Author::App, kind);
    }

    fn sent(&mut self, minutes_ago: u64, to: Peer, text: &str) {
        let next = self.next_seq();
        let stretch = *self.open.get_or_insert(next);
        let text = text.to_string();
        let kind = ActivityKind::MessageOut { to, text, stretch };
        self.push(minutes_ago, Author::Agent, kind);
    }

    fn shared(&mut self, minutes_ago: u64, thing: u64, title: &str) {
        let title = title.to_string();
        let kind = ActivityKind::Shared { thing, title };
        self.push(minutes_ago, Author::Agent, kind);
    }

    fn asked_for_note(&mut self, minutes_ago: u64) {
        self.push(minutes_ago, Author::App, ActivityKind::AskedForNote);
    }

    fn note(&mut self, minutes_ago: u64, text: &str) {
        let stretch = self.open.take();
        let text = text.to_string();
        let kind = ActivityKind::Note { text, stretch };
        self.push(minutes_ago, Author::Agent, kind);
    }
}

fn did(tool: &str, summary: &str) -> Step {
    Step::Did {
        tool: tool.to_string(),
        summary: summary.to_string(),
        failed: None,
    }
}

fn failed(tool: &str, summary: &str, error: &str) -> Step {
    Step::Did {
        tool: tool.to_string(),
        summary: summary.to_string(),
        failed: Some(error.to_string()),
    }
}

fn changed(tool: &str, path: &str) -> Step {
    Step::Changed {
        tool: tool.to_string(),
        path: path.to_string(),
    }
}

fn named(name: &str) -> Peer {
    Peer {
        name: Some(name.to_string()),
        address: None,
    }
}

fn addressed(address: &str) -> Peer {
    Peer {
        name: None,
        address: Some(address.to_string()),
    }
}

/// An earlier build's turn, then three sessions of plugin-written work: a plan, a nudged fix, a review round.
fn full_log() -> Vec<ActivityEntry> {
    let mut log = LogWriter::new();
    log.asked(140, "find why test_flaky_retry fails on CI");
    log.old_step(139, did("Glob", "tests/**/test_retry*.py"));
    log.old_step(138, did("Read", "tests/sentry/test_retry.py"));
    log.old_step(130, changed("Edit", "tests/sentry/test_retry.py"));
    log.session(96, StartSource::New);
    log.asked(
        95,
        "Look into what closing a terminal sends, and write a plan. No code.",
    );
    log.step(94, did("Read", "crates/runtime/src/pty.rs"));
    log.step(93, did("Read", "crates/runtime/src/program.rs"));
    log.step(92, did("Grep", "SIGHUP"));
    log.step(92, did("Grep", "kill("));
    log.step(91, did("Grep", "fn shutdown"));
    log.step(88, changed("Write", "PLAN-sigkill.md"));
    log.shared(87, 1, "SIGKILL grace-period plan");
    log.note(87, PLAN_NOTE);
    log.asked(70, "Quote hook script paths safely.");
    log.step(69, did("Read", "crates/services/src/agent_hooks.rs"));
    log.step(66, changed("Edit", "crates/services/src/agent_hooks.rs"));
    log.step(64, did("Bash", "cargo test -p taco-services"));
    log.asked_for_note(62);
    log.note(61, NUDGED_NOTE);
    log.session(50, StartSource::Compacted);
    log.heard(49, named("code-83"), REVIEW_MESSAGE);
    log.step(48, did("Read", "crates/terminal-view/src/shapes.rs"));
    log.step(44, changed("Edit", "crates/terminal-view/src/shapes.rs"));
    log.step(
        43,
        failed(
            "Bash",
            "cargo test -p taco-terminal-view",
            "Exit code 101: test shapes::faint_lines_meet ... FAILED",
        ),
    );
    log.step(42, did("Bash", "cargo test -p taco-terminal-view"));
    log.sent(40, named("code-83"), FIXED_MESSAGE);
    log.asked(30, "Now rebase it onto main.");
    log.step(29, did("Bash", "git fetch origin"));
    log.step(
        28,
        failed(
            "Bash",
            "git rebase origin/main",
            "Exit code 1: CONFLICT (content): Merge conflict in crates/terminal-view/src/draw.rs",
        ),
    );
    log.step(20, did("Read", "crates/terminal-view/src/draw.rs"));
    log.step(15, changed("Edit", "crates/terminal-view/src/draw.rs"));
    log.heard(5, addressed(ADDRESS), HEADS_UP);
    log.step(3, did("Bash", "git rebase --continue"));
    log.entries
}

fn checkout_file(relative: &str) -> String {
    let joined = Path::new(CHECKOUT).join(relative);
    let resolved = fs::canonicalize(&joined);
    let path = match resolved {
        Ok(resolved) => resolved,
        Err(_) => joined,
    };
    path.to_string_lossy().into_owned()
}

/// A now line and one shared thing of each kind, newest first, as the workspace keeps them.
fn full_board() -> Board {
    let now = now_ms();
    let ago = |minutes: u64| now.saturating_sub(minutes * MS_PER_MINUTE);
    let thing = |id: u64, minutes: u64, by: Author, kind: &str, title: &str, target| SharedThing {
        id,
        at_ms: ago(minutes),
        by,
        kind: kind.to_string(),
        title: title.to_string(),
        target,
    };
    let merged = SharedTarget::PullRequest {
        url: Some("https://github.com/ldelvoye/taco-shells/pull/155".to_string()),
        number: 155,
        state: PrState::Merged,
        checked_ms: None,
    };
    let open = SharedTarget::PullRequest {
        url: Some("https://github.com/ldelvoye/taco-shells/pull/179".to_string()),
        number: 179,
        state: PrState::Open,
        checked_ms: Some(ago(6)),
    };
    let link = SharedTarget::Link {
        url: "https://claude.ai/artifact/9NXFTKtkNKmvbxjMDsn1zL".to_string(),
    };
    let image = SharedTarget::File {
        path: checkout_file(CHECKOUT_IMAGE),
    };
    let document = SharedTarget::File {
        path: checkout_file(CHECKOUT_DOCUMENT),
    };
    let finding_length = FINDING_BODY.chars().count() as u64;
    let text = SharedTarget::Text {
        characters: finding_length,
    };
    let shared = vec![
        thing(
            6,
            8,
            Author::App,
            "Pull request",
            "#179 ldelvoye/taco-shells",
            open,
        ),
        thing(
            5,
            20,
            Author::OtherAgent(named("code-83")),
            "Review",
            "What was approved for Agent Activity",
            link,
        ),
        thing(
            4,
            38,
            Author::Agent,
            "Images",
            "The logo before and after",
            image,
        ),
        thing(
            3,
            44,
            Author::App,
            "Pull request",
            "#155 Draw block and box characters as shapes",
            merged,
        ),
        thing(
            2,
            86,
            Author::Agent,
            "Plan",
            "SIGKILL grace-period plan",
            document,
        ),
        thing(
            FINDING_ID,
            87,
            Author::Agent,
            "Finding",
            "Why closing a terminal leaves its shell running",
            text,
        ),
    ];
    Board {
        now: Some(NOW_LINE.to_string()),
        shared,
    }
}

fn narrow_activity() -> Shown {
    let mut shown = quiet();
    shown.drawn.panels.agent_activity.size = NARROW_PANEL;
    shown
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
        default_bindings: default_bindings(),
        unread_keybindings: false,
        settings: Settings::default(),
        unread: UnreadSettings::None,
    }
}

fn quiet() -> Shown {
    let mut shown = mixed();
    shown.notices = Notices::default();
    shown
}

fn broken_keys() -> Shown {
    let mut shown = quiet();
    shown.notices.config_problems = vec![KEYBINDINGS_PROBLEM.to_string()];
    shown.unread_keybindings = true;
    shown
}

fn switches() -> Shown {
    let mut shown = quiet();
    shown.settings.restore_on_launch = false;
    shown
}

fn paths() -> Shown {
    let mut shown = quiet();
    shown.settings.shell_row_name = ShellRowName::Path;
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
        default_bindings: default_bindings(),
        unread_keybindings: false,
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

fn offer_kind() -> AgentSetupNoticeKind {
    AgentSetupNoticeKind::Offer {
        action: AgentMenuAction::SetUp,
    }
}

fn too_old_kind() -> AgentSetupNoticeKind {
    let reason = TOO_OLD_REASON.to_string();
    AgentSetupNoticeKind::Unavailable { reason }
}

fn one_notice(kind: AgentSetupNoticeKind, text: &str, terminal: &str) -> AgentSetupView {
    let notice = AgentSetupNotice {
        harness: HarnessId(CLAUDE.to_string()),
        kind,
        text: text.to_string(),
        terminals: vec![TerminalId(terminal.to_string())],
    };
    AgentSetupView {
        menus: Vec::new(),
        notices: vec![notice],
    }
}

/// The quiet one with a plain shell selected and a setup notice naming it.
fn setup_on_shell(kind: AgentSetupNoticeKind) -> Shown {
    let mut shown = quiet();
    let terminal = format!("t{SHELL_ROW}");
    shown.setups = one_notice(kind, OFFER_TEXT, &terminal);
    if let Some(arrangement) = &mut shown.drawn.arrangement {
        arrangement.selected = SHELL_ROW;
    }
    shown
}

/// The quiet one, whose working agent is selected, with a setup notice naming its terminal.
fn setup_beside_agent(kind: AgentSetupNoticeKind, text: &str) -> Shown {
    let mut shown = quiet();
    shown.setups = one_notice(kind, text, "t0");
    shown
}

fn offered_setup() -> AgentSetupView {
    let notice = AgentSetupNotice {
        harness: HarnessId(CLAUDE.to_string()),
        kind: AgentSetupNoticeKind::Offer {
            action: AgentMenuAction::SetUp,
        },
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
