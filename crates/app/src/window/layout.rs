//! The window's layout: the body, its panels and the middle.

use std::time::Instant;

use gpui::{
    AnyElement, Context, Div, InteractiveElement, IntoElement, ParentElement, Styled, div, px,
};
use taco_protocol::{DrawnState, Edge, Panel, PanelPlace};
use taco_rules::{DropMark, open_on, selected_terminal};

use super::WindowView;
use crate::activity_panel::{ActivityInput, activity_panel, now_ms};
use crate::banners::{Notices, banner_cards, banners_of};
use crate::drive::mark;
use crate::look::{STRIP_WIDTH, TITLE_BAR_HEIGHT};
use crate::motion::{PAGE_TIME, progress};
use crate::panels::{bounded_size, docked, landing_preview};
use crate::shared_view::shared_view;
use crate::strip::strip;
use crate::terminal_list::{ListInput, terminal_list};

const MIDDLE_MARK: &str = "middle";
const PLACEHOLDER_FONT: &str = "Menlo";
const PLACEHOLDER_TEXT_SIZE: f32 = 12.;
const TERMINAL_INSET_SIDE: f32 = 8.;
const TERMINAL_INSET_TOP: f32 = 6.;

impl WindowView {
    pub(super) fn body(
        &self,
        drawn: &DrawnState,
        mark: Option<&DropMark>,
        cx: &mut Context<Self>,
    ) -> Div {
        let look = &self.look;
        let middle = self.middle(drawn, cx);
        let mut center = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(middle);
        let pages_open = !self.pages.open.is_empty();
        let side_strip = strip(&drawn.panels, pages_open, look, cx);
        let mut body = div().relative().flex().flex_1().min_h_0().child(side_strip);
        // A page stands alone beside the strip; the panels come back as they were when it closes.
        if pages_open {
            return body.child(center);
        }
        let panels = &drawn.panels;
        if let Some(panel) = open_on(panels, Edge::Bottom) {
            let place = panels.place(panel);
            center = center.child(self.panel(panel, place, drawn, mark, cx));
        }
        if let Some(panel) = open_on(panels, Edge::Left) {
            let place = panels.place(panel);
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        body = body.child(center);
        if let Some(panel) = open_on(panels, Edge::Right) {
            let place = panels.place(panel);
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        if let Some((panel, edge)) = self.arrangement.panel_target {
            let place = panels.place(panel);
            let size = bounded_size(edge, place.size, self.viewport);
            let preview = landing_preview(panel, edge, size, look);
            body = body.child(preview);
        }
        // A closed page fades out over the whole area beside the strip, uncovering the window as it was.
        let now = Instant::now();
        let reduce_motion = cx.reduce_motion();
        if let Some((left, since)) = self.pages.leaving {
            let gone = progress(since, now, PAGE_TIME, reduce_motion);
            if gone < 1. {
                let remaining = 1. - gone;
                let fading = self
                    .page_element(left, now, reduce_motion, cx)
                    .opacity(remaining);
                // gpui has no hit area that lets a click through, so the fading page is covered: a click on it in
                // these few frames does nothing rather than reach a page that is already closed.
                let shield = div()
                    .id("leaving-page-shield")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude();
                let leaving_area = div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(STRIP_WIDTH))
                    .right_0()
                    .child(fading)
                    .child(shield);
                body = body.child(leaving_area);
            }
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
        let edge = place.edge;
        // A size saved in a larger window, or edited by hand, must not cover the middle.
        let mut size = bounded_size(edge, place.size, self.viewport);
        if let Some((resized, live)) = self.arrangement.resizing
            && resized == panel
        {
            size = live;
        }
        match panel {
            Panel::Terminals => {
                let now = Instant::now();
                let drawing_checks = self.done_checks.drawing(now);
                let input = ListInput {
                    drawn,
                    mark,
                    look,
                    drawing_checks: &drawing_checks,
                    home: self.home.as_deref(),
                    shell_row_name: self.shown.settings.shell_row_name,
                    slot: self.arrangement.slot,
                };
                let list = terminal_list(&input, cx);
                docked(panel, edge, size, list, look, cx)
            }
            Panel::AgentActivity => {
                let terminal = selected_terminal(drawn.arrangement.as_ref());
                let mut agent_row = None;
                if let Some(terminal) = terminal {
                    agent_row = drawn.agent_rows.get(terminal);
                }
                let input = ActivityInput {
                    terminal,
                    agent_row,
                    notices: &self.shown.setups.notices,
                    log: &self.activity.log,
                    folds_open: &self.activity.folds_open,
                    texts_open: &self.activity.texts_open,
                    shared_all_shown: self.activity.shared_all_shown,
                    now_ms: now_ms(),
                    look,
                };
                let contents = activity_panel(&input, cx);
                docked(panel, edge, size, contents, look, cx)
            }
        }
    }

    // The selected terminal, with the banners as cards over it so they never change its size.
    fn middle(&self, drawn: &DrawnState, cx: &mut Context<Self>) -> Div {
        let look = &self.look;
        let workspace_problem = drawn.problem.as_deref();
        let page = self.pages.open.last().copied();
        let without_config;
        let mut notices = &self.shown.notices;
        let mut banners_below = 0.;
        if page.is_some() {
            // The page's own card shows the config problems.
            without_config = Notices {
                config_problems: Vec::new(),
                ..self.shown.notices.clone()
            };
            notices = &without_config;
            banners_below = TITLE_BAR_HEIGHT;
        }
        let banners = banners_of(notices, workspace_problem, &self.dismissed);
        let mut terminal = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(look.terminal_background);
        let now = Instant::now();
        let reduce_motion = cx.reduce_motion();
        match page {
            Some(shown) => {
                let appeared = self.page_appeared(now, reduce_motion);
                let drawn_page = self.page_element(shown, now, reduce_motion, cx);
                let fading_in = drawn_page.opacity(appeared).track_focus(&self.pages.focus);
                terminal = terminal.child(fading_in);
            }
            None => match &self.activity.opened {
                Some(opened) => {
                    let thing =
                        shared_view(opened, look, cx).track_focus(&self.activity.opened_focus);
                    terminal = terminal.child(thing);
                }
                None => terminal = terminal.child(self.terminal_or_placeholder()),
            },
        }
        if !banners.is_empty() {
            let cards = banner_cards(&banners, banners_below, look, cx);
            terminal = terminal.child(cards);
        }
        terminal = mark(terminal, MIDDLE_MARK);
        div().flex().flex_col().flex_1().min_h_0().child(terminal)
    }

    fn terminal_or_placeholder(&self) -> AnyElement {
        let Some(terminals) = &self.terminals else {
            return self.placeholder();
        };
        // The grid is laid out inside the inset, so the size the view reports shrinks with it.
        let inset = div()
            .size_full()
            .pl(px(TERMINAL_INSET_SIDE))
            .pr(px(TERMINAL_INSET_SIDE))
            .pt(px(TERMINAL_INSET_TOP))
            .child(terminals.clone());
        inset.into_any_element()
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
