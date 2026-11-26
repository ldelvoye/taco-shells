use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, TimeZone};
use gpui::{
    Context, Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, px, svg,
};
use taco_protocol::{
    ActivityEntry, ActivityEvent, ActivityKind, AgentRow, HarnessId, RowState, TerminalId,
};
use taco_services::{AgentSetupNotice, AgentSetupNoticeKind};

use crate::assets::CLOSE_ICON;
use crate::look::{Look, SMALL_TEXT_SIZE};
use crate::terminal_list::state_mark;
use crate::window::WindowView;

const SCROLL_ID: &str = "activity-scroll";
const SHELL_TEXT: &str = "No agent here, just a shell.";
const NEEDS_HEADING: &str = "Needs you.";
const SET_UP_LABEL: &str = "Set up";
const SHOW_FILES: &str = "Show the files";
const HIDE_FILES: &str = "Hide the files";
const SHOW_OLDER: &str = "Show older";
const PANEL_PADDING: f32 = 12.;
const CARD_PADDING: f32 = 10.;
const CARD_RADIUS: f32 = 8.;
const TIME_WIDTH: f32 = 48.;
const DOT_SIZE: f32 = 5.;
const DOT_TOP: f32 = 8.;
const LINE_GAP: f32 = 6.;
const CLOSE_SIZE: f32 = 14.;
const MS_PER_MINUTE: u64 = 60_000;
const MINUTES_PER_HOUR: u64 = 60;

/// What the panel shows when its terminal has no agent row.
#[derive(Clone, Debug, PartialEq)]
pub enum NoAgentLine {
    Shell,
    Offer { harness: HarnessId, text: String },
    Unavailable { text: String, reason: String },
    Outcome { harness: HarnessId, text: String },
}

/// One drawn line of the log: neighbouring entries of one kind folded together.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    /// The `seq` of the newest entry folded into it, which names the line.
    pub seq: u64,
    pub at_ms: u64,
    pub text: String,
    pub detail: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fold {
    Read,
    Changed,
}

/// The log the window keeps for the terminal it watches.
#[derive(Clone, Debug, Default)]
pub struct ActivityLog {
    pub terminal: Option<TerminalId>,
    /// Oldest first, as the workspace sends them.
    pub entries: Vec<ActivityEntry>,
    pub complete: bool,
}

/// Everything one drawing of the panel reads.
pub struct ActivityInput<'a> {
    pub terminal: Option<&'a TerminalId>,
    pub agent_row: Option<&'a AgentRow>,
    pub notices: &'a [AgentSetupNotice],
    pub log: &'a ActivityLog,
    /// The `seq` of each line whose files are shown.
    pub expanded: &'a HashSet<u64>,
    pub now_ms: u64,
    pub look: &'a Look,
}

impl ActivityLog {
    /// Applies one event; one for another terminal is ignored.
    pub fn apply(&mut self, event: ActivityEvent) {
        match event {
            ActivityEvent::Loaded {
                terminal,
                entries,
                complete,
            } => {
                if !self.watches(&terminal) {
                    return;
                }
                self.entries = entries;
                self.complete = complete;
            }
            ActivityEvent::Older {
                terminal,
                entries,
                complete,
            } => {
                if !self.watches(&terminal) {
                    return;
                }
                let mut merged = entries;
                merged.append(&mut self.entries);
                self.entries = merged;
                self.complete = complete;
            }
            ActivityEvent::Appended { terminal, entry } => {
                if !self.watches(&terminal) {
                    return;
                }
                self.entries.push(entry);
            }
        }
    }

    /// The `seq` of the oldest entry held, for asking for older ones.
    pub fn oldest(&self) -> Option<u64> {
        let first = self.entries.first()?;
        Some(first.seq)
    }

    fn watches(&self, terminal: &TerminalId) -> bool {
        self.terminal.as_ref() == Some(terminal)
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

pub fn log_lines(entries: &[ActivityEntry]) -> Vec<LogLine> {
    let mut lines = Vec::new();
    let mut first = 0;
    while first < entries.len() {
        let last = end_of_run(entries, first);
        let run = &entries[first..=last];
        let line = line_of(run);
        lines.push(line);
        first = last + 1;
    }
    lines.reverse();
    lines
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

fn fold_of(kind: &ActivityKind) -> Option<Fold> {
    match kind {
        ActivityKind::Did { tool, .. } if tool == "Read" => Some(Fold::Read),
        ActivityKind::Changed { .. } => Some(Fold::Changed),
        _ => None,
    }
}

fn item_of(kind: &ActivityKind) -> &str {
    match kind {
        ActivityKind::Asked { text } => text,
        ActivityKind::Did { summary, .. } => summary,
        ActivityKind::Changed { path, .. } => path,
    }
}

fn end_of_run(entries: &[ActivityEntry], first: usize) -> usize {
    let fold = fold_of(&entries[first].kind);
    if fold.is_none() {
        return first;
    }
    let mut last = first;
    while last + 1 < entries.len() && fold_of(&entries[last + 1].kind) == fold {
        last += 1;
    }
    last
}

fn line_of(run: &[ActivityEntry]) -> LogLine {
    let newest = &run[run.len() - 1];
    let fold = fold_of(&newest.kind);
    let mut text = single_text(&newest.kind);
    let mut detail = Vec::new();
    if let Some(fold) = fold
        && run.len() > 1
    {
        let verb = match fold {
            Fold::Read => "Read",
            Fold::Changed => "Changed",
        };
        text = format!("{verb} {} files", run.len());
        let items = run.iter().map(|entry| item_of(&entry.kind).to_string());
        detail = items.collect();
    }
    LogLine {
        seq: newest.seq,
        at_ms: newest.at_ms,
        text,
        detail,
    }
}

fn single_text(kind: &ActivityKind) -> String {
    match kind {
        ActivityKind::Asked { text } => format!("You asked: {text}"),
        ActivityKind::Did { tool, summary } => match tool.as_str() {
            "Bash" => format!("Ran {summary}"),
            "Read" => format!("Read {summary}"),
            "Grep" | "Glob" => format!("Searched for {summary}"),
            _ => format!("{tool}: {summary}"),
        },
        ActivityKind::Changed { path, .. } => format!("Changed {path}"),
    }
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
        .px(px(6.))
        .py(px(2.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .hover(move |style| style.bg(veil))
        .child(label.to_string())
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
    let mut section = div().flex().flex_col().gap(px(LINE_GAP));
    if let Some(text) = now_text(row, input.now_ms) {
        let mark = state_mark(Some(row.state), look);
        let now = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(mark)
            .child(text);
        section = section.child(now);
    }
    let lines = log_lines(&input.log.entries);
    for line in &lines {
        let shown = log_line_element(line, input, cx);
        section = section.child(shown);
    }
    if !input.log.complete && !input.log.entries.is_empty() {
        let label = quiet_button(element_id("older", 0), SHOW_OLDER, look);
        let older = label.on_click(cx.listener(|view, _, _, _| view.show_older()));
        section = section.child(div().flex().child(older));
    }
    section
}

fn log_line_element(line: &LogLine, input: &ActivityInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let time_text = time_label(line.at_ms, input.now_ms);
    let time = div()
        .flex_none()
        .w(px(TIME_WIDTH))
        .mt(px(1.))
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .child(time_text);
    let dot = div()
        .flex_none()
        .mt(px(DOT_TOP))
        .size(px(DOT_SIZE))
        .rounded_full()
        .bg(look.secondary_text);
    let mut body = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(2.))
        .child(line.text.clone());
    if !line.detail.is_empty() {
        let is_open = input.expanded.contains(&line.seq);
        body = with_files(body, line, is_open, look, cx);
    }
    div()
        .flex()
        .items_start()
        .gap(px(8.))
        .child(time)
        .child(dot)
        .child(body)
}

fn with_files(
    body: Div,
    line: &LogLine,
    is_open: bool,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let mut body = body;
    if is_open {
        for path in &line.detail {
            let file = div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(SMALL_TEXT_SIZE))
                .text_color(look.secondary_text)
                .child(path.clone());
            body = body.child(file);
        }
    }
    let mut label = SHOW_FILES;
    if is_open {
        label = HIDE_FILES;
    }
    let seq = line.seq;
    let toggle = quiet_button(element_id("files", seq), label, look)
        .on_click(cx.listener(move |view, _, _, cx| view.toggle_files(seq, cx)));
    let placed = div().flex().child(toggle);
    body.child(placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn did(seq: u64, tool: &str, summary: &str) -> ActivityEntry {
        let kind = ActivityKind::Did {
            tool: tool.to_string(),
            summary: summary.to_string(),
        };
        ActivityEntry {
            seq,
            at_ms: seq * 1000,
            kind,
        }
    }

    fn changed(seq: u64, path: &str) -> ActivityEntry {
        let kind = ActivityKind::Changed {
            tool: "Edit".to_string(),
            path: path.to_string(),
        };
        ActivityEntry {
            seq,
            at_ms: seq * 1000,
            kind,
        }
    }

    fn asked(seq: u64, text: &str) -> ActivityEntry {
        let kind = ActivityKind::Asked {
            text: text.to_string(),
        };
        ActivityEntry {
            seq,
            at_ms: seq * 1000,
            kind,
        }
    }

    fn line(seq: u64, text: &str, detail: &[&str]) -> LogLine {
        let detail = detail.iter().map(|item| item.to_string()).collect();
        LogLine {
            seq,
            at_ms: seq * 1000,
            text: text.to_string(),
            detail,
        }
    }

    fn notice(kind: AgentSetupNoticeKind, terminal: &str) -> AgentSetupNotice {
        AgentSetupNotice {
            harness: HarnessId("claude".to_string()),
            kind,
            text: "Set up Claude?".to_string(),
            terminals: vec![TerminalId(terminal.to_string())],
        }
    }

    #[test]
    fn log_lines_fold_runs_and_word_each_kind() {
        let entries = vec![
            asked(1, "fix it"),
            did(2, "Read", "a.rs"),
            did(3, "Read", "b.rs"),
            did(4, "Read", "c.rs"),
            did(5, "Bash", "ls"),
            did(6, "Read", "d.rs"),
            changed(7, "x.rs"),
            changed(8, "y.rs"),
            did(9, "Grep", "foo"),
            did(10, "Glob", "*.rs"),
            changed(11, "z.rs"),
            did(12, "WebFetch", "example.com"),
        ];
        let lines = log_lines(&entries);
        let expected = vec![
            line(12, "WebFetch: example.com", &[]),
            line(11, "Changed z.rs", &[]),
            line(10, "Searched for *.rs", &[]),
            line(9, "Searched for foo", &[]),
            line(8, "Changed 2 files", &["x.rs", "y.rs"]),
            line(6, "Read d.rs", &[]),
            line(5, "Ran ls", &[]),
            line(4, "Read 3 files", &["a.rs", "b.rs", "c.rs"]),
            line(1, "You asked: fix it", &[]),
        ];
        assert_eq!(lines, expected);
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
