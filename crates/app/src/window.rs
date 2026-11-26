use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use futures::StreamExt;
use gpui::{
    AnyElement, AppContext, Context, Div, DragMoveEvent, Focusable, InteractiveElement,
    IntoElement, ParentElement, Pixels, Point, Render, Size, Styled, Task, Window, div, px,
};
use taco_protocol::{
    ActivityRequest, ConfigFile, Dragged, DrawnState, DropPlace, Edge, HarnessId, Panel,
    PanelPlace, RequestReply, TabGroupId, TerminalId, WorkspaceRequest,
};
use taco_rules::{DropMark, DropPoint, drop_for, drop_mark_for, selected_terminal};
use taco_services::{AgentMenuAction, AgentSetupView};

use crate::activity::{ActivityInput, ActivityLog, activity_panel, now_ms};
use crate::banners::{Banner, banner_cards};
use crate::drive::mark;
use crate::link::{
    Ahead, ServicesOutput, ServicesPort, WindowOutput, WindowReceiver, WorkspacePort, drawn_with,
    next_move,
};
use crate::look::{FONT_FAMILY, Look, TEXT_SIZE};
use crate::panels::{
    PanelDrag, ResizeDrag, docked, drop_bands, open_on, place_of, size_for_pointer,
};
use crate::platform::TrafficLights;
use crate::popups::{Popup, popup_layer};
use crate::strip::strip;
use crate::terminal_list::{ListDrag, ListInput, terminal_list};
use crate::text_field::{TextField, TextFieldEvent};
use crate::title_bar::title_bar;

const MIDDLE_MARK: &str = "middle";
const PLACEHOLDER_FONT: &str = "Menlo";
const PLACEHOLDER_TEXT_SIZE: f32 = 12.;
const CLOCK_TICK: Duration = Duration::from_secs(30);

/// What the window draws: the workspace's state, and what the services and the app put beside it.
#[derive(Clone, Debug)]
pub struct Shown {
    pub drawn: DrawnState,
    pub banners: Vec<Banner>,
    /// Lines drawn in the middle in place of a terminal.
    pub placeholder_screen: Vec<String>,
    pub setups: AgentSetupView,
}

/// The one window's root view.
pub struct WindowView {
    shown: Shown,
    look: Look,
    traffic_lights: TrafficLights,
    port: Rc<dyn WorkspacePort>,
    services: Rc<dyn ServicesPort>,
    activity: ActivityLog,
    /// The log lines whose files are shown, by `seq`.
    expanded: HashSet<u64>,
    ahead: Option<Ahead>,
    /// The number of this window's last focus move.
    last_move: u64,
    drop_place: Option<DropPlace>,
    popup: Option<Popup>,
    /// A panel being resized, and the size it is drawn at until the release sends it.
    resizing: Option<(Panel, f32)>,
    /// Whether a panel is being moved by its title, which shows where it can go.
    panel_dragging: bool,
    last_token: u64,
    awaiting: HashMap<u64, Awaited>,
    _following: Task<()>,
    _clock: Task<()>,
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
        traffic_lights: TrafficLights,
        port: Rc<dyn WorkspacePort>,
        services: Rc<dyn ServicesPort>,
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
        // Keeps "Finished 5 minutes ago" true while nothing else redraws the window.
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CLOCK_TICK).await;
                let ticked = this.update(cx, |view, cx| {
                    if view.shown.drawn.panels.agent_activity.open {
                        cx.notify();
                    }
                });
                if ticked.is_err() {
                    return;
                }
            }
        });
        Self {
            shown,
            look,
            traffic_lights,
            port,
            services,
            activity: ActivityLog::default(),
            expanded: HashSet::new(),
            ahead: None,
            last_move,
            drop_place: None,
            popup: None,
            resizing: None,
            panel_dragging: false,
            last_token: 0,
            awaiting: HashMap::new(),
            _following: following,
            _clock: clock,
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
            WindowOutput::Activity(event) => {
                self.activity.apply(event);
                cx.notify();
            }
            WindowOutput::Services(ServicesOutput::Setups(setups)) => {
                self.shown.setups = setups;
                cx.notify();
            }
            // No other service output is drawn.
            WindowOutput::Services(_) => {}
        }
    }

    /// Watches the selected terminal's log while Agent Activity is open, and nothing while it is closed.
    fn watch_activity(&mut self, drawn: &DrawnState) {
        let mut wanted = None;
        if drawn.panels.agent_activity.open {
            let selected = selected_terminal(drawn.arrangement.as_ref());
            wanted = selected.cloned();
        }
        if wanted == self.activity.terminal {
            return;
        }
        self.activity = ActivityLog {
            terminal: wanted.clone(),
            ..ActivityLog::default()
        };
        self.expanded.clear();
        let request = ActivityRequest::Watch { terminal: wanted };
        self.port.activity(request);
    }

    pub(crate) fn set_up(&self, harness: &HarnessId) {
        self.services.agent_action(harness, AgentMenuAction::SetUp);
    }

    pub(crate) fn dismiss_notice(&self, harness: &HarnessId) {
        self.services.dismiss_notice(harness);
    }

    pub(crate) fn toggle_files(&mut self, seq: u64, cx: &mut Context<Self>) {
        let was_open = self.expanded.remove(&seq);
        if !was_open {
            self.expanded.insert(seq);
        }
        cx.notify();
    }

    pub(crate) fn show_older(&self) {
        let Some(terminal) = &self.activity.terminal else {
            return;
        };
        let Some(before) = self.activity.oldest() else {
            return;
        };
        let request = ActivityRequest::Older {
            terminal: terminal.clone(),
            before,
        };
        self.port.activity(request);
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
                // A popup opened since the request is the user's more recent choice.
                if self.popup.is_some() {
                    return;
                }
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
            needs_focus: true,
            _edits: edits,
        });
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

    pub(crate) fn toggle_panel(&self, panel: Panel) {
        let drawn = self.drawn();
        let place = place_of(&drawn.panels, panel);
        if place.open {
            self.close_panel(panel);
            return;
        }
        let request = WorkspaceRequest::PanelOpened { panel };
        self.port.send(request, None);
    }

    pub(crate) fn close_panel(&self, panel: Panel) {
        let request = WorkspaceRequest::PanelClosed { panel };
        self.port.send(request, None);
    }

    pub(crate) fn move_panel(&mut self, panel: Panel, edge: Edge, cx: &mut Context<Self>) {
        self.panel_dragging = false;
        cx.notify();
        let request = WorkspaceRequest::PanelMoved { panel, edge };
        self.port.send(request, None);
    }

    pub(crate) fn open_settings(&self) {
        let request = WorkspaceRequest::OpenTerminal {
            file: Some(ConfigFile::Settings),
        };
        self.port.send(request, None);
    }

    fn resize_to(
        &mut self,
        drag: &ResizeDrag,
        pointer: Point<Pixels>,
        window: Size<Pixels>,
    ) -> bool {
        let size = size_for_pointer(drag.edge, pointer, window);
        let live = Some((drag.panel, size));
        if self.resizing == live {
            return false;
        }
        self.resizing = live;
        true
    }

    // The size goes to the workspace once, on release, so a resize is one save and one terminal resize.
    fn finish_resize(&mut self, cx: &mut Context<Self>) {
        let Some((panel, size)) = self.resizing.take() else {
            return;
        };
        cx.notify();
        let request = WorkspaceRequest::PanelResized { panel, size };
        self.port.send(request, None);
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

        let side_strip = strip(&drawn.panels, look, cx);
        let mut body = div().relative().flex().flex_1().min_h_0().child(side_strip);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Left) {
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        body = body.child(center);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Right) {
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        if self.panel_dragging {
            body = body.child(drop_bands(look, cx));
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
        let mut size = place.size;
        if let Some((resized, live)) = self.resizing
            && resized == panel
        {
            size = live;
        }
        match panel {
            Panel::Terminals => {
                let input = ListInput { drawn, mark, look };
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
                    log: &self.activity,
                    expanded: &self.expanded,
                    now_ms: now_ms(),
                    look,
                };
                let contents = activity_panel(&input, cx);
                docked(panel, edge, size, contents, look, cx)
            }
        }
    }

    // The selected terminal, with the banners as cards over it so they never change its size.
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
        if !banners.is_empty() {
            terminal = terminal.child(banner_cards(banners, look));
        }
        terminal = mark(terminal, MIDDLE_MARK);
        div().flex().flex_col().flex_1().min_h_0().child(terminal)
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
        // Rendering is the one place the drawn selection is final, whether it came from the workspace or a move ahead of it.
        self.watch_activity(&drawn);
        // Once no drag is under way, there is nothing to mark and no panel on the move.
        if !cx.has_active_drag() {
            self.drop_place = None;
            self.panel_dragging = false;
            self.resizing = None;
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
        // The field takes the keyboard on the first frame it is drawn in, which only rendering has the window for.
        if let Some(Popup::Group {
            field, needs_focus, ..
        }) = &mut self.popup
            && *needs_focus
        {
            let shown_field = field.read(cx);
            let handle = shown_field.focus_handle(cx);
            window.focus(&handle, cx);
            *needs_focus = false;
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
            .on_drag_move(cx.listener(|view, _: &DragMoveEvent<PanelDrag>, _, cx| {
                if !view.panel_dragging {
                    view.panel_dragging = true;
                    cx.notify();
                }
            }))
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<ResizeDrag>, _, cx| {
                    let drag = *event.drag(cx);
                    let window_size = event.bounds.size;
                    let pointer = event.event.position;
                    if view.resize_to(&drag, pointer, window_size) {
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|view, _: &ResizeDrag, _, cx| view.finish_resize(cx)))
            .child(bar)
            .child(body);
        if let Some(popup) = &self.popup {
            root = root.child(popup_layer(popup, &drawn, look, cx));
        }
        root
    }
}
