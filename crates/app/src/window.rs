use std::collections::HashMap;
use std::rc::Rc;

use futures::StreamExt;
use gpui::{
    AnyElement, AppContext, Context, Div, Focusable, IntoElement, ParentElement, Pixels, Point,
    Render, Styled, Task, Window, div, px,
};
use taco_protocol::{
    Dragged, DrawnState, DropPlace, Edge, Panel, PanelPlace, RequestReply, TabGroupId, TerminalId,
    WorkspaceRequest,
};
use taco_rules::{DropMark, DropPoint, drop_for, drop_mark_for, selected_terminal};

use crate::banners::{Banner, banner_cards, banner_strips};
use crate::drive::mark;
use crate::link::{Ahead, WindowOutput, WindowReceiver, WorkspacePort, drawn_with, next_move};
use crate::look::{FONT_FAMILY, Look, TEXT_SIZE};
use crate::panels::{docked, open_on, title_plus};
use crate::platform::TrafficLights;
use crate::popups::{Popup, popup_layer};
use crate::strip::strip;
use crate::terminal_list::{ListDrag, ListInput, terminal_list};
use crate::text_field::{TextField, TextFieldEvent};
use crate::title_bar::title_bar;
use crate::trial::{BannerLayout, PlusPlace, Trial};

const MIDDLE_MARK: &str = "middle";
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
    /// The number of this window's last focus move.
    last_move: u64,
    drop_place: Option<DropPlace>,
    popup: Option<Popup>,
    /// Set when the group popup's name field has just opened and should take the keyboard.
    focus_field: bool,
    last_token: u64,
    awaiting: HashMap<u64, Awaited>,
    _following: Task<()>,
}

/// What the window does with a reply it asked for.
enum Awaited {
    /// Open the new group's popup where the row's popup was.
    NewGroup { at: Point<Pixels> },
}

impl WindowView {
    pub fn new(
        shown: Shown,
        look: Look,
        trial: Trial,
        traffic_lights: TrafficLights,
        port: Rc<dyn WorkspacePort>,
        outputs: WindowReceiver,
        cx: &mut Context<Self>,
    ) -> Self {
        let last_move = shown.drawn.focus_count;
        let following = cx.spawn(async move |this, cx| {
            let mut outputs = outputs;
            while let Some(output) = outputs.next().await {
                let received = this.update(cx, |view, cx| view.receive(output, cx));
                if received.is_err() {
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
            last_move,
            drop_place: None,
            popup: None,
            focus_field: false,
            last_token: 0,
            awaiting: HashMap::new(),
            _following: following,
        }
    }

    pub fn set_look(&mut self, look: Look) {
        self.look = look;
    }

    fn receive(&mut self, output: WindowOutput, cx: &mut Context<Self>) {
        match output {
            WindowOutput::State(state) => self.show(state, cx),
            WindowOutput::Replied { token, reply } => self.replied(token, reply, cx),
            WindowOutput::Emptied => cx.quit(),
            // Nothing in the window asks for these yet.
            WindowOutput::Activity(_) | WindowOutput::Services(_) => {}
        }
    }

    fn replied(&mut self, token: u64, reply: RequestReply, cx: &mut Context<Self>) {
        let Some(awaited) = self.awaiting.remove(&token) else {
            return;
        };
        match awaited {
            Awaited::NewGroup { at } => {
                let Some(tab_group) = reply.tab_group else {
                    return;
                };
                self.open_group_popup(tab_group, at, cx);
            }
        }
    }

    fn send_awaiting(&mut self, request: WorkspaceRequest, awaited: Awaited) {
        self.last_token += 1;
        let token = self.last_token;
        self.awaiting.insert(token, awaited);
        self.port.send(request, Some(token));
    }

    pub(crate) fn open_row_popup(
        &mut self,
        terminal: TerminalId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.popup = Some(Popup::Row { terminal, at });
        cx.notify();
    }

    pub(crate) fn open_group_popup(
        &mut self,
        tab_group: TabGroupId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let name = self.tab_group_name(&tab_group);
        let look = self.look.clone();
        let field = cx.new(|cx| TextField::new(&name, look, cx));
        let renamed = tab_group.clone();
        let edits = cx.subscribe(
            &field,
            move |view, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Edited(text) => view.rename(&renamed, text),
                TextFieldEvent::Submitted => view.close_popup(cx),
            },
        );
        self.popup = Some(Popup::Group {
            tab_group,
            at,
            field,
            _edits: edits,
        });
        self.focus_field = true;
        cx.notify();
    }

    fn tab_group_name(&self, tab_group: &TabGroupId) -> String {
        let drawn = self.drawn();
        let Some(arrangement) = &drawn.arrangement else {
            return String::new();
        };
        let mut tab_groups = arrangement.tab_groups.iter();
        let Some(found) = tab_groups.find(|group| &group.id == tab_group) else {
            return String::new();
        };
        found.name.clone()
    }

    pub(crate) fn close_popup(&mut self, cx: &mut Context<Self>) {
        if self.popup.is_none() {
            return;
        }
        self.popup = None;
        cx.notify();
    }

    pub(crate) fn fork(&mut self, terminal: &TerminalId, cx: &mut Context<Self>) {
        let request = WorkspaceRequest::ForkTerminal {
            terminal: terminal.clone(),
        };
        self.port.send(request, None);
        self.close_popup(cx);
    }

    pub(crate) fn new_group(
        &mut self,
        terminal: &TerminalId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let request = WorkspaceRequest::TabGroupCreated {
            terminal: terminal.clone(),
        };
        self.send_awaiting(request, Awaited::NewGroup { at });
        self.close_popup(cx);
    }

    pub(crate) fn ungroup(&mut self, tab_group: &TabGroupId, cx: &mut Context<Self>) {
        let request = WorkspaceRequest::TabGroupUngrouped {
            id: tab_group.clone(),
        };
        self.port.send(request, None);
        self.close_popup(cx);
    }

    fn rename(&self, tab_group: &TabGroupId, name: &str) {
        let request = WorkspaceRequest::TabGroupRenamed {
            id: tab_group.clone(),
            name: name.to_string(),
        };
        self.port.send(request, None);
    }

    fn show(&mut self, state: DrawnState, cx: &mut Context<Self>) {
        if state.revision <= self.shown.drawn.revision {
            return;
        }
        self.shown.drawn = state;
        cx.notify();
    }

    /// The published state with the window's own focus move laid over it: what the window draws.
    pub(crate) fn drawn(&self) -> DrawnState {
        let (drawn, _) = drawn_with(&self.shown.drawn, self.ahead.as_ref());
        drawn
    }

    // Drawn at once, ahead of the workspace, so a key typed straight after a selection reaches the new terminal.
    pub(crate) fn select(&mut self, terminal: &TerminalId, cx: &mut Context<Self>) {
        let drawn = self.drawn();
        let current = selected_terminal(drawn.arrangement.as_ref());
        if current == Some(terminal) {
            return;
        }
        let count = next_move(self.last_move, self.shown.drawn.focus_count);
        self.last_move = count;
        self.ahead = Some(Ahead {
            terminal: terminal.clone(),
            count,
        });
        let reported = WorkspaceRequest::FocusReported {
            terminal: terminal.clone(),
            count,
        };
        self.port.send(reported, None);
        cx.notify();
    }

    pub(crate) fn close(&self, terminal: &TerminalId) {
        let request = WorkspaceRequest::CloseTerminal {
            terminal: terminal.clone(),
        };
        self.port.send(request, None);
    }

    pub(crate) fn toggle_group(&self, tab_group: &TabGroupId) {
        let request = WorkspaceRequest::TabGroupToggled {
            id: tab_group.clone(),
        };
        self.port.send(request, None);
    }

    pub(crate) fn open_terminal_in(&self, tab_group: Option<TabGroupId>) {
        let request = WorkspaceRequest::OpenTerminalIn { tab_group };
        self.port.send(request, None);
    }

    pub(crate) fn leave_list(&mut self, cx: &mut Context<Self>) {
        if self.drop_place.is_none() {
            return;
        }
        self.drop_place = None;
        cx.notify();
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
        self.port.send(request, None);
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
        terminal = mark(terminal, MIDDLE_MARK);
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
        // Once no drag is under way, there is nothing to mark.
        if !cx.has_active_drag() {
            self.drop_place = None;
        }
        let mut mark = None;
        if let (Some(arrangement), Some(place)) = (&drawn.arrangement, &self.drop_place) {
            mark = drop_mark_for(arrangement, place);
        }

        let popup_stays = match &self.popup {
            Some(popup) => popup.still_shown_in(&drawn),
            None => false,
        };
        if !popup_stays {
            self.popup = None;
        }
        if self.focus_field
            && let Some(Popup::Group { field, .. }) = &self.popup
        {
            let handle = field.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
            self.focus_field = false;
        }

        let look = &self.look;
        let full_screen = window.is_fullscreen();
        let bar = title_bar(&drawn, look, full_screen, self.traffic_lights);
        let body = self.body(&drawn, mark.as_ref(), cx);
        let mut root = div()
            .flex()
            .flex_col()
            .size_full()
            .bg(look.panels)
            .text_color(look.text)
            .font_family(FONT_FAMILY)
            .text_size(px(TEXT_SIZE))
            .child(bar)
            .child(body);
        if let Some(popup) = &self.popup {
            root = root.child(popup_layer(popup, &drawn, look, cx));
        }
        root
    }
}
