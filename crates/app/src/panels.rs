use gpui::{
    AnyElement, App, AppContext, Context, CursorStyle, Div, ElementId, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Pixels, Point, Render, SharedString, Size,
    StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use taco_protocol::{Edge, Panel, PanelPlace, Panels};

use crate::assets::CLOSE_ICON;
use crate::drive::mark;
use crate::look::{FONT_FAMILY, Look, PANEL_TITLE_HEIGHT, STRIP_WIDTH, TEXT_SIZE};
use crate::window::WindowView;

const TITLE_ICON_SIZE: f32 = 14.;
const RESIZE_HANDLE: f32 = 5.;
const DROP_BAND: f32 = 72.;
const MIN_SIDE_PANEL: f32 = 140.;
const MIN_BOTTOM_PANEL: f32 = 120.;
const MAX_WINDOW_SHARE: f32 = 0.5;
const DRAG_PREVIEW_WIDTH: f32 = 160.;
const EDGES: [Edge; 3] = [Edge::Left, Edge::Right, Edge::Bottom];

/// A panel being moved by its title.
#[derive(Clone, Debug)]
pub struct PanelDrag {
    pub panel: Panel,
    look: Look,
}

/// A panel being resized by the handle on its inner edge.
#[derive(Clone, Copy, Debug)]
pub struct ResizeDrag {
    pub panel: Panel,
    pub edge: Edge,
}

struct PanelDragPreview {
    panel: Panel,
    look: Look,
}

/// Nothing follows the pointer while a panel is resized: the panel itself moves.
struct NoPreview;

impl Render for PanelDragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        div()
            .w(px(DRAG_PREVIEW_WIDTH))
            .h(px(PANEL_TITLE_HEIGHT))
            .flex()
            .items_center()
            .px(px(12.))
            .bg(look.panels)
            .border_1()
            .border_color(look.hairline)
            .rounded(px(6.))
            .shadow(look.card_shadow())
            .font_family(FONT_FAMILY)
            .text_size(px(TEXT_SIZE))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(look.text)
            .opacity(0.9)
            .child(name_of(self.panel))
    }
}

impl Render for NoPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

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

pub fn place_of(panels: &Panels, panel: Panel) -> &PanelPlace {
    match panel {
        Panel::Terminals => &panels.terminals,
        Panel::AgentActivity => &panels.agent_activity,
    }
}

/// A panel's size with the pointer at `pointer` in a window of `window` size: at least a usable minimum, at most half the window.
pub fn size_for_pointer(edge: Edge, pointer: Point<Pixels>, window: Size<Pixels>) -> f32 {
    let width = f32::from(window.width);
    let height = f32::from(window.height);
    let x = f32::from(pointer.x);
    let y = f32::from(pointer.y);
    let wanted = match edge {
        Edge::Left => x - STRIP_WIDTH,
        Edge::Right => width - x,
        Edge::Bottom => height - y,
    };
    bounded_size(edge, wanted, window)
}

/// A panel's size kept between a usable minimum and half the window, whether it is being dragged or was saved in a larger window.
pub fn bounded_size(edge: Edge, wanted: f32, window: Size<Pixels>) -> f32 {
    let (minimum, room) = match edge {
        Edge::Left | Edge::Right => (MIN_SIDE_PANEL, f32::from(window.width)),
        Edge::Bottom => (MIN_BOTTOM_PANEL, f32::from(window.height)),
    };
    let maximum = room * MAX_WINDOW_SHARE;
    wanted.clamp(minimum, maximum.max(minimum))
}

/// A docked panel: its title, then its contents, `size` along the edge it sits on, with a resize handle on its inner edge.
pub fn docked(
    panel: Panel,
    edge: Edge,
    size: f32,
    contents: impl IntoElement,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let title = panel_title(panel, look, cx);
    let body = div().flex_1().min_h_0().overflow_hidden().child(contents);
    let handle = resize_handle(panel, edge);
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
        .child(body)
        .child(handle);
    let frame = mark(framed, format!("panel:{driver_name}"));
    let extent = px(size);
    match edge {
        Edge::Left => frame.w(extent).border_r_1(),
        Edge::Right => frame.w(extent).border_l_1(),
        Edge::Bottom => frame.h(extent).border_t_1(),
    }
}

fn panel_title(panel: Panel, look: &Look, cx: &mut Context<WindowView>) -> AnyElement {
    let close_icon = svg()
        .path(CLOSE_ICON)
        .size(px(TITLE_ICON_SIZE))
        .text_color(look.secondary_text);
    let driver_name = driver_name_of(panel);
    let close = div()
        .id(element_id("panel-close", driver_name))
        .child(close_icon)
        .on_click(cx.listener(move |view, _, _, cx| {
            // The title behind it would start a move on the same press.
            cx.stop_propagation();
            view.close_panel(panel);
        }));
    let drag = PanelDrag {
        panel,
        look: look.clone(),
    };
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
        .child(close);
    let marked = mark(title, format!("panel:{driver_name}:title"));
    marked
        .id(element_id("panel-title", driver_name))
        .on_drag(drag, panel_preview)
        .into_any_element()
}

fn resize_handle(panel: Panel, edge: Edge) -> AnyElement {
    let driver_name = driver_name_of(panel);
    let handle = div().id(element_id("panel-resize", driver_name)).absolute();
    let placed = match edge {
        Edge::Left => handle
            .top_0()
            .bottom_0()
            .right(px(-RESIZE_HANDLE / 2.))
            .w(px(RESIZE_HANDLE))
            .cursor(CursorStyle::ResizeLeftRight),
        Edge::Right => handle
            .top_0()
            .bottom_0()
            .left(px(-RESIZE_HANDLE / 2.))
            .w(px(RESIZE_HANDLE))
            .cursor(CursorStyle::ResizeLeftRight),
        Edge::Bottom => handle
            .left_0()
            .right_0()
            .top(px(-RESIZE_HANDLE / 2.))
            .h(px(RESIZE_HANDLE))
            .cursor(CursorStyle::ResizeUpDown),
    };
    let resize = ResizeDrag { panel, edge };
    placed
        .on_drag(resize, |_, _, _, cx| cx.new(|_| NoPreview))
        .into_any_element()
}

/// While a panel is moved, a band on each edge it can go to, lit under the pointer.
pub fn drop_bands(look: &Look, cx: &mut Context<WindowView>) -> Div {
    let bands = EDGES.map(|edge| drop_band(edge, look, cx));
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(STRIP_WIDTH))
        .right_0()
        .children(bands)
}

fn drop_band(edge: Edge, look: &Look, cx: &mut Context<WindowView>) -> AnyElement {
    let lit = look.strong_veil;
    let line = look.secondary_text;
    let edge_name = format!("{edge:?}");
    let band = div().id(element_id("drop-band", &edge_name)).absolute();
    let placed = match edge {
        Edge::Left => band.top_0().bottom_0().left_0().w(px(DROP_BAND)),
        Edge::Right => band.top_0().bottom_0().right_0().w(px(DROP_BAND)),
        Edge::Bottom => band.left_0().right_0().bottom_0().h(px(DROP_BAND)),
    };
    placed
        .border_1()
        .border_color(look.hairline)
        .drag_over::<PanelDrag>(move |style, _, _, _| style.bg(lit).border_color(line))
        .on_drop(cx.listener(move |view, drag: &PanelDrag, _, cx| {
            view.move_panel(drag.panel, edge, cx);
        }))
        .into_any_element()
}

fn element_id(kind: &str, name: &str) -> ElementId {
    let id = SharedString::from(format!("{kind}:{name}"));
    ElementId::Name(id)
}

fn panel_preview(
    drag: &PanelDrag,
    _position: Point<Pixels>,
    _window: &mut Window,
    cx: &mut App,
) -> Entity<PanelDragPreview> {
    let preview = PanelDragPreview {
        panel: drag.panel,
        look: drag.look.clone(),
    };
    cx.new(|_| preview)
}

#[cfg(test)]
mod tests {
    use gpui::{px, size};
    use taco_protocol::Edge;

    use super::bounded_size;

    #[test]
    fn a_panel_never_takes_more_than_half_the_window_or_less_than_a_usable_size() {
        let small_window = size(px(800.), px(500.));
        assert_eq!(bounded_size(Edge::Left, 600., small_window), 400.);
        assert_eq!(bounded_size(Edge::Right, 300., small_window), 300.);
        assert_eq!(bounded_size(Edge::Bottom, 400., small_window), 250.);
        assert_eq!(bounded_size(Edge::Left, 20., small_window), 140.);
        assert_eq!(bounded_size(Edge::Bottom, 20., small_window), 120.);
        let tiny_window = size(px(200.), px(200.));
        assert_eq!(bounded_size(Edge::Left, 600., tiny_window), 140.);
    }
}
