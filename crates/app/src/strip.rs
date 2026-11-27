use gpui::{
    Context, Div, ElementId, InteractiveElement, ParentElement, SharedString, Stateful,
    StatefulInteractiveElement, Styled, div, px, svg,
};
use taco_protocol::{Edge, Panel, PanelPlace, Panels};

use crate::assets::{AGENT_ACTIVITY_ICON, SETTINGS_ICON, TERMINALS_ICON};
use crate::drive::mark;
use crate::look::{ICON_SIZE, Look, STRIP_BUTTON_SIZE, STRIP_WIDTH};
use crate::page::PageMove;
use crate::panels::driver_name_of;
use crate::window::WindowView;

const SETTINGS_MARK: &str = "strip:settings";
const SETTINGS_ID: &str = "strip-button:settings";
const EDGE_BAR_THICKNESS: f32 = 2.;
const EDGE_BAR_LENGTH: f32 = 12.;

pub fn icon_of(panel: Panel) -> &'static str {
    match panel {
        Panel::Terminals => TERMINALS_ICON,
        Panel::AgentActivity => AGENT_ACTIVITY_ICON,
    }
}

/// The strip of icons down the left edge: the panels in a fixed order, then Settings at the foot.
pub fn strip(
    panels: &Panels,
    settings_open: bool,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let terminals = panel_button(Panel::Terminals, &panels.terminals, look, cx);
    let agent_activity = panel_button(Panel::AgentActivity, &panels.agent_activity, look, cx);
    let settings_button = button(SETTINGS_ICON, settings_open, look);
    let marked = mark(settings_button, SETTINGS_MARK);
    let settings = marked
        .id(SETTINGS_ID)
        .on_click(cx.listener(|view, _, _, cx| view.move_pages(PageMove::ToggleSettings, cx)));
    div()
        .flex()
        .flex_col()
        .flex_none()
        .items_center()
        .gap(px(4.))
        .w(px(STRIP_WIDTH))
        .py(px(6.))
        .border_r_1()
        .border_color(look.hairline)
        .child(terminals)
        .child(agent_activity)
        .child(div().flex_1())
        .child(settings)
}

fn panel_button(
    panel: Panel,
    place: &PanelPlace,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Stateful<Div> {
    let icon = icon_of(panel);
    let shown = button(icon, place.open, look);
    let barred = shown.child(edge_bar(place.edge, place.open, look));
    let driver_name = driver_name_of(panel);
    let marked = mark(barred, format!("strip:{driver_name}"));
    let id = SharedString::from(format!("strip-button:{driver_name}"));
    marked
        .id(ElementId::Name(id))
        .on_click(cx.listener(move |view, _, _, _| view.toggle_panel(panel)))
}

fn button(icon: &'static str, open: bool, look: &Look) -> Div {
    let mut ink = look.secondary_text;
    let mut shown = div()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .size(px(STRIP_BUTTON_SIZE))
        .rounded(px(6.))
        .hover(|style| style.bg(look.veil));
    if open {
        ink = look.text;
        shown = shown.bg(look.strong_veil);
    }
    let glyph = svg().path(icon).size(px(ICON_SIZE)).text_color(ink);
    shown.child(glyph)
}

fn edge_bar(edge: Edge, open: bool, look: &Look) -> Div {
    let mut ink = look.secondary_text.opacity(0.5);
    if open {
        ink = look.secondary_text;
    }
    let along = (STRIP_BUTTON_SIZE - EDGE_BAR_LENGTH) / 2.;
    let bar = div().absolute().rounded_full().bg(ink);
    match edge {
        Edge::Left => bar
            .left(px(1.))
            .top(px(along))
            .w(px(EDGE_BAR_THICKNESS))
            .h(px(EDGE_BAR_LENGTH)),
        Edge::Right => bar
            .right(px(1.))
            .top(px(along))
            .w(px(EDGE_BAR_THICKNESS))
            .h(px(EDGE_BAR_LENGTH)),
        Edge::Bottom => bar
            .bottom(px(1.))
            .left(px(along))
            .w(px(EDGE_BAR_LENGTH))
            .h(px(EDGE_BAR_THICKNESS)),
    }
}
