use std::collections::BTreeMap;

use gpui::{
    AnyElement, Div, FontWeight, InteractiveElement, IntoElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px, svg, transparent_black,
};
use taco_protocol::{AgentRow, Arrangement, DrawnState, Row, RowState, TabGroup, TerminalId};
use taco_rules::summary_state;

use crate::assets::{DONE_ICON, PLUS_ICON, TRASH_ICON, WORKING_ICON};
use crate::look::{Look, MARK_SIZE, ROW_HEIGHT, ROW_RADIUS, TRAY_RADIUS};
use crate::trial::PlusPlace;

const UNTITLED_ROW: &str = "Terminal";
const UNNAMED_TAB_GROUP: &str = "Group";
const NEW_TERMINAL: &str = "New terminal";
const ROW_GROUP: &str = "row";
const LIST_ID: &str = "terminal-list";
const ROW_ICON_SIZE: f32 = 14.;
const TRAY_HEADER_HEIGHT: f32 = 26.;

/// The Terminals panel's list: every row, with a tab group's rows in a neutral tray under its name.
pub fn terminal_list(drawn: &DrawnState, look: &Look, plus: PlusPlace) -> Stateful<Div> {
    let mut list = div()
        .id(LIST_ID)
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(6.))
        .py(px(5.));
    if plus == PlusPlace::Head {
        list = list.child(new_terminal_row(look));
    }
    if let Some(arrangement) = &drawn.arrangement {
        let entries = entries_of(arrangement, &drawn.agent_rows, look);
        list = list.children(entries);
    }
    if plus == PlusPlace::Foot {
        list = list.child(new_terminal_row(look));
    }
    list
}

// Walks the rows once, folding each run of rows in one tab group into a tray, so every row is drawn exactly once.
fn entries_of(
    arrangement: &Arrangement,
    agent_rows: &BTreeMap<TerminalId, AgentRow>,
    look: &Look,
) -> Vec<AnyElement> {
    let mut entries = Vec::new();
    let mut index = 0;
    while index < arrangement.rows.len() {
        let row = &arrangement.rows[index];
        let Some(tab_group) = tab_group_of(arrangement, row) else {
            let shown = row_element(row, index == arrangement.selected, agent_rows, look);
            entries.push(shown.into_any_element());
            index += 1;
            continue;
        };
        let last = end_of_run(&arrangement.rows, index);
        let shown = tray_element(arrangement, tab_group, index, last, agent_rows, look);
        entries.push(shown.into_any_element());
        index = last + 1;
    }
    entries
}

fn tab_group_of<'a>(arrangement: &'a Arrangement, row: &Row) -> Option<&'a TabGroup> {
    let id = row.tab_group.as_ref()?;
    let mut tab_groups = arrangement.tab_groups.iter();
    tab_groups.find(|group| &group.id == id)
}

/// The index of the last row in the run that starts at `first` and shares its tab group.
fn end_of_run(rows: &[Row], first: usize) -> usize {
    let tab_group = &rows[first].tab_group;
    let mut last = first;
    while last + 1 < rows.len() && &rows[last + 1].tab_group == tab_group {
        last += 1;
    }
    last
}

fn tray_element(
    arrangement: &Arrangement,
    tab_group: &TabGroup,
    first: usize,
    last: usize,
    agent_rows: &BTreeMap<TerminalId, AgentRow>,
    look: &Look,
) -> Div {
    let members = &arrangement.rows[first..=last];
    let holds_selected = (first..=last).contains(&arrangement.selected);
    let header = tray_header(tab_group, members, holds_selected, agent_rows, look);
    let mut tray = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .pt(px(2.))
        .px(px(3.))
        .pb(px(4.))
        .rounded(px(TRAY_RADIUS))
        .bg(look.veil)
        .child(header);
    if tab_group.collapsed {
        return tray;
    }
    for (offset, row) in members.iter().enumerate() {
        let index = first + offset;
        let selected = index == arrangement.selected;
        tray = tray.child(row_element(row, selected, agent_rows, look));
    }
    tray
}

fn tray_header(
    tab_group: &TabGroup,
    members: &[Row],
    holds_selected: bool,
    agent_rows: &BTreeMap<TerminalId, AgentRow>,
    look: &Look,
) -> Div {
    let mut name = tab_group.name.trim();
    if name.is_empty() {
        name = UNNAMED_TAB_GROUP;
    }
    let count = div()
        .font_weight(FontWeight::NORMAL)
        .text_color(look.secondary_text)
        .child(members.len().to_string());
    let mut header = div()
        .flex()
        .items_center()
        .gap(px(7.))
        .h(px(TRAY_HEADER_HEIGHT))
        .px(px(7.))
        .rounded(px(ROW_RADIUS))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(name.to_string())
        .child(count);
    if !tab_group.collapsed {
        return header;
    }
    let terminals: Vec<&TerminalId> = members.iter().map(|row| &row.terminal).collect();
    let summary = summary_state(agent_rows, &terminals);
    header = header.child(state_mark(summary, look));
    if holds_selected {
        header = raised(header, look);
    }
    header
}

fn row_element(
    row: &Row,
    selected: bool,
    agent_rows: &BTreeMap<TerminalId, AgentRow>,
    look: &Look,
) -> Div {
    let mut label = row.title.as_str();
    if label.is_empty() {
        label = UNTITLED_ROW;
    }
    let mut state = None;
    if let Some(agent_row) = agent_rows.get(&row.terminal) {
        state = Some(agent_row.state);
    }
    let name = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(label.to_string());
    let trash = svg()
        .path(TRASH_ICON)
        .size(px(ROW_ICON_SIZE))
        .flex_none()
        .text_color(transparent_black())
        .group_hover(ROW_GROUP, |style| style.text_color(look.secondary_text));
    let shown = div()
        .group(ROW_GROUP)
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(ROW_HEIGHT))
        .px(px(8.))
        .rounded(px(ROW_RADIUS))
        .child(state_mark(state, look))
        .child(name)
        .child(trash);
    if selected {
        return raised(shown, look);
    }
    shown.hover(|style| style.bg(look.veil))
}

/// The one raised thing in the list: a card with a hairline, a soft shadow and a heavier name.
fn raised(shown: Div, look: &Look) -> Div {
    shown
        .bg(look.selected_card)
        .border_1()
        .border_color(look.card_edge())
        .shadow(look.card_shadow())
        .font_weight(FontWeight::SEMIBOLD)
}

/// The mark for an agent's state, or an empty space of the same size.
pub fn state_mark(state: Option<RowState>, look: &Look) -> AnyElement {
    let place = div().flex_none().size(px(MARK_SIZE));
    match state {
        Some(RowState::Working) => {
            let ring = svg().path(WORKING_ICON).size(px(MARK_SIZE));
            ring.flex_none().text_color(look.working).into_any_element()
        }
        Some(RowState::Needs) => place.rounded_full().bg(look.needs).into_any_element(),
        Some(RowState::Done) => {
            let check = svg().path(DONE_ICON).size(px(MARK_SIZE));
            check.flex_none().text_color(look.done).into_any_element()
        }
        Some(RowState::Idle) | None => place.into_any_element(),
    }
}

fn new_terminal_row(look: &Look) -> Div {
    let plus = svg()
        .path(PLUS_ICON)
        .size(px(ROW_ICON_SIZE))
        .flex_none()
        .text_color(look.secondary_text);
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .h(px(ROW_HEIGHT))
        .px(px(6.))
        .rounded(px(ROW_RADIUS))
        .text_color(look.secondary_text)
        .hover(|style| style.bg(look.veil))
        .child(plus)
        .child(NEW_TERMINAL)
}
