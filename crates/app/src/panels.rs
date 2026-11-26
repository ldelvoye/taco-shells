use gpui::{AnyElement, Div, FontWeight, IntoElement, ParentElement, Styled, div, px, svg};
use taco_protocol::{Edge, Panel, PanelPlace, Panels};

use crate::assets::{CLOSE_ICON, PLUS_ICON};
use crate::drive::mark;
use crate::look::{Look, PANEL_TITLE_HEIGHT};

const TITLE_ICON_SIZE: f32 = 14.;

pub fn name_of(panel: Panel) -> &'static str {
    match panel {
        Panel::Terminals => "Terminals",
        Panel::AgentActivity => "Agent Activity",
    }
}

/// The panel's name in the driver's bounds: `terminals` or `agent_activity`.
pub fn driver_name_of(panel: Panel) -> &'static str {
    match panel {
        Panel::Terminals => "terminals",
        Panel::AgentActivity => "agent_activity",
    }
}

/// The panel open on an edge, if any: at most one is, which `rules` keeps true.
pub fn open_on(panels: &Panels, edge: Edge) -> Option<(Panel, &PanelPlace)> {
    let places = [
        (Panel::Terminals, &panels.terminals),
        (Panel::AgentActivity, &panels.agent_activity),
    ];
    let mut all = places.into_iter();
    all.find(|(_, place)| place.open && place.edge == edge)
}

/// A docked panel: its title, then its contents, sized along the edge it sits on.
pub fn docked(
    panel: Panel,
    place: &PanelPlace,
    title_extra: Option<AnyElement>,
    contents: impl IntoElement,
    look: &Look,
) -> Div {
    let title = panel_title(panel, title_extra, look);
    let body = div().flex_1().min_h_0().overflow_hidden().child(contents);
    let driver_name = driver_name_of(panel);
    let framed = div()
        .relative()
        .flex()
        .flex_col()
        .flex_none()
        .min_w_0()
        .min_h_0()
        .border_color(look.hairline)
        .child(title)
        .child(body);
    let frame = mark(framed, format!("panel:{driver_name}"));
    let size = px(place.size);
    match place.edge {
        Edge::Left => frame.w(size).border_r_1(),
        Edge::Right => frame.w(size).border_l_1(),
        Edge::Bottom => frame.h(size).border_t_1(),
    }
}

fn panel_title(panel: Panel, extra: Option<AnyElement>, look: &Look) -> Div {
    let close = svg()
        .path(CLOSE_ICON)
        .size(px(TITLE_ICON_SIZE))
        .text_color(look.secondary_text);
    let mut controls = div().flex().items_center().gap(px(8.));
    if let Some(element) = extra {
        controls = controls.child(element);
    }
    controls = controls.child(close);
    let title = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_between()
        .h(px(PANEL_TITLE_HEIGHT))
        .pl(px(12.))
        .pr(px(10.))
        .border_b_1()
        .border_color(look.hairline)
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(name_of(panel))
        .child(controls);
    let driver_name = driver_name_of(panel);
    mark(title, format!("panel:{driver_name}:title"))
}

/// The new-terminal button for the trial that puts it in the Terminals title.
pub fn title_plus(look: &Look) -> AnyElement {
    let plus = svg()
        .path(PLUS_ICON)
        .size(px(TITLE_ICON_SIZE))
        .text_color(look.secondary_text);
    plus.into_any_element()
}
