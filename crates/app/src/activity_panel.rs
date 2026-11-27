//! Agent Activity drawn as a list: the lines `activity_log` reads from the entries, newest first.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone};
use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, div, px, rgb, svg,
};
use taco_protocol::{
    AgentRow, Author, HarnessId, Peer, PrState, RowState, SharedTarget, SharedThing, StartSource,
    Step, TerminalId,
};
use taco_services::{AgentSetupNotice, AgentSetupNoticeKind};

use crate::activity_log::{ActivityLog, LogItem, Stretch, StretchStep, log_items, preview};
use crate::assets::{
    CLOSE_ICON, DOCUMENT_ICON, IMAGE_ICON, LINK_ICON, PULL_REQUEST_ICON, TEXT_ICON,
};
use crate::look::{Look, SMALL_TEXT_SIZE};
use crate::shell::shell_orange;
use crate::terminal_list::state_mark;
use crate::window::WindowView;

const SCROLL_ID: &str = "activity-scroll";
const OLDER_ID: &str = "activity-older";
const SHELL_TEXT: &str = "No agent here, just a shell.";
const NEEDS_HEADING: &str = "Needs you.";
const SET_UP_LABEL: &str = "Set up";
const SHOW_OLDER: &str = "Show older";
const SHOW_ALL: &str = "Show all";
const SHOW_LESS: &str = "Show less";
const LOAD_EARLIER: &str = "Load earlier steps";
const NOTE_LABEL: &str = "Agent";
const UNNOTED_LABEL: &str = "No note from the agent";
const WORKING_LABEL: &str = "Steps so far";
const ASKED_LABEL: &str = "You asked";
const ASKED_FOR_NOTE_LABEL: &str = "The app asked for a note";
const STEP_FONT: &str = "Menlo";
const STEP_TEXT_SIZE: f32 = 11.;
const PANEL_PADDING: f32 = 12.;
const CARD_PADDING: f32 = 10.;
const CARD_RADIUS: f32 = 8.;
const AUTHOR_DOT: f32 = 6.;
const APP_DOT_ALPHA: f32 = 0.55;
const LABEL_GAP: f32 = 6.;
const BODY_INDENT: f32 = AUTHOR_DOT + LABEL_GAP;
const LINE_GAP: f32 = 4.;
const ITEM_PADDING: f32 = 9.;
const RULE_GAP: f32 = 8.;
const QUIET_PADDING: f32 = 6.;
const SHARED_ALL_ID: &str = "activity-shared-all";
/// The ink on the shell orange, in both appearances: 6.53:1.
const NOW_INK: u32 = 0x2B211C;
const NOW_TEXT_SIZE: f32 = 13.5;
const NOW_LINES: usize = 4;
const NOW_RADIUS: f32 = 10.;
const NOW_PADDING_X: f32 = 14.;
const NOW_PADDING_Y: f32 = 11.;
const TOP_GAP: f32 = 14.;
const SHARED_HEADING: &str = "SHARED";
const SHARED_HEADING_SIZE: f32 = 10.5;
const SHARED_SHOWN: usize = 5;
const SHARED_ICON_SIZE: f32 = 16.;
const SHARED_RADIUS: f32 = 9.;
const SHOW_FEWER: &str = "Show fewer";
const PILL_RADIUS: f32 = 8.;
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
const CLOSE_SIZE: f32 = 14.;
pub const MS_PER_MINUTE: u64 = 60_000;
const MINUTES_PER_HOUR: u64 = 60;

/// What the panel shows when its terminal has no agent row.
#[derive(Clone, Debug, PartialEq)]
pub enum NoAgentLine {
    Shell,
    Offer { harness: HarnessId, text: String },
    Unavailable { text: String, reason: String },
    Outcome { harness: HarnessId, text: String },
}

/// Everything one drawing of the panel reads.
pub struct ActivityInput<'a> {
    pub terminal: Option<&'a TerminalId>,
    pub agent_row: Option<&'a AgentRow>,
    pub notices: &'a [AgentSetupNotice],
    pub log: &'a ActivityLog,
    /// The stretch numbers whose steps are shown.
    pub folds_open: &'a HashSet<u64>,
    /// The `seq` of each long text shown whole.
    pub texts_open: &'a HashSet<u64>,
    /// Whether Shared lists every thing rather than the newest few.
    pub shared_all_shown: bool,
    pub now_ms: u64,
    pub look: &'a Look,
}

/// Who wrote a line, which picks its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Who {
    Agent,
    User,
    OtherAgent,
    App,
}

impl Who {
    fn of(author: &Author) -> Self {
        match author {
            Author::Agent => Self::Agent,
            Author::User => Self::User,
            Author::OtherAgent(_) => Self::OtherAgent,
            Author::App => Self::App,
        }
    }

    fn ink(self, look: &Look) -> Hsla {
        match self {
            Self::Agent => look.agent_ink,
            Self::User => look.user_ink,
            Self::OtherAgent => look.other_agent_ink,
            Self::App => look.secondary_text,
        }
    }

    fn dot(self, look: &Look) -> Hsla {
        match self {
            Self::Agent => shell_orange(),
            Self::App => look.secondary_text.opacity(APP_DOT_ALPHA),
            Self::User | Self::OtherAgent => self.ink(look),
        }
    }
}

pub fn no_agent_line(terminal: &TerminalId, notices: &[AgentSetupNotice]) -> NoAgentLine {
    let mut about_terminal = notices
        .iter()
        .filter(|notice| notice.terminals.contains(terminal));
    let Some(notice) = about_terminal.next() else {
        return NoAgentLine::Shell;
    };
    let harness = notice.harness.clone();
    let text = notice.text.clone();
    match &notice.kind {
        AgentSetupNoticeKind::Offer => NoAgentLine::Offer { harness, text },
        AgentSetupNoticeKind::Unavailable { reason } => {
            let reason = reason.clone();
            NoAgentLine::Unavailable { text, reason }
        }
        AgentSetupNoticeKind::Done | AgentSetupNoticeKind::Problem => {
            NoAgentLine::Outcome { harness, text }
        }
    }
}

pub fn activity_panel(input: &ActivityInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let mut content = div()
        .id(SCROLL_ID)
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(PANEL_PADDING))
        .p(px(PANEL_PADDING));
    if let Some(row) = input.agent_row {
        if row.state == RowState::Needs
            && let Some(question) = &row.question
        {
            content = content.child(question_card(question, look));
        }
        content = content.child(log_section(input, row, cx));
    } else {
        let line = no_agent_for(input);
        content = content.child(no_agent_element(&line, look, cx));
    }
    div().size_full().child(content)
}

/// Milliseconds since the Unix epoch, as the log's `at_ms` counts them.
pub fn now_ms() -> u64 {
    let since_epoch = SystemTime::now().duration_since(UNIX_EPOCH);
    let Ok(elapsed) = since_epoch else {
        return 0;
    };
    let millis = elapsed.as_millis();
    let Ok(fitted) = u64::try_from(millis) else {
        return u64::MAX;
    };
    fitted
}

fn no_agent_for(input: &ActivityInput) -> NoAgentLine {
    match input.terminal {
        Some(terminal) => no_agent_line(terminal, input.notices),
        None => NoAgentLine::Shell,
    }
}

fn peer_name(peer: &Peer) -> String {
    if let Some(name) = &peer.name {
        return name.clone();
    }
    if let Some(address) = &peer.address {
        return address.clone();
    }
    "another agent".to_string()
}

fn shared_label(by: &Author) -> String {
    match by {
        Author::Agent => "Agent shared".to_string(),
        Author::User => "You shared".to_string(),
        Author::OtherAgent(peer) => format!("{} shared", peer_name(peer)),
        Author::App => "The app found".to_string(),
    }
}

fn session_words(source: StartSource) -> &'static str {
    match source {
        StartSource::New => "New session",
        StartSource::Resumed => "Session resumed",
        StartSource::Compacted => "Continued after compacting",
        StartSource::Cleared => "Session cleared",
    }
}

fn step_text(step: &StretchStep) -> String {
    match step {
        StretchStep::Step(Step::Did { tool, summary, .. }) => match tool.as_str() {
            "Bash" => format!("Ran {summary}"),
            "Read" => format!("Read {summary}"),
            "Grep" | "Glob" => format!("Searched for {summary}"),
            _ => format!("{tool}: {summary}"),
        },
        StretchStep::Step(Step::Changed { tool, path }) => match tool.as_str() {
            "Write" => format!("Wrote {path}"),
            _ => format!("Edited {path}"),
        },
        StretchStep::SentMessage(peer) => format!("Sent a message to {}", peer_name(peer)),
    }
}

fn is_search(step: &StretchStep) -> bool {
    let StretchStep::Step(Step::Did { tool, .. }) = step else {
        return false;
    };
    tool == "Grep" || tool == "Glob"
}

/// One line per step, with a run of searches told as one.
fn step_lines(steps: &[StretchStep]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut searches = Vec::new();
    for step in steps {
        if is_search(step) {
            searches.push(step);
            continue;
        }
        tell_searches(&mut searches, &mut lines);
        lines.push(step_text(step));
    }
    tell_searches(&mut searches, &mut lines);
    lines
}

fn tell_searches(searches: &mut Vec<&StretchStep>, lines: &mut Vec<String>) {
    match searches.as_slice() {
        [] => {}
        [only] => lines.push(step_text(only)),
        several => lines.push(format!("Searched {} times", several.len())),
    }
    searches.clear();
}

fn fold_label(steps: &Stretch, is_open: bool) -> String {
    let count = steps.steps.len();
    if is_open && count == 1 {
        return "Hide the step".to_string();
    }
    if is_open {
        return "Hide the steps".to_string();
    }
    if count == 0 {
        return "Show the earlier steps".to_string();
    }
    if steps.earlier_not_loaded {
        return format!("Show the {count}+ steps");
    }
    if count == 1 {
        return "Show the step".to_string();
    }
    format!("Show the {count} steps")
}

fn local_time(ms: u64) -> Option<DateTime<Local>> {
    let signed = i64::try_from(ms).ok()?;
    Local.timestamp_millis_opt(signed).single()
}

fn time_label(at_ms: u64, now_ms: u64) -> String {
    let Some(at) = local_time(at_ms) else {
        return String::new();
    };
    let Some(now) = local_time(now_ms) else {
        return String::new();
    };
    if at.date_naive() == now.date_naive() {
        return at.format("%H:%M").to_string();
    }
    at.format("%b %-d").to_string()
}

fn finished_text(since_ms: u64, now_ms: u64) -> String {
    let elapsed = now_ms.saturating_sub(since_ms);
    let minutes = elapsed / MS_PER_MINUTE;
    if minutes < 1 {
        return "Finished just now".to_string();
    }
    if minutes == 1 {
        return "Finished 1 minute ago".to_string();
    }
    if minutes < MINUTES_PER_HOUR {
        return format!("Finished {minutes} minutes ago");
    }
    let hours = minutes / MINUTES_PER_HOUR;
    if hours == 1 {
        return "Finished 1 hour ago".to_string();
    }
    format!("Finished {hours} hours ago")
}

fn now_text(row: &AgentRow, now_ms: u64) -> Option<String> {
    match row.state {
        RowState::Working => Some("Working".to_string()),
        RowState::Needs => Some("Waiting for your answer".to_string()),
        RowState::Done => Some(finished_text(row.state_since_ms, now_ms)),
        RowState::Idle => None,
    }
}

fn element_id(kind: &str, number: u64) -> ElementId {
    let id = SharedString::from(format!("activity-{kind}:{number}"));
    ElementId::Name(id)
}

fn question_card(question: &str, look: &Look) -> Div {
    let mark = state_mark(Some(RowState::Needs), look);
    let heading = div().font_weight(FontWeight::SEMIBOLD).child(NEEDS_HEADING);
    let head = div()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(mark)
        .child(heading);
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .p(px(CARD_PADDING))
        .bg(look.selected_card)
        .border_1()
        .border_color(look.hairline)
        .rounded(px(CARD_RADIUS))
        .child(head)
        .child(question.to_string())
}

fn quiet_button(id: ElementId, label: &str, look: &Look) -> Stateful<Div> {
    let veil = look.veil;
    div()
        .id(id)
        .flex_none()
        .px(px(QUIET_PADDING))
        .py(px(2.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .hover(move |style| style.bg(veil))
        .child(label.to_string())
}

// The label lines up with the text above it, the button's padding hanging out to the left.
fn aligned_button(id: ElementId, label: &str, look: &Look) -> Stateful<Div> {
    let button = quiet_button(id, label, look);
    button.ml(px(-QUIET_PADDING))
}

fn no_agent_element(line: &NoAgentLine, look: &Look, cx: &mut Context<WindowView>) -> Div {
    let row = div().flex().items_center().gap(px(10.));
    match line {
        NoAgentLine::Shell => div().text_color(look.secondary_text).child(SHELL_TEXT),
        NoAgentLine::Offer { harness, text } => {
            let shown = text_cell(text);
            let button = set_up_button(harness, look, cx);
            row.child(shown).child(button)
        }
        NoAgentLine::Unavailable { text, reason } => {
            let shown = text_cell(text);
            let why = div()
                .flex_none()
                .text_color(look.secondary_text)
                .child(reason.clone());
            row.child(shown).child(why)
        }
        NoAgentLine::Outcome { harness, text } => {
            let shown = text_cell(text);
            let dismiss = dismiss_button(harness, look, cx);
            row.child(shown).child(dismiss)
        }
    }
}

fn text_cell(text: &str) -> Div {
    div().flex_1().min_w_0().child(text.to_string())
}

fn set_up_button(harness: &HarnessId, look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let chosen = harness.clone();
    div()
        .id("activity-set-up")
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .child(SET_UP_LABEL)
        .on_click(cx.listener(move |view, _, _, _| view.set_up(&chosen)))
}

fn dismiss_button(harness: &HarnessId, look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let dismissed = harness.clone();
    let veil = look.veil;
    let cross = svg()
        .path(CLOSE_ICON)
        .size(px(CLOSE_SIZE))
        .flex_none()
        .text_color(look.secondary_text);
    div()
        .id("activity-dismiss")
        .flex_none()
        .p(px(2.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(veil))
        .child(cross)
        .on_click(cx.listener(move |view, _, _, _| view.dismiss_notice(&dismissed)))
}

fn log_section(input: &ActivityInput, row: &AgentRow, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let mut section = div().flex().flex_col();
    let mut top = Vec::new();
    if let Some(now) = &input.log.board.now {
        top.push(now_card(now, row, input));
    } else if let Some(text) = now_text(row, input.now_ms) {
        let mark = state_mark(Some(row.state), look);
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(mark)
            .child(text);
        top.push(header);
    }
    if let Some(shared) = shared_section(input, cx) {
        top.push(shared);
    }
    if !top.is_empty() {
        let above_the_log = div()
            .flex()
            .flex_col()
            .gap(px(TOP_GAP))
            .pb(px(ITEM_PADDING))
            .border_b_1()
            .border_color(look.hairline)
            .children(top);
        section = section.child(above_the_log);
    }
    let agent_working = matches!(row.state, RowState::Working | RowState::Needs);
    let items = log_items(input.log, agent_working);
    for item in &items {
        let shown = item_element(item, input, cx);
        section = section.child(shown);
    }
    if !input.log.complete && !input.log.entries.is_empty() {
        let label = aligned_button(OLDER_ID.into(), SHOW_OLDER, look);
        let older = label.on_click(cx.listener(|view, _, _, _| view.show_older()));
        let placed = div().flex().pt(px(ITEM_PADDING)).child(older);
        section = section.child(placed);
    }
    section
}

/// The agent's own line on what it is doing, on the shell's orange, with the row's state under it.
fn now_card(now: &str, row: &AgentRow, input: &ActivityInput) -> Div {
    let look = input.look;
    let ink: Hsla = rgb(NOW_INK).into();
    let line = div()
        .text_size(px(NOW_TEXT_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .line_clamp(NOW_LINES)
        .text_ellipsis()
        .child(now.to_string());
    let mut card = div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .px(px(NOW_PADDING_X))
        .py(px(NOW_PADDING_Y))
        .rounded(px(NOW_RADIUS))
        .bg(shell_orange())
        .text_color(ink)
        .child(line);
    if let Some(state) = now_text(row, input.now_ms) {
        let mut inked = look.clone();
        inked.working = ink;
        inked.done = ink;
        inked.needs = ink;
        let mark = state_mark(Some(row.state), &inked);
        let state_line = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(SMALL_TEXT_SIZE))
            .child(mark)
            .child(state);
        card = card.child(state_line);
    }
    card
}

fn shown_count(total: usize, all_shown: bool) -> usize {
    if all_shown {
        return total;
    }
    total.min(SHARED_SHOWN)
}

fn has_more_than_shown(total: usize) -> bool {
    total > SHARED_SHOWN
}

/// A pull request's state as last seen, never as if current: "Open, seen 12:04".
fn pull_request_state(state: PrState, checked_ms: Option<u64>, at_ms: u64, now_ms: u64) -> String {
    let word = match state {
        PrState::Open => "Open",
        PrState::Draft => "Draft",
        PrState::Merged => "Merged",
        PrState::Closed => "Closed",
    };
    let mut seen_ms = at_ms;
    if let Some(checked) = checked_ms {
        seen_ms = seen_ms.max(checked);
    }
    let seen = time_label(seen_ms, now_ms);
    format!("{word}, seen {seen}")
}

fn is_image(path: &str) -> bool {
    let Some((_, extension)) = path.rsplit_once('.') else {
        return false;
    };
    let lowered = extension.to_ascii_lowercase();
    IMAGE_EXTENSIONS.contains(&lowered.as_str())
}

fn target_icon(target: &SharedTarget) -> &'static str {
    match target {
        SharedTarget::Text { .. } => TEXT_ICON,
        SharedTarget::File { path } if is_image(path) => IMAGE_ICON,
        SharedTarget::File { .. } => DOCUMENT_ICON,
        SharedTarget::Link { .. } => LINK_ICON,
        SharedTarget::PullRequest { .. } => PULL_REQUEST_ICON,
    }
}

fn sharer_name(by: &Author) -> String {
    match by {
        Author::Agent => "agent".to_string(),
        Author::User => "you".to_string(),
        Author::OtherAgent(peer) => peer_name(peer),
        Author::App => "the app".to_string(),
    }
}

/// What the agent and the app shared, newest first, in one card.
fn shared_section(input: &ActivityInput, cx: &mut Context<WindowView>) -> Option<Div> {
    let shared = &input.log.board.shared;
    if shared.is_empty() {
        return None;
    }
    let look = input.look;
    let heading_text = format!("{SHARED_HEADING} · {}", shared.len());
    let heading = div()
        .text_size(px(SHARED_HEADING_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(look.agent_ink)
        .child(heading_text);
    let count = shown_count(shared.len(), input.shared_all_shown);
    let mut card = div()
        .flex()
        .flex_col()
        .rounded(px(SHARED_RADIUS))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card);
    let shown = shared.iter().take(count).enumerate();
    for (index, thing) in shown {
        let ruled_above = index > 0;
        let row = shared_row(thing, ruled_above, input);
        card = card.child(row);
    }
    let mut section = div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(6.))
        .child(heading)
        .child(card.w_full());
    if has_more_than_shown(shared.len()) {
        let mut label = format!("Show all {}", shared.len());
        if input.shared_all_shown {
            label = SHOW_FEWER.to_string();
        }
        let button = aligned_button(SHARED_ALL_ID.into(), &label, look);
        let toggle = button.on_click(cx.listener(|view, _, _, cx| view.toggle_shared_all(cx)));
        section = section.child(toggle);
    }
    Some(section)
}

fn shared_row(thing: &SharedThing, ruled_above: bool, input: &ActivityInput) -> Div {
    let look = input.look;
    let icon_path = target_icon(&thing.target);
    let icon = svg()
        .path(icon_path)
        .size(px(SHARED_ICON_SIZE))
        .flex_none()
        .mt(px(1.))
        .text_color(shell_orange());
    let title = div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .font_weight(FontWeight::MEDIUM)
        .child(thing.title.clone());
    let who = Who::of(&thing.by);
    let dot = div()
        .flex_none()
        .size(px(AUTHOR_DOT))
        .rounded_full()
        .bg(who.dot(look));
    let name = div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(sharer_name(&thing.by));
    let sharer = div()
        .flex()
        .items_center()
        .max_w_full()
        .gap(px(4.))
        .child(dot)
        .child(name);
    let time = time_label(thing.at_ms, input.now_ms);
    let mut facts = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(6.))
        .gap_y(px(2.))
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .child(thing.kind.clone())
        .child(sharer)
        .child(time);
    if let SharedTarget::PullRequest {
        state, checked_ms, ..
    } = &thing.target
    {
        let words = pull_request_state(*state, *checked_ms, thing.at_ms, input.now_ms);
        let pill = div()
            .max_w_full()
            .px(px(6.))
            .rounded(px(PILL_RADIUS))
            .bg(look.strong_veil)
            .text_color(look.text)
            .child(words);
        facts = facts.child(pill);
    }
    let words = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(2.))
        .child(title)
        .child(facts);
    let mut row = div()
        .flex()
        .items_start()
        .gap(px(8.))
        .px(px(CARD_PADDING))
        .py(px(7.))
        .child(icon)
        .child(words);
    if ruled_above {
        row = row.border_t_1().border_color(look.hairline);
    }
    row
}

fn item_element(item: &LogItem, input: &ActivityInput, cx: &mut Context<WindowView>) -> AnyElement {
    match item {
        LogItem::Note {
            seq,
            at_ms,
            text,
            steps,
        } => {
            let said = long_text(*seq, text, input, cx);
            let mut body = vec![said];
            if let Some(steps) = steps
                && let Some(fold) = fold(steps, input, cx)
            {
                body.push(fold);
            }
            entry(Who::Agent, NOTE_LABEL, *at_ms, body, input)
        }
        LogItem::Unnoted { at_ms, steps } => {
            let mut body = Vec::new();
            if let Some(fold) = fold(steps, input, cx) {
                body.push(fold);
            }
            entry(Who::App, UNNOTED_LABEL, *at_ms, body, input)
        }
        LogItem::Working { at_ms, steps } => {
            let mut body = Vec::new();
            if let Some(newest) = steps.steps.last() {
                let newest_text = step_text(newest);
                body.push(peek(&newest_text, input.look));
            }
            if let Some(fold) = fold(steps, input, cx) {
                body.push(fold);
            }
            entry(Who::App, WORKING_LABEL, *at_ms, body, input)
        }
        LogItem::Asked { seq, at_ms, text } => {
            let said = long_text(*seq, text, input, cx);
            entry(Who::User, ASKED_LABEL, *at_ms, vec![said], input)
        }
        LogItem::MessageIn {
            seq,
            at_ms,
            from,
            text,
        } => {
            let label = format!("From {}", peer_name(from));
            let said = long_text(*seq, text, input, cx);
            entry(Who::OtherAgent, &label, *at_ms, vec![said], input)
        }
        LogItem::MessageOut {
            seq,
            at_ms,
            to,
            text,
        } => {
            let label = format!("To {}", peer_name(to));
            let said = long_text(*seq, text, input, cx);
            entry(Who::Agent, &label, *at_ms, vec![said], input)
        }
        LogItem::Shared {
            at_ms, by, title, ..
        } => {
            let label = shared_label(by);
            let who = Who::of(by);
            let shown = div().min_w_0().child(title.clone()).into_any_element();
            entry(who, &label, *at_ms, vec![shown], input)
        }
        LogItem::AskedForNote { at_ms, .. } => {
            entry(Who::App, ASKED_FOR_NOTE_LABEL, *at_ms, Vec::new(), input)
        }
        LogItem::SessionStarted { at_ms, source, .. } => {
            session_divider(*source, *at_ms, input).into_any_element()
        }
    }
}

/// A line of the log: its author's dot and label with the time, then what it says.
fn entry(
    who: Who,
    label: &str,
    at_ms: u64,
    body: Vec<AnyElement>,
    input: &ActivityInput,
) -> AnyElement {
    let look = input.look;
    let dot = div()
        .flex_none()
        .size(px(AUTHOR_DOT))
        .rounded_full()
        .bg(who.dot(look));
    let name = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .font_weight(FontWeight::MEDIUM)
        .text_color(who.ink(look))
        .child(label.to_string());
    let time_text = time_label(at_ms, input.now_ms);
    let time = div()
        .flex_none()
        .text_color(look.secondary_text)
        .child(time_text);
    let head = div()
        .flex()
        .items_center()
        .gap(px(LABEL_GAP))
        .text_size(px(SMALL_TEXT_SIZE))
        .child(dot)
        .child(name)
        .child(time);
    let mut line = div()
        .flex()
        .flex_col()
        .gap(px(LINE_GAP))
        .py(px(ITEM_PADDING))
        .border_b_1()
        .border_color(look.hairline)
        .child(head);
    if !body.is_empty() {
        let said = div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(LINE_GAP))
            .min_w_0()
            .pl(px(BODY_INDENT))
            .children(body);
        line = line.child(said);
    }
    line.into_any_element()
}

fn peek(text: &str, look: &Look) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_color(look.secondary_text)
        .child(text.to_string())
        .into_any_element()
}

/// A text shown whole, or its start with a button that shows the rest.
fn long_text(
    seq: u64,
    text: &str,
    input: &ActivityInput,
    cx: &mut Context<WindowView>,
) -> AnyElement {
    let Some(cut) = preview(text) else {
        return div().min_w_0().child(text.to_string()).into_any_element();
    };
    let is_open = input.texts_open.contains(&seq);
    let mut shown = cut;
    let mut label = SHOW_ALL;
    if is_open {
        shown = text.to_string();
        label = SHOW_LESS;
    }
    let id = element_id("text", seq);
    let button = aligned_button(id, label, input.look);
    let toggle = button.on_click(cx.listener(move |view, _, _, cx| view.toggle_text(seq, cx)));
    let words = div().min_w_0().child(shown);
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(2.))
        .min_w_0()
        .child(words)
        .child(toggle)
        .into_any_element()
}

/// The button that shows a stretch's steps, and the steps when shown; none when there is nothing to show.
fn fold(
    steps: &Stretch,
    input: &ActivityInput,
    cx: &mut Context<WindowView>,
) -> Option<AnyElement> {
    let nothing_to_show = steps.steps.is_empty() && !steps.earlier_not_loaded;
    if nothing_to_show {
        return None;
    }
    let look = input.look;
    let number = steps.number;
    let is_open = input.folds_open.contains(&number);
    let label = fold_label(steps, is_open);
    let id = element_id("fold", number);
    let button = aligned_button(id, &label, look);
    let toggle = button.on_click(cx.listener(move |view, _, _, cx| view.toggle_fold(number, cx)));
    let mut column = div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(2.))
        .w_full()
        .min_w_0()
        .child(toggle);
    if !is_open {
        return Some(column.into_any_element());
    }
    let mut list = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .w_full()
        .min_w_0()
        .pl(px(6.))
        .font_family(STEP_FONT)
        .text_size(px(STEP_TEXT_SIZE))
        .text_color(look.secondary_text);
    for line in step_lines(&steps.steps) {
        let step = div().min_w_0().child(line);
        list = list.child(step);
    }
    column = column.child(list);
    if steps.earlier_not_loaded {
        let id = element_id("earlier", number);
        let button = aligned_button(id, LOAD_EARLIER, look);
        let earlier = button.on_click(cx.listener(|view, _, _, _| view.show_older()));
        column = column.child(earlier);
    }
    Some(column.into_any_element())
}

// The hairlines shrink to nothing before the words wrap.
fn session_divider(source: StartSource, at_ms: u64, input: &ActivityInput) -> Div {
    let look = input.look;
    let time = time_label(at_ms, input.now_ms);
    let text = format!("{} · {time}", session_words(source));
    let words = div().text_center().child(text);
    let rule = || div().flex_1().min_w_0().h(px(1.)).bg(look.hairline);
    div()
        .flex()
        .items_center()
        .gap(px(RULE_GAP))
        .py(px(ITEM_PADDING))
        .border_b_1()
        .border_color(look.hairline)
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .child(rule())
        .child(words)
        .child(rule())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(kind: AgentSetupNoticeKind, terminal: &str) -> AgentSetupNotice {
        AgentSetupNotice {
            harness: HarnessId("claude".to_string()),
            kind,
            text: "Set up Claude?".to_string(),
            terminals: vec![TerminalId(terminal.to_string())],
        }
    }

    fn did(tool: &str, summary: &str) -> StretchStep {
        let tool = tool.to_string();
        let summary = summary.to_string();
        StretchStep::Step(Step::Did {
            tool,
            summary,
            failed: None,
        })
    }

    fn changed(tool: &str, path: &str) -> StretchStep {
        let tool = tool.to_string();
        let path = path.to_string();
        StretchStep::Step(Step::Changed { tool, path })
    }

    #[test]
    fn steps_tell_a_run_of_searches_as_one_and_folds_count_what_is_held() {
        let code_83 = Peer {
            name: Some("code-83".to_string()),
            address: None,
        };
        let steps = vec![
            did("Grep", "SIGHUP"),
            did("Glob", "*.rs"),
            did("Read", "pty.rs"),
            changed("Write", "PLAN.md"),
            changed("Edit", "pty.rs"),
            StretchStep::SentMessage(code_83),
            did("Glob", "tests/*.rs"),
        ];
        let expected = [
            "Searched 2 times",
            "Read pty.rs",
            "Wrote PLAN.md",
            "Edited pty.rs",
            "Sent a message to code-83",
            "Searched for tests/*.rs",
        ];
        assert_eq!(step_lines(&steps), expected);

        let stretch = |count: usize, earlier_not_loaded: bool| Stretch {
            number: 1,
            steps: steps[..count].to_vec(),
            earlier_not_loaded,
        };
        assert_eq!(fold_label(&stretch(1, false), false), "Show the step");
        assert_eq!(fold_label(&stretch(1, false), true), "Hide the step");
        assert_eq!(fold_label(&stretch(7, true), false), "Show the 7+ steps");
        assert_eq!(
            fold_label(&stretch(0, true), false),
            "Show the earlier steps"
        );
    }

    #[test]
    fn a_pull_request_reads_as_last_seen_and_shared_shows_the_newest_five_until_asked() {
        let now = now_ms();
        let shared_at = now - 40 * MS_PER_MINUTE;
        let checked_at = now - 5 * MS_PER_MINUTE;
        let checked = pull_request_state(PrState::Open, Some(checked_at), shared_at, now);
        let expected = format!("Open, seen {}", time_label(checked_at, now));
        assert_eq!(checked, expected);
        let never_checked = pull_request_state(PrState::Merged, None, shared_at, now);
        let expected = format!("Merged, seen {}", time_label(shared_at, now));
        assert_eq!(never_checked, expected);
        let checked_before_shared = shared_at - 5 * MS_PER_MINUTE;
        let not_earlier =
            pull_request_state(PrState::Open, Some(checked_before_shared), shared_at, now);
        assert_eq!(
            not_earlier,
            format!("Open, seen {}", time_label(shared_at, now))
        );

        assert_eq!(shown_count(3, false), 3);
        assert_eq!(shown_count(23, false), 5);
        assert_eq!(shown_count(23, true), 23);
        assert!(!has_more_than_shown(5));
        assert!(has_more_than_shown(6));

        assert!(is_image("/Users/me/view-check/logo-after.PNG"));
        assert!(!is_image("/Users/me/PLAN-sigkill.md"));
        assert!(!is_image("/Users/me/pictures.png/notes"));
    }

    #[test]
    fn no_agent_line_follows_the_notice_for_this_terminal() {
        let here = TerminalId("here".to_string());
        let reason = "Turned off".to_string();
        let unavailable = AgentSetupNoticeKind::Unavailable {
            reason: reason.clone(),
        };
        let harness = HarnessId("claude".to_string());
        let text = "Set up Claude?".to_string();

        assert_eq!(no_agent_line(&here, &[]), NoAgentLine::Shell);
        let elsewhere = notice(AgentSetupNoticeKind::Offer, "there");
        assert_eq!(no_agent_line(&here, &[elsewhere]), NoAgentLine::Shell);

        let offer = notice(AgentSetupNoticeKind::Offer, "here");
        let expected = NoAgentLine::Offer {
            harness: harness.clone(),
            text: text.clone(),
        };
        assert_eq!(no_agent_line(&here, &[offer]), expected);

        let unavailable = notice(unavailable, "here");
        let expected = NoAgentLine::Unavailable {
            text: text.clone(),
            reason,
        };
        assert_eq!(no_agent_line(&here, &[unavailable]), expected);

        for kind in [AgentSetupNoticeKind::Done, AgentSetupNoticeKind::Problem] {
            let outcome = notice(kind, "here");
            let expected = NoAgentLine::Outcome {
                harness: harness.clone(),
                text: text.clone(),
            };
            assert_eq!(no_agent_line(&here, &[outcome]), expected);
        }
    }
}
