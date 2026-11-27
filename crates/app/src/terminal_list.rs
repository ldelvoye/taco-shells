use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, AppContext, Context, Div, DragMoveEvent, Easing,
    ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Pixels, Point, Render, SharedString, Stateful, StatefulInteractiveElement,
    Styled, TimedTransformation, Transformation, Window, div, px, size, svg, transparent_black,
};
use taco_protocol::{
    AgentRow, Arrangement, Dragged, DrawnState, Row, RowState, TabGroup, TabGroupId, TerminalId,
};
use taco_rules::{DropMark, DropPoint, MarkEdge, MarkTarget, summary_state};
use taco_settings::ShellRowName;

use crate::assets::{DONE_ICON, PLUS_ICON, RIPPLE_ICON, TRASH_ICON, WORKING_ICON};
use crate::done_checks::DRAW_TIME;
use crate::drive;
use crate::look::{FONT_FAMILY, Look, MARK_SIZE, ROW_HEIGHT, ROW_RADIUS, TEXT_SIZE, TRAY_RADIUS};
use crate::row_name::row_name;
use crate::shell::{shell_orange, shell_outline};
use crate::slot::{Slot, fraction_at};
use crate::window::WindowView;

const UNNAMED_TAB_GROUP: &str = "Group";
const NEW_TERMINAL: &str = "New terminal";
const ROW_GROUP: &str = "row";
const HEADER_GROUP: &str = "tray-header";
const LIST_ID: &str = "terminal-list";
const LIST_END_ID: &str = "terminal-list-end";
const ROW_ICON_SIZE: f32 = 14.;
const TRAY_HEADER_HEIGHT: f32 = 26.;
const SLOT_OUTLINE: f32 = 2.;
const SLOT_WASH: f32 = 0.12;
const DRAG_PREVIEW_WIDTH: f32 = 200.;
// The marks move as tacoshells.dev's do (public/site.css): ts-spin and ts-ripple.
const SPIN_PERIOD: Duration = Duration::from_millis(1100);
const RIPPLE_PERIOD: Duration = Duration::from_millis(1600);
const RIPPLE_GROWTH: f32 = 2.8;
const RIPPLE_START_ALPHA: f32 = 0.5;
// Smooth at this size, and half the presents of a 60 Hz screen while an agent works for hours.
const MARK_FPS: f32 = 30.;

/// What a drag out of the list carries: the thing dragged, and the name its picture shows.
#[derive(Clone, Debug)]
pub struct ListDrag {
    pub dragged: Dragged,
    label: String,
    look: Look,
}

/// The picture that follows the pointer during a drag.
struct DragPreview {
    label: String,
    look: Look,
}

/// Everything one drawing of the list reads.
pub struct ListInput<'a> {
    pub drawn: &'a DrawnState,
    pub mark: Option<&'a DropMark>,
    pub look: &'a Look,
    /// The rows whose done check is still drawing itself.
    pub drawing_checks: &'a HashSet<TerminalId>,
    /// Where a plain shell's folder is named "~".
    pub home: Option<&'a Path>,
    pub shell_row_name: ShellRowName,
    /// The slot opening where a dragged row or group will land.
    pub slot: Option<Slot>,
}

impl Render for DragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let card = div()
            .w(px(DRAG_PREVIEW_WIDTH))
            .h(px(ROW_HEIGHT))
            .flex()
            .items_center()
            .px(px(8.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .font_family(FONT_FAMILY)
            .text_size(px(TEXT_SIZE))
            .text_color(look.text)
            .child(self.label.clone());
        raised(card, look).opacity(0.9)
    }
}

/// The Terminals panel's list: every row, with a tab group's rows in a neutral tray under its name.
pub fn terminal_list(input: &ListInput, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let look = input.look;
    let mut list = div()
        .id(LIST_ID)
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(px(2.))
        .px(px(6.))
        .py(px(5.))
        .on_drop(cx.listener(|view, drag: &ListDrag, _, cx| view.drop_on_list(drag, cx)))
        .on_drag_move(cx.listener(|view, event: &DragMoveEvent<ListDrag>, _, cx| {
            if event.bounds.contains(&event.event.position) {
                return;
            }
            view.leave_list(cx);
        }))
        .child(new_terminal_row(look, cx));
    if let Some(arrangement) = &input.drawn.arrangement {
        let entries = entries_of(arrangement, input, cx);
        list = list.children(entries);
        if let Some(slot) = input.slot
            && slot.line.at == arrangement.rows.len()
        {
            list = list.child(slot_element(slot));
        }
    }
    let mut end_shift = 0.;
    if let Some(slot) = input.slot {
        end_shift = slot.shift_of_end();
    }
    let end = div()
        .id(LIST_END_ID)
        .flex_1()
        .min_h(px(ROW_HEIGHT))
        .on_drag_move(
            cx.listener(move |view, event: &DragMoveEvent<ListDrag>, _, cx| {
                let pointer = event.event.position;
                if fraction_at(event.bounds, pointer, end_shift).is_none() {
                    return;
                }
                let drag = event.drag(cx);
                let dragged = drag.dragged.clone();
                view.drag_over(&dragged, DropPoint::End, cx);
            }),
        );
    list.child(end)
}

// Walks the rows once, folding each run of rows in one tab group into a tray, so every row is drawn exactly once.
fn entries_of(
    arrangement: &Arrangement,
    input: &ListInput,
    cx: &mut Context<WindowView>,
) -> Vec<AnyElement> {
    let mut entries = Vec::new();
    let mut index = 0;
    while index < arrangement.rows.len() {
        let row = &arrangement.rows[index];
        // A slot at a group's first row sits before the whole tray, unless it goes under the header.
        if let Some(slot) = input.slot
            && slot.line.at == index
            && !slot.line.header_before
        {
            entries.push(slot_element(slot));
        }
        let Some(tab_group) = tab_group_of(arrangement, row) else {
            let shown = row_element(arrangement, index, input, cx);
            entries.push(shown.into_any_element());
            index += 1;
            continue;
        };
        let last = end_of_run(&arrangement.rows, index);
        let shown = tray_element(arrangement, tab_group, index, last, input, cx);
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
    input: &ListInput,
    cx: &mut Context<WindowView>,
) -> Div {
    let look = input.look;
    let members = &arrangement.rows[first..=last];
    let holds_selected = (first..=last).contains(&arrangement.selected);
    let header = tray_header(tab_group, first, members, holds_selected, input, cx);
    let mut tray = div()
        .relative()
        .flex()
        .flex_none()
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
    for offset in 0..members.len() {
        let index = first + offset;
        if let Some(slot) = input.slot
            && slot.line.at == index
            && (index != first || slot.line.header_before)
        {
            tray = tray.child(slot_element(slot));
        }
        tray = tray.child(row_element(arrangement, index, input, cx));
    }
    tray
}

fn tray_header(
    tab_group: &TabGroup,
    first: usize,
    members: &[Row],
    holds_selected: bool,
    input: &ListInput,
    cx: &mut Context<WindowView>,
) -> Stateful<Div> {
    let look = input.look;
    let mut name = tab_group.name.trim();
    if name.is_empty() {
        name = UNNAMED_TAB_GROUP;
    }
    let count = div()
        .font_weight(FontWeight::NORMAL)
        .text_color(look.secondary_text)
        .child(members.len().to_string());
    let mut header = div()
        .group(HEADER_GROUP)
        .relative()
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
    if tab_group.collapsed {
        let terminals: Vec<&TerminalId> = members.iter().map(|row| &row.terminal).collect();
        let summary = summary_state(&input.drawn.agent_rows, &terminals);
        header = header.child(state_mark(summary, look));
        if holds_selected {
            header = raised(header, look);
        }
    }
    header = header
        .child(div().flex_1())
        .child(open_in_group_button(&tab_group.id, look, cx));
    if let Some(mark) = input.mark
        && mark.target == MarkTarget::Header(tab_group.id.clone())
        && mark.edge == MarkEdge::Whole
    {
        header = header.bg(look.strong_veil);
    }
    let mut shift = 0.;
    if let Some(slot) = input.slot {
        shift = slot.shift_of_header(first);
    }

    let toggled = tab_group.id.clone();
    let hovered = tab_group.id.clone();
    let popped = tab_group.id.clone();
    let drag = ListDrag {
        dragged: Dragged::TabGroup(tab_group.id.clone()),
        label: name.to_string(),
        look: look.clone(),
    };
    header
        .id(element_id("tray", &tab_group.id.0))
        .on_click(cx.listener(move |view, _, _, _| view.toggle_group(&toggled)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                view.open_group_popup(popped.clone(), event.position, cx);
            }),
        )
        .on_drag(drag, drag_preview)
        .on_drag_move(
            cx.listener(move |view, event: &DragMoveEvent<ListDrag>, _, cx| {
                let pointer = event.event.position;
                let Some(fraction) = fraction_at(event.bounds, pointer, shift) else {
                    return;
                };
                let drag = event.drag(cx);
                let dragged = drag.dragged.clone();
                let point = DropPoint::Header {
                    tab_group: hovered.clone(),
                    fraction,
                };
                view.drag_over(&dragged, point, cx);
            }),
        )
}

fn open_in_group_button(
    tab_group: &TabGroupId,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Stateful<Div> {
    let opened_in = tab_group.clone();
    let plus = svg()
        .path(PLUS_ICON)
        .size(px(ROW_ICON_SIZE))
        .text_color(transparent_black())
        .group_hover(HEADER_GROUP, |style| style.text_color(look.secondary_text));
    div()
        .id(element_id("tray-plus", &tab_group.0))
        .flex_none()
        .child(plus)
        .on_click(cx.listener(move |view, _, _, cx| {
            // The header behind it collapses on a click.
            cx.stop_propagation();
            view.open_terminal_in(Some(opened_in.clone()), cx);
        }))
}

fn row_element(
    arrangement: &Arrangement,
    index: usize,
    input: &ListInput,
    cx: &mut Context<WindowView>,
) -> Stateful<Div> {
    let look = input.look;
    let row = &arrangement.rows[index];
    let selected = index == arrangement.selected;
    let has_agent = input.drawn.agent_rows.contains_key(&row.terminal);
    let directory = input.drawn.directories.get(&row.terminal);
    let folder = directory.map(String::as_str);
    let shown_name = row_name(
        &row.title,
        has_agent,
        folder,
        input.home,
        input.shell_row_name,
    );
    let label = shown_name.as_str();
    let state = agent_state(&input.drawn.agent_rows, &row.terminal);
    let name = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(label.to_string());
    let mut shown = div()
        .group(ROW_GROUP)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(ROW_HEIGHT))
        .px(px(8.))
        .rounded(px(ROW_RADIUS))
        .child(row_mark(state, &row.terminal, input))
        .child(name)
        .child(close_button(&row.terminal, look, cx));
    if selected {
        shown = raised(shown, look);
    } else {
        shown = shown.hover(|style| style.bg(look.veil));
    }
    if let Some(mark) = input.mark
        && mark.target == MarkTarget::Row(index)
        && mark.edge == MarkEdge::Whole
    {
        shown = shown.bg(look.strong_veil);
    }
    let mut shift = 0.;
    if let Some(slot) = input.slot {
        shift = slot.shift_of_row(index);
    }
    let terminal_name = &row.terminal.0;
    shown = drive::mark(shown, format!("row:{terminal_name}"));

    let selected_terminal = row.terminal.clone();
    let popped = row.terminal.clone();
    let drag = ListDrag {
        dragged: Dragged::Row(row.terminal.clone()),
        label: label.to_string(),
        look: look.clone(),
    };
    shown
        .id(element_id("row", &row.terminal.0))
        .on_click(cx.listener(move |view, _, _, cx| view.select(&selected_terminal, cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                view.open_row_popup(popped.clone(), event.position, cx);
            }),
        )
        .on_drag(drag, drag_preview)
        .on_drag_move(
            cx.listener(move |view, event: &DragMoveEvent<ListDrag>, _, cx| {
                let pointer = event.event.position;
                let Some(fraction) = fraction_at(event.bounds, pointer, shift) else {
                    return;
                };
                let drag = event.drag(cx);
                let dragged = drag.dragged.clone();
                let point = DropPoint::Row { index, fraction };
                view.drag_over(&dragged, point, cx);
            }),
        )
}

fn close_button(terminal: &TerminalId, look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let closed = terminal.clone();
    let trash = svg()
        .path(TRASH_ICON)
        .size(px(ROW_ICON_SIZE))
        .text_color(transparent_black())
        .group_hover(ROW_GROUP, |style| style.text_color(look.secondary_text));
    div()
        .id(element_id("close", &terminal.0))
        .flex_none()
        .child(trash)
        .on_click(cx.listener(move |view, _, _, cx| {
            // The row behind it would select itself on the same click.
            cx.stop_propagation();
            view.close(&closed);
        }))
}

fn agent_state(
    agent_rows: &BTreeMap<TerminalId, AgentRow>,
    terminal: &TerminalId,
) -> Option<RowState> {
    let agent_row = agent_rows.get(terminal)?;
    Some(agent_row.state)
}

/// The pointer's height within the hovered element, 0 to 1.
fn drag_preview(
    drag: &ListDrag,
    _position: Point<Pixels>,
    _window: &mut Window,
    cx: &mut App,
) -> gpui::Entity<DragPreview> {
    let preview = DragPreview {
        label: drag.label.clone(),
        look: drag.look.clone(),
    };
    cx.new(|_| preview)
}

fn element_id(kind: &str, name: &str) -> ElementId {
    let id = SharedString::from(format!("{kind}:{name}"));
    ElementId::Name(id)
}

// The "taco shell slot" Lucas chose: a row-sized gap outlined in the shell's orange.
fn slot_element(slot: Slot) -> AnyElement {
    let opened = slot.height / ROW_HEIGHT;
    let wash = shell_orange().opacity(SLOT_WASH);
    let outline = shell_outline(SLOT_OUTLINE, ROW_RADIUS);
    div()
        .relative()
        .flex_none()
        .h(px(slot.height))
        .rounded(px(ROW_RADIUS))
        .opacity(opened)
        .bg(wash)
        .child(outline)
        .into_any_element()
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
            // Turned in the scene the window already has: no view is drawn again while it spins.
            let turn = TimedTransformation::rotation(SPIN_PERIOD, Transformation::default());
            let spin = turn.with_max_fps(MARK_FPS);
            let ring = svg()
                .path(WORKING_ICON)
                .size(px(MARK_SIZE))
                .with_timed_transformation(spin);
            ring.flex_none().text_color(look.working).into_any_element()
        }
        Some(RowState::Needs) => {
            let ripple = ripple(look);
            let dot = place.relative().rounded_full().bg(look.needs);
            dot.child(ripple).into_any_element()
        }
        Some(RowState::Done) => {
            let check = svg().path(DONE_ICON).size(px(MARK_SIZE));
            check.flex_none().text_color(look.done).into_any_element()
        }
        Some(RowState::Idle) | None => place.into_any_element(),
    }
}

// gpui draws a one-shot animation finished at once, with no frames, under Reduce Motion.
fn row_mark(state: Option<RowState>, terminal: &TerminalId, input: &ListInput) -> AnyElement {
    let just_done = state == Some(RowState::Done) && input.drawing_checks.contains(terminal);
    if just_done {
        return drawing_check(terminal, input.look);
    }
    state_mark(state, input.look)
}

// The site's ts-draw: the check's stroke runs left to right, so revealing it from the left draws it.
fn drawing_check(terminal: &TerminalId, look: &Look) -> AnyElement {
    let check = svg()
        .path(DONE_ICON)
        .flex_none()
        .size(px(MARK_SIZE))
        .text_color(look.done);
    let reveal = div()
        .flex_none()
        .h(px(MARK_SIZE))
        .overflow_hidden()
        .child(check);
    let id = element_id("drawing-check", &terminal.0);
    let draw = Animation::new(DRAW_TIME)
        .with_easing(ease_out)
        .with_max_fps(MARK_FPS);
    let drawing = reveal.with_animation(id, draw, |reveal, delta| reveal.w(px(MARK_SIZE * delta)));
    div()
        .flex_none()
        .size(px(MARK_SIZE))
        .child(drawing)
        .into_any_element()
}

// CSS's ease-out, which the site's check uses.
fn ease_out(delta: f32) -> f32 {
    let left = 1. - delta;
    1. - left * left
}

// The site's needs-you ripple: a copy of the dot that grows to 2.8 times its size as it fades from half to nothing.
fn ripple(look: &Look) -> AnyElement {
    let box_size = MARK_SIZE * RIPPLE_GROWTH;
    let offset = (box_size - MARK_SIZE) / 2.;
    let start = Transformation::alpha(RIPPLE_START_ALPHA);
    let end = Transformation::scale(size(RIPPLE_GROWTH, RIPPLE_GROWTH)).with_alpha(0.);
    let pulse = TimedTransformation::new(RIPPLE_PERIOD)
        .from(start)
        .to(end)
        .with_easing(Easing::EaseOutQuint)
        .with_max_fps(MARK_FPS);
    let spread = svg()
        .path(RIPPLE_ICON)
        .size(px(box_size))
        .with_timed_transformation(pulse)
        .text_color(look.needs);
    div()
        .absolute()
        .left(px(-offset))
        .top(px(-offset))
        .child(spread)
        .into_any_element()
}

fn new_terminal_row(look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let plus = svg()
        .path(PLUS_ICON)
        .size(px(ROW_ICON_SIZE))
        .flex_none()
        .text_color(look.secondary_text);
    div()
        .id("new-terminal-row")
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .h(px(ROW_HEIGHT))
        .px(px(6.))
        .rounded(px(ROW_RADIUS))
        .text_color(look.secondary_text)
        .hover(|style| style.bg(look.veil))
        .child(plus)
        .child(NEW_TERMINAL)
        .on_click(cx.listener(|view, _, _, cx| view.open_terminal_in(None, cx)))
}
