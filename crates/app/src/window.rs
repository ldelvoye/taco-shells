use std::rc::Rc;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    AnyElement, Context, Div, IntoElement, ParentElement, Render, Styled, Task, Window, div, px,
};
use taco_protocol::{
    Dragged, DrawnState, DropPlace, Edge, Panel, PanelPlace, TabGroupId, TerminalId,
    WorkspaceRequest,
};
use taco_rules::{DropMark, DropPoint, drop_for, drop_mark_for, selected_terminal};

use crate::banners::{Banner, banner_cards, banner_strips};
use crate::link::{Ahead, WorkspacePort, drawn_with};
use crate::look::{FONT_FAMILY, Look, TEXT_SIZE};
use crate::panels::{docked, open_on, title_plus};
use crate::platform::TrafficLights;
use crate::strip::strip;
use crate::terminal_list::{ListDrag, ListInput, terminal_list};
use crate::title_bar::title_bar;
use crate::trial::{BannerLayout, PlusPlace, Trial};

const PLACEHOLDER_FONT: &str = "Menlo";
const PLACEHOLDER_TEXT_SIZE: f32 = 12.;

/// What the window draws: the workspace's state, and what the services and the app put beside it.
#[derive(Clone, Debug)]
pub struct Shown {
    pub drawn: DrawnState,
    pub banners: Vec<Banner>,
    /// Lines drawn in the middle in place of a terminal.
    pub placeholder_screen: Vec<String>,
}

/// The one window's root view.
pub struct WindowView {
    shown: Shown,
    look: Look,
    trial: Trial,
    traffic_lights: TrafficLights,
    port: Rc<dyn WorkspacePort>,
    ahead: Option<Ahead>,
    /// How many focus moves this window has made, counting on from the workspace's count.
    moves: u64,
    /// Where the drag in progress would land, if it would land anywhere.
    drop_place: Option<DropPlace>,
    _published: Task<()>,
}

impl WindowView {
    pub fn new(
        shown: Shown,
        look: Look,
        trial: Trial,
        traffic_lights: TrafficLights,
        port: Rc<dyn WorkspacePort>,
        published: UnboundedReceiver<DrawnState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let moves = shown.drawn.focus_count;
        let following = cx.spawn(async move |this, cx| {
            let mut published = published;
            while let Some(state) = published.next().await {
                let shown = this.update(cx, |view, cx| view.show(state, cx));
                if shown.is_err() {
                    return;
                }
            }
        });
        Self {
            shown,
            look,
            trial,
            traffic_lights,
            port,
            ahead: None,
            moves,
            drop_place: None,
            _published: following,
        }
    }

    pub fn set_look(&mut self, look: Look) {
        self.look = look;
    }

    fn show(&mut self, state: DrawnState, cx: &mut Context<Self>) {
        if state.revision <= self.shown.drawn.revision {
            return;
        }
        self.shown.drawn = state;
        cx.notify();
    }

    fn drawn(&self) -> DrawnState {
        let (drawn, _) = drawn_with(&self.shown.drawn, self.ahead.as_ref());
        drawn
    }

    // Only the window holds the keyboard, so a selection is drawn at once and reported after.
    pub(crate) fn select(&mut self, terminal: &TerminalId, cx: &mut Context<Self>) {
        let drawn = self.drawn();
        let current = selected_terminal(drawn.arrangement.as_ref());
        if current == Some(terminal) {
            return;
        }
        self.moves += 1;
        let count = self.moves;
        self.ahead = Some(Ahead {
            terminal: terminal.clone(),
            count,
        });
        let reported = WorkspaceRequest::FocusReported {
            terminal: terminal.clone(),
            count,
        };
        self.port.send(reported);
        cx.notify();
    }

    pub(crate) fn close(&mut self, terminal: &TerminalId, _cx: &mut Context<Self>) {
        let request = WorkspaceRequest::CloseTerminal {
            terminal: terminal.clone(),
        };
        self.port.send(request);
    }

    pub(crate) fn toggle_group(&mut self, tab_group: &TabGroupId, _cx: &mut Context<Self>) {
        let request = WorkspaceRequest::TabGroupToggled {
            id: tab_group.clone(),
        };
        self.port.send(request);
    }

    pub(crate) fn open_terminal_in(
        &mut self,
        tab_group: Option<TabGroupId>,
        _cx: &mut Context<Self>,
    ) {
        let request = WorkspaceRequest::OpenTerminalIn { tab_group };
        self.port.send(request);
    }

    pub(crate) fn drag_over(
        &mut self,
        dragged: &Dragged,
        point: DropPoint,
        cx: &mut Context<Self>,
    ) {
        let drawn = self.drawn();
        let Some(arrangement) = &drawn.arrangement else {
            return;
        };
        let place = drop_for(arrangement, dragged, &point);
        if place == self.drop_place {
            return;
        }
        self.drop_place = place;
        cx.notify();
    }

    pub(crate) fn drop_on_list(&mut self, drag: &ListDrag, cx: &mut Context<Self>) {
        let place = self.drop_place.take();
        cx.notify();
        let Some(place) = place else {
            return;
        };
        let request = WorkspaceRequest::Dropped {
            dragged: drag.dragged.clone(),
            place,
        };
        self.port.send(request);
    }

    fn body(&self, drawn: &DrawnState, mark: Option<&DropMark>, cx: &mut Context<Self>) -> Div {
        let look = &self.look;
        let mut center = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.middle());
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Bottom) {
            center = center.child(self.panel(panel, place, drawn, mark, cx));
        }

        let mut body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(strip(&drawn.panels, look));
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Left) {
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        body = body.child(center);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Right) {
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        body
    }

    fn panel(
        &self,
        panel: Panel,
        place: &PanelPlace,
        drawn: &DrawnState,
        mark: Option<&DropMark>,
        cx: &mut Context<Self>,
    ) -> Div {
        let look = &self.look;
        match panel {
            Panel::Terminals => {
                let input = ListInput {
                    drawn,
                    mark,
                    look,
                    plus: self.trial.plus,
                };
                let list = terminal_list(&input, cx);
                let mut extra = None;
                if self.trial.plus == PlusPlace::Title {
                    extra = Some(title_plus(look));
                }
                docked(panel, place, extra, list, look)
            }
            Panel::AgentActivity => docked(panel, place, None, div(), look),
        }
    }

    // The selected terminal, with the banners over it or above it.
    fn middle(&self) -> Div {
        let look = &self.look;
        let banners = &self.shown.banners;
        let mut terminal = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(look.terminal_background)
            .child(self.placeholder());
        let mut middle = div().flex().flex_col().flex_1().min_h_0();
        if banners.is_empty() {
            return middle.child(terminal);
        }
        match self.trial.banners {
            BannerLayout::Cards => {
                terminal = terminal.child(banner_cards(banners, look));
            }
            BannerLayout::Strips => {
                middle = middle.children(banner_strips(banners, look));
            }
        }
        middle.child(terminal)
    }

    fn placeholder(&self) -> AnyElement {
        let lines = self
            .shown
            .placeholder_screen
            .iter()
            .map(|line| div().whitespace_nowrap().child(line.clone()));
        div()
            .p(px(10.))
            .font_family(PLACEHOLDER_FONT)
            .text_size(px(PLACEHOLDER_TEXT_SIZE))
            .children(lines)
            .into_any_element()
    }
}

impl Render for WindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (drawn, kept) = drawn_with(&self.shown.drawn, self.ahead.as_ref());
        self.ahead = kept;
        // A drag that ended outside the list leaves nothing to mark.
        if !cx.has_active_drag() {
            self.drop_place = None;
        }
        let mut mark = None;
        if let (Some(arrangement), Some(place)) = (&drawn.arrangement, &self.drop_place) {
            mark = drop_mark_for(arrangement, place);
        }

        let look = &self.look;
        let full_screen = window.is_fullscreen();
        let bar = title_bar(&drawn, look, full_screen, self.traffic_lights);
        let body = self.body(&drawn, mark.as_ref(), cx);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(look.panels)
            .text_color(look.text)
            .font_family(FONT_FAMILY)
            .text_size(px(TEXT_SIZE))
            .child(bar)
            .child(body)
    }
}
