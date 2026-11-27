use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{
    Context, DragMoveEvent, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Pixels, Point, Render, Size, Styled, Subscription, Task, Window,
    div, px,
};
use taco_protocol::{
    ActivityEvent, DrawnState, DropPlace, Edge, Panel, TabGroupId, TerminalId, WorkspaceRequest,
};
use taco_rules::{drop_mark_for, selected_terminal};
use taco_services::AgentSetupView;
use taco_settings::{Binding, CommandId, Config, Settings, UnreadSettings, palette_for};
use taco_terminal_view::{Terminals, TerminalsEvent};

use crate::activity_log::ActivityLog;
use crate::banners::{Dismissal, Notices};
use crate::done_checks::DoneChecks;
use crate::link::{
    Ahead, Ports, ServicesOutput, ServicesPort, WindowOutput, WorkspacePort, drawn_with, next_move,
};
use crate::look::{FONT_FAMILY, Look, TEXT_SIZE};
use crate::menu::{
    AgentMenuClicked, Minimize, Quit, RestartToUpdate, ShowHookScripts, Zoom, set_menus,
};
use crate::middle::{Attachable, opens_link};
use crate::motion::{DrawnBounds, Moved, PAGE_TIME, moved_settings, progress};
use crate::page::Page;
use crate::panels::{PanelDrag, ResizeDrag};
use crate::platform::TrafficLights;
use crate::popups::{Popup, popup_layer};
use crate::settings_page::{SettingsFields, TypedSetting};
use crate::shared_view::{OpenedThing, body_content};
use crate::shortcuts_page::Capture;
use crate::slot::{Slot, SlotLine, slot_line_for};
use crate::text_field::TextField;
use crate::title_bar::title_bar;

mod activity_state;
mod arrangement;
mod keyboard;
mod layout;
mod pages;
mod settings_state;
mod shortcuts_state;

use settings_state::settings_fields;
use shortcuts_state::search_field;

const CLOCK_TICK: Duration = Duration::from_secs(30);

/// What the window draws: the workspace's state, and what the services and the app put beside it.
#[derive(Clone, Debug)]
pub struct Shown {
    pub drawn: DrawnState,
    pub notices: Notices,
    /// Lines drawn in the middle in place of a terminal.
    pub placeholder_screen: Vec<String>,
    pub setups: AgentSetupView,
    pub bindings: Vec<Binding>,
    /// The shipped bindings alone, which "Back to default" returns a command to.
    pub default_bindings: Vec<Binding>,
    /// keybindings.json could not be read, so the shortcuts page cannot change it.
    pub unread_keybindings: bool,
    pub settings: Settings,
    pub unread: UnreadSettings,
}

/// The one window's root view.
pub struct WindowView {
    shown: Shown,
    look: Look,
    fields: SettingsFields,
    /// The typed setting whose field has the keyboard; a new config leaves its text alone.
    editing: Option<TypedSetting>,
    /// Watches the fields' focus, set up on the first frame since it needs the window.
    field_focus: Vec<Subscription>,
    traffic_lights: TrafficLights,
    port: Rc<dyn WorkspacePort>,
    services: Rc<dyn ServicesPort>,
    config_changed: Box<dyn Fn(&Config)>,
    activity: ActivityLog,
    /// The stretch numbers whose steps are shown.
    folds_open: HashSet<u64>,
    /// The `seq` of each long text in the log shown whole.
    texts_open: HashSet<u64>,
    /// Whether Shared lists every thing rather than the newest few.
    shared_all_shown: bool,
    ahead: Option<Ahead>,
    /// The number of this window's last focus move.
    last_move: u64,
    drop_place: Option<DropPlace>,
    popup: Option<Popup>,
    /// A panel being resized, and the size it is drawn at until the release sends it.
    resizing: Option<(Panel, f32)>,
    /// A panel being moved by its title, and the edge it would go to with the pointer where it is.
    panel_target: Option<(Panel, Edge)>,
    last_token: u64,
    awaiting: HashMap<u64, Awaited>,
    _following: Task<()>,
    _clock: Task<()>,
    /// The window's own focus, which keys reach when nothing inside it holds the keyboard.
    focus: FocusHandle,
    dismissed: HashSet<Dismissal>,
    /// The runtime's terminals, drawn in the middle; none while the window runs on fixtures.
    terminals: Option<Entity<Terminals>>,
    /// What the terminal view was last told to show.
    attached: Attachable,
    /// The workspace's last request to give the keyboard back to the selected terminal.
    refocus: u64,
    done_checks: DoneChecks,
    home: Option<PathBuf>,
    /// The window's size as of this frame, which bounds the panels.
    viewport: Size<Pixels>,
    /// Where the landing slot is and when it started opening there.
    slot_opened: Option<(SlotLine, Instant)>,
    /// The slot as drawn this frame.
    slot: Option<Slot>,
    /// The pages over the middle, the top one drawn; empty when the terminal shows.
    pages: Vec<Page>,
    page_focus: FocusHandle,
    /// What on the page takes the keyboard on the next frame, after a page opens or a field lets go.
    page_wants_keyboard: Option<PageKeyboard>,
    shortcut_search: Entity<TextField>,
    shortcut_query: String,
    /// A shortcut being set on the shortcuts page; while it is open, every key goes to it.
    capture: Option<Capture>,
    /// Why each command's last shortcut change was not written, until it is changed again.
    keymap_refused: HashMap<CommandId, String>,
    /// The switches and segments a config changed, each with the value it is moving from.
    moved: HashMap<&'static str, Moved>,
    drawn_bounds: DrawnBounds,
    /// When the page on top came up, so it fades in.
    page_since: Option<Instant>,
    /// The last page to close, fading out over the terminal.
    leaving: Option<(Page, Instant)>,
    /// The last page has just closed and the terminal on screen takes the keyboard back.
    terminal_wants_keyboard: bool,
    /// A shared thing shown in the middle in place of the terminal, under any page.
    opened: Option<OpenedThing>,
    opened_focus: FocusHandle,
    /// The thing has just opened and takes the keyboard on the next frame.
    opened_wants_keyboard: bool,
    /// Why each setting's last write was refused, by its name in settings.json, until it is written again.
    refused: HashMap<&'static str, String>,
}

/// What the window does with a reply it asked for.
enum Awaited {
    /// Open the new group's popup where the row's popup was.
    NewGroup { at: Point<Pixels> },
}

/// Where the keyboard goes on a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageKeyboard {
    Page,
    Search,
}

impl WindowView {
    pub fn new(
        shown: Shown,
        look: Look,
        traffic_lights: TrafficLights,
        ports: Ports,
        cx: &mut Context<Self>,
    ) -> Self {
        let last_move = shown.drawn.focus_count;
        let refocus = shown.drawn.refocus;
        set_menus(&shown, cx);
        let Ports {
            workspace: port,
            services,
            outputs,
            config_changed,
            terminals,
        } = ports;
        if let Some(terminals) = &terminals {
            let heard = cx.subscribe(terminals, |view, _, event, cx| {
                view.terminals_said(event, cx)
            });
            heard.detach();
        }
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
        let fields = settings_fields(&shown.settings, &look, cx);
        let shortcut_search = search_field(&look, cx);
        Self {
            shown,
            look,
            fields,
            editing: None,
            field_focus: Vec::new(),
            traffic_lights,
            port,
            services,
            config_changed,
            activity: ActivityLog::default(),
            folds_open: HashSet::new(),
            texts_open: HashSet::new(),
            shared_all_shown: false,
            ahead: None,
            last_move,
            drop_place: None,
            popup: None,
            resizing: None,
            panel_target: None,
            last_token: 0,
            awaiting: HashMap::new(),
            _following: following,
            _clock: clock,
            focus: cx.focus_handle(),
            dismissed: HashSet::new(),
            terminals,
            attached: Attachable::default(),
            refocus,
            done_checks: DoneChecks::default(),
            home: std::env::home_dir(),
            viewport: Size::default(),
            slot_opened: None,
            slot: None,
            pages: Vec::new(),
            page_focus: cx.focus_handle(),
            page_wants_keyboard: None,
            shortcut_search,
            shortcut_query: String::new(),
            capture: None,
            keymap_refused: HashMap::new(),
            moved: HashMap::new(),
            drawn_bounds: DrawnBounds::default(),
            page_since: None,
            leaving: None,
            terminal_wants_keyboard: false,
            opened: None,
            opened_focus: cx.focus_handle(),
            opened_wants_keyboard: false,
            refused: HashMap::new(),
        }
    }

    #[cfg(feature = "drive")]
    pub fn set_look(&mut self, look: Look, cx: &mut Context<Self>) {
        self.look = look;
        self.refresh_fields(cx);
    }

    /// The terminal view in the middle, for the driver to read; none on fixtures.
    #[cfg(feature = "drive")]
    pub(crate) fn terminals(&self) -> Option<Entity<Terminals>> {
        self.terminals.clone()
    }

    fn receive(&mut self, output: WindowOutput, cx: &mut Context<Self>) {
        match output {
            WindowOutput::State(state) => self.show(state, cx),
            WindowOutput::Replied { token, reply } => self.replied(token, reply, cx),
            WindowOutput::Activity(ActivityEvent::Body {
                terminal,
                thing,
                body,
            }) => {
                let content = body_content(body);
                self.fill_opened(&terminal, thing, content, cx);
            }
            WindowOutput::Activity(event) => {
                self.activity.apply(event);
                cx.notify();
            }
            WindowOutput::Services(output) => self.services_said(output, cx),
        }
    }

    fn services_said(&mut self, output: ServicesOutput, cx: &mut Context<Self>) {
        let notices = &mut self.shown.notices;
        match output {
            ServicesOutput::Setups(setups) => {
                self.shown.setups = setups;
                set_menus(&self.shown, cx);
            }
            ServicesOutput::Config(config) => {
                (self.config_changed)(&config);
                if let Some(terminals) = &self.terminals {
                    let palette = palette_for(config.appearance);
                    terminals.update(cx, |view, cx| {
                        view.apply(&config.settings, &palette.terminal, cx);
                    });
                }
                self.look = Look::new(config.appearance);
                self.shown.bindings = config.bindings;
                self.shown.default_bindings = config.default_bindings;
                self.shown.unread_keybindings = config.unread_keybindings;
                let moved = moved_settings(&self.shown.settings, &config.settings);
                self.shown.settings = config.settings;
                self.shown.unread = config.unread_settings;
                notices.config_problems = config.problems;
                let now = Instant::now();
                let reduce_motion = cx.reduce_motion();
                for from in moved {
                    let started = self.move_from(from, now, reduce_motion);
                    let key = started.from.key();
                    self.moved.insert(key, started);
                }
                self.refresh_fields(cx);
                self.close_stale_capture();
            }
            ServicesOutput::UpdateReady { version } => {
                notices.update = Some(version);
                notices.update_problem = None;
                set_menus(&self.shown, cx);
            }
            ServicesOutput::UpdateProblem(problem) => notices.update_problem = Some(problem),
            ServicesOutput::Problem(problem) => {
                if !notices.problems.contains(&problem) {
                    notices.problems.push(problem);
                }
            }
            ServicesOutput::QuitForUpdate => cx.dispatch_action(&Quit),
            ServicesOutput::SettingRefused { key, reason } => {
                self.refused.insert(key, reason);
                self.refresh_fields(cx);
            }
            ServicesOutput::KeymapRefused { command, reason } => {
                self.keymap_refused.insert(command, reason);
            }
        }
        cx.notify();
    }

    pub(crate) fn apply_update(&self) {
        self.services.apply_update();
    }

    pub(crate) fn dismiss_banner(&mut self, dismissal: Dismissal, cx: &mut Context<Self>) {
        self.dismissed.insert(dismissal);
        cx.notify();
    }

    fn terminals_said(&mut self, event: &TerminalsEvent, cx: &mut Context<Self>) {
        match event {
            // A click put the keyboard in the terminal on screen: the selection it already shows, reported as a focus move.
            TerminalsEvent::Focused(terminal) => self.select(terminal, cx),
            TerminalsEvent::LinkClicked(url) => {
                if opens_link(url) {
                    cx.open_url(url);
                }
            }
        }
    }

    fn show_hook_scripts(&self, cx: &mut Context<Self>) {
        let directory = self.services.hooks_directory();
        cx.reveal_path(&directory);
    }

    fn agent_menu_clicked(&self, clicked: &AgentMenuClicked) {
        self.services.agent_action(&clicked.harness, clicked.action);
    }

    fn show(&mut self, state: DrawnState, cx: &mut Context<Self>) {
        if state.revision <= self.shown.drawn.revision {
            return;
        }
        let now = Instant::now();
        self.done_checks.note(&self.shown.drawn, &state, now);
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
        self.close_pages(cx);
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

    pub(crate) fn open_terminal_in(
        &mut self,
        tab_group: Option<TabGroupId>,
        cx: &mut Context<Self>,
    ) {
        self.close_pages(cx);
        let request = WorkspaceRequest::OpenTerminalIn { tab_group };
        self.port.send(request, None);
    }
}

impl Render for WindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.watch_field_focus(window, cx);
        let now = Instant::now();
        let reduce_motion = cx.reduce_motion();
        if let Some((_, since)) = self.leaving
            && progress(since, now, PAGE_TIME, reduce_motion) >= 1.
        {
            self.leaving = None;
        }
        if self.pages_moving(now, reduce_motion) {
            window.request_animation_frame();
        }
        self.viewport = window.viewport_size();
        let (drawn, kept) = drawn_with(&self.shown.drawn, self.ahead.as_ref());
        self.ahead = kept;
        // Rendering is the one place the drawn selection is final, whether it came from the workspace or a move ahead of it.
        self.watch_activity(&drawn);
        self.close_opened_elsewhere(&drawn);
        // Once no drag is under way, there is nothing to mark and no panel on the move.
        if !cx.has_active_drag() {
            self.drop_place = None;
            self.panel_target = None;
            self.resizing = None;
        }
        let mut mark = None;
        if let (Some(arrangement), Some(place)) = (&drawn.arrangement, &self.drop_place) {
            mark = drop_mark_for(arrangement, place);
        }
        let mut line = None;
        if let (Some(arrangement), Some(mark)) = (&drawn.arrangement, &mark) {
            line = slot_line_for(arrangement, mark);
        }
        self.slot = self.open_slot(line, window, cx);

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

        self.show_terminals(&drawn, window, cx);
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
            .track_focus(&self.focus)
            .on_action(|_: &Minimize, window, _| window.minimize_window())
            .on_action(|_: &Zoom, window, _| window.zoom_window())
            .on_action(cx.listener(|view, _: &RestartToUpdate, _, _| view.apply_update()))
            .on_action(cx.listener(|view, _: &ShowHookScripts, _, cx| {
                view.show_hook_scripts(cx);
            }))
            .on_action(cx.listener(|view, clicked: &AgentMenuClicked, _, _| {
                view.agent_menu_clicked(clicked);
            }))
            .capture_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                let ran = view.key_down(event, cx);
                if ran {
                    cx.stop_propagation();
                }
            }))
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<PanelDrag>, _, cx| {
                    let panel = event.drag(cx).panel;
                    let pointer = event.event.position;
                    let window_size = event.bounds.size;
                    if view.aim_panel(panel, pointer, window_size) {
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|view, drag: &PanelDrag, _, cx| view.drop_panel(drag, cx)))
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
