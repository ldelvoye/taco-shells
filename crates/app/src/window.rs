use gpui::{AnyElement, Context, Div, IntoElement, ParentElement, Render, Styled, Window, div, px};
use taco_protocol::{DrawnState, Edge, Panel, PanelPlace};

use crate::banners::{Banner, banner_cards, banner_strips};
use crate::look::{FONT_FAMILY, Look, TEXT_SIZE};
use crate::panels::{docked, open_on, title_plus};
use crate::platform::TrafficLights;
use crate::strip::strip;
use crate::terminal_list::terminal_list;
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
}

impl WindowView {
    pub fn new(shown: Shown, look: Look, trial: Trial, traffic_lights: TrafficLights) -> Self {
        Self {
            shown,
            look,
            trial,
            traffic_lights,
        }
    }

    pub fn set_look(&mut self, look: Look) {
        self.look = look;
    }

    fn body(&self) -> Div {
        let drawn = &self.shown.drawn;
        let look = &self.look;
        let mut center = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(self.middle());
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Bottom) {
            center = center.child(self.panel(panel, place));
        }

        let mut body = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(strip(&drawn.panels, look));
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Left) {
            body = body.child(self.panel(panel, place));
        }
        body = body.child(center);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Right) {
            body = body.child(self.panel(panel, place));
        }
        body
    }

    fn panel(&self, panel: Panel, place: &PanelPlace) -> Div {
        let look = &self.look;
        match panel {
            Panel::Terminals => {
                let list = terminal_list(&self.shown.drawn, look, self.trial.plus);
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
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let look = &self.look;
        let full_screen = window.is_fullscreen();
        let bar = title_bar(&self.shown.drawn, look, full_screen, self.traffic_lights);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(look.panels)
            .text_color(look.text)
            .font_family(FONT_FAMILY)
            .text_size(px(TEXT_SIZE))
            .child(bar)
            .child(self.body())
    }
}
