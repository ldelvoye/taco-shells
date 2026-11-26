use gpui::{Div, ParentElement, Styled, div, px, svg};
use taco_protocol::{Edge, Panel, PanelPlace, Panels};

use crate::assets::{AGENT_ACTIVITY_ICON, SETTINGS_ICON, TERMINALS_ICON};
use crate::drive::mark;
use crate::look::{ICON_SIZE, Look, STRIP_BUTTON_SIZE, STRIP_WIDTH};
use crate::panels::driver_name_of;

const SETTINGS_MARK: &str = "strip:settings";
const EDGE_BAR_THICKNESS: f32 = 2.;
const EDGE_BAR_LENGTH: f32 = 12.;

pub fn icon_of(panel: Panel) -> &'static str {
    match panel {
        Panel::Terminals => TERMINALS_ICON,
        Panel::AgentActivity => AGENT_ACTIVITY_ICON,
    }
}

/// The strip of icons down the left edge: the panels in a fixed order, then Settings at the foot.
pub fn strip(panels: &Panels, look: &Look) -> Div {
    let terminals = panel_button(Panel::Terminals, &panels.terminals, look);
    let agent_activity = panel_button(Panel::AgentActivity, &panels.agent_activity, look);
    let settings_button = button(SETTINGS_ICON, false, look);
    let settings = mark(settings_button, SETTINGS_MARK);
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

fn panel_button(panel: Panel, place: &PanelPlace, look: &Look) -> Div {
    let icon = icon_of(panel);
    let shown = button(icon, place.open, look);
    let barred = shown.child(edge_bar(place.edge, place.open, look));
    let driver_name = driver_name_of(panel);
    mark(barred, format!("strip:{driver_name}"))
}

fn button(icon: &'static str, open: bool, look: &Look) -> Div {
    let mut ink = look.secondary_text;
    let mut shown = div()
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .size(px(STRIP_BUTTON_SIZE))
        .rounded(px(6.));
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
