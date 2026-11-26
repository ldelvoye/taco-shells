use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui::{
    AnyElement, App, AppContext, Bounds, ClipboardItem, Context, Div, DragMoveEvent, Entity,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Pixels,
    Point, Render, Size, Styled, Subscription, Task, Window, div, point, px, size,
};
use taco_protocol::{
    ActivityRequest, ConfigFile, Dragged, DrawnState, DropPlace, Edge, FocusMove, HarnessId, Panel,
    PanelPlace, RequestReply, TabGroupId, TerminalId, WorkspaceRequest,
};
use taco_rules::{DropMark, DropPoint, drop_for, drop_mark_for, focus_target, selected_terminal};
use taco_services::{AgentMenuAction, AgentSetupView};
use taco_settings::{
    Binding, CommandId, Config, SettingChange, Settings, UnreadSettings, command_for, palette_for,
};
use taco_terminal_view::{Terminals, TerminalsEvent};

use crate::activity::{ActivityInput, ActivityLog, activity_panel, now_ms};
use crate::banners::{Dismissal, Notices, banner_cards, banners_of};
use crate::done_checks::DoneChecks;
use crate::drive::mark;
use crate::keys::chord_of;
use crate::link::{
    Ahead, Ports, ServicesOutput, ServicesPort, WindowOutput, WorkspacePort, drawn_with, next_move,
};
use crate::look::{FONT_FAMILY, Look, ROW_HEIGHT, STRIP_WIDTH, TEXT_SIZE, TITLE_BAR_HEIGHT};
use crate::menu::{
    AgentMenuClicked, Minimize, Quit, RestartToUpdate, ShowHookScripts, Zoom, set_menus,
};
use crate::middle::{Attachable, attachable, line_editing_bytes, opens_link};
use crate::panels::{
    PanelDrag, ResizeDrag, bounded_size, docked, edge_near, landing_preview, open_on, place_of,
    size_for_pointer,
};
use crate::platform::TrafficLights;
use crate::popups::{Popup, RenameStep, popup_layer, rename_step};
use crate::settings_page::{
    OverPage, Page, SettingsFields, SettingsInput, Typed, TypedSetting, over_page, published_text,
    settings_page, typed,
};
use crate::slot::{Slot, SlotLine, opened_height, slot_line_for};
use crate::strip::strip;
use crate::terminal_list::{ListDrag, ListInput, terminal_list};
use crate::text_field::{TextField, TextFieldEvent};
use crate::title_bar::title_bar;

const MIDDLE_MARK: &str = "middle";
const PLACEHOLDER_FONT: &str = "Menlo";
const PLACEHOLDER_TEXT_SIZE: f32 = 12.;
const CLOCK_TICK: Duration = Duration::from_secs(30);
const ESCAPE: &str = "escape";
const NOT_WHOLE: &str = "Scrollback must be a whole number.";
const TYPED_SETTINGS: [TypedSetting; 2] = [TypedSetting::Font, TypedSetting::Scrollback];
const TERMINAL_INSET_SIDE: f32 = 8.;
const TERMINAL_INSET_TOP: f32 = 6.;

/// What the window draws: the workspace's state, and what the services and the app put beside it.
#[derive(Clone, Debug)]
pub struct Shown {
    pub drawn: DrawnState,
    pub notices: Notices,
    /// Lines drawn in the middle in place of a terminal.
    pub placeholder_screen: Vec<String>,
    pub setups: AgentSetupView,
    pub bindings: Vec<Binding>,
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
    /// The log lines whose files are shown, by `seq`.
    expanded: HashSet<u64>,
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
    /// A page has just opened and takes the keyboard on the next frame.
    page_wants_keyboard: bool,
    /// The last page has just closed and the terminal on screen takes the keyboard back.
    terminal_wants_keyboard: bool,
    /// Why each setting's last write was refused, by its name in settings.json, until it is written again.
    refused: HashMap<&'static str, String>,
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
            expanded: HashSet::new(),
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
            page_wants_keyboard: false,
            terminal_wants_keyboard: false,
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
            // Through `Quit`, not `cx.quit()`, so everything behind the window is stopped in order first.
            WindowOutput::Emptied => cx.dispatch_action(&Quit),
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
                self.shown.settings = config.settings;
                self.shown.unread = config.unread_settings;
                notices.config_problems = config.problems;
                self.refresh_fields(cx);
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
        let opening_name = name.clone();
        let edits = cx.subscribe(&field, move |view, _, event: &TextFieldEvent, cx| {
            let step = rename_step(event, &opening_name);
            match step {
                RenameStep::Rename(text) => view.rename(&renamed, &text),
                RenameStep::Close => view.close_popup(cx),
                RenameStep::RenameAndClose(text) => {
                    view.rename(&renamed, &text);
                    view.close_popup(cx);
                }
            }
        });
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

    /// The slot to draw this frame, opening from when it last moved; it opens on this clock rather than gpui's so the rows' hit test subtracts exactly the height drawn.
    fn open_slot(&mut self, line: Option<SlotLine>, window: &mut Window, cx: &App) -> Option<Slot> {
        let Some(line) = line else {
            self.slot_opened = None;
            return None;
        };
        let now = Instant::now();
        let mut opened = now;
        if let Some((open_line, since)) = self.slot_opened
            && open_line == line
        {
            opened = since;
        }
        self.slot_opened = Some((line, opened));
        let mut height = ROW_HEIGHT;
        if !cx.reduce_motion() {
            height = opened_height(now - opened, ROW_HEIGHT);
        }
        if height < ROW_HEIGHT {
            window.request_animation_frame();
        }
        Some(Slot { line, height })
    }

    /// Notes the edge a dragged panel would go to with the pointer here; true when it changed.
    fn aim_panel(&mut self, panel: Panel, pointer: Point<Pixels>, window: Size<Pixels>) -> bool {
        let origin = point(px(STRIP_WIDTH), px(TITLE_BAR_HEIGHT));
        let width = window.width - px(STRIP_WIDTH);
        let height = window.height - px(TITLE_BAR_HEIGHT);
        let area = Bounds::new(origin, size(width, height));
        let edge = edge_near(pointer, area);
        let mut target = None;
        if let Some(edge) = edge {
            target = Some((panel, edge));
        }
        if target == self.panel_target {
            return false;
        }
        self.panel_target = target;
        true
    }

    fn drop_panel(&mut self, drag: &PanelDrag, cx: &mut Context<Self>) {
        let target = self.panel_target.take();
        cx.notify();
        let Some((panel, edge)) = target else {
            return;
        };
        if panel != drag.panel {
            return;
        }
        let request = WorkspaceRequest::PanelMoved { panel, edge };
        self.port.send(request, None);
    }

    fn open_file(&mut self, file: ConfigFile, cx: &mut Context<Self>) {
        self.close_pages(cx);
        let request = WorkspaceRequest::OpenTerminal { file: Some(file) };
        self.port.send(request, None);
    }

    pub(crate) fn open_settings_file(&mut self, cx: &mut Context<Self>) {
        self.open_file(ConfigFile::Settings, cx);
    }

    /// Opens the settings page, or closes it when it is the page on top.
    pub(crate) fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        if self.pages.last() == Some(&Page::Settings) {
            self.close_page(cx);
            return;
        }
        self.pages.push(Page::Settings);
        self.page_wants_keyboard = true;
        cx.notify();
    }

    pub(crate) fn close_page(&mut self, cx: &mut Context<Self>) {
        if self.pages.pop().is_none() {
            return;
        }
        self.page_wants_keyboard = !self.pages.is_empty();
        self.terminal_wants_keyboard = self.pages.is_empty();
        cx.notify();
    }

    /// Closes every page, for an action that means to show a terminal.
    fn close_pages(&mut self, cx: &mut Context<Self>) {
        if self.pages.is_empty() {
            return;
        }
        self.pages.clear();
        self.terminal_wants_keyboard = true;
        cx.notify();
    }

    pub(crate) fn change_setting(&mut self, change: SettingChange) {
        let key = change.key();
        self.refused.remove(key);
        self.services.write_setting(change);
    }

    fn field(&self, setting: TypedSetting) -> &Entity<TextField> {
        match setting {
            TypedSetting::Font => &self.fields.font,
            TypedSetting::Scrollback => &self.fields.scrollback,
        }
    }

    fn field_said(
        &mut self,
        setting: TypedSetting,
        event: &TextFieldEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextFieldEvent::Edited(_) => {}
            TextFieldEvent::Submitted => {
                self.send_typed(setting, cx);
            }
            TextFieldEvent::Cancelled => {
                // The field gives the keyboard to the page, so the blur that follows finds nothing new to write and a second Esc closes the page.
                self.show_published(setting, cx);
                self.page_wants_keyboard = true;
                cx.notify();
            }
        }
    }

    /// Writes the field's value on losing focus, as Enter would.
    fn left_field(&mut self, setting: TypedSetting, cx: &mut Context<Self>) {
        if self.editing == Some(setting) {
            self.editing = None;
        }
        let sent = self.send_typed(setting, cx);
        if !sent {
            self.show_published(setting, cx);
        }
    }

    /// Writes what the field holds when it is a new, valid value; says whether it did.
    fn send_typed(&mut self, setting: TypedSetting, cx: &mut Context<Self>) -> bool {
        let text = self.field(setting).read(cx).text().to_string();
        let key = setting.key();
        let result = typed(setting, &text, &self.shown.settings);
        cx.notify();
        match result {
            Typed::Change(change) => {
                self.change_setting(change);
                true
            }
            Typed::Unchanged => {
                self.refused.remove(key);
                false
            }
            Typed::NotWhole => {
                self.refused.insert(key, NOT_WHOLE.to_string());
                false
            }
        }
    }

    fn show_published(&self, setting: TypedSetting, cx: &mut Context<Self>) {
        let text = published_text(setting, &self.shown.settings);
        let field = self.field(setting);
        field.update(cx, |field, cx| field.set_text(&text, cx));
    }

    /// Shows the published values and the look in every field but the one being typed in.
    fn refresh_fields(&self, cx: &mut Context<Self>) {
        for setting in TYPED_SETTINGS {
            let look = self.look.clone();
            let field = self.field(setting);
            field.update(cx, |field, cx| field.set_look(look, cx));
            if self.editing != Some(setting) {
                self.show_published(setting, cx);
            }
        }
    }

    fn watch_field_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.field_focus.is_empty() {
            return;
        }
        for setting in TYPED_SETTINGS {
            let handle = self.field(setting).focus_handle(cx);
            let entered = cx.on_focus(&handle, window, move |view, _, _| {
                view.editing = Some(setting);
            });
            let left = cx.on_blur(&handle, window, move |view, _, cx| {
                view.left_field(setting, cx);
            });
            self.field_focus.push(entered);
            self.field_focus.push(left);
        }
    }

    /// Runs the command a key-down's chord is bound to; false when the window has nothing to run for it, so the key goes on to the terminal.
    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        // The name field keeps every key while it is open.
        if self.popup.is_some() {
            return false;
        }
        let page_open = !self.pages.is_empty();
        let escaped = event.keystroke.key == ESCAPE && !event.keystroke.modifiers.modified();
        if page_open && escaped {
            self.close_page(cx);
            return true;
        }
        let chord = chord_of(&event.keystroke);
        let Some(command) = command_for(&self.shown.bindings, &chord) else {
            return false;
        };
        if page_open {
            return self.run_over_page(command, event.is_held, cx);
        }
        if !self.runs(command) {
            return false;
        }
        // Moving a word at a time repeats while held, as in any terminal; every other command runs once.
        let repeats = matches!(
            command,
            CommandId::TerminalWordLeft | CommandId::TerminalWordRight
        );
        if !event.is_held || repeats {
            self.run(command, cx);
        }
        true
    }

    fn run_over_page(&mut self, command: CommandId, held: bool, cx: &mut Context<Self>) -> bool {
        if held {
            return false;
        }
        match over_page(command) {
            OverPage::ClosePage => self.close_page(cx),
            OverPage::ToggleSettings => self.toggle_settings(cx),
            OverPage::ShowTerminal => {
                self.close_pages(cx);
                self.run(command, cx);
            }
            OverPage::Run => self.run(command, cx),
            OverPage::PassOn => return false,
        }
        true
    }

    fn runs(&self, command: CommandId) -> bool {
        let window_command = matches!(
            command,
            CommandId::TerminalNew
                | CommandId::TerminalClose
                | CommandId::TerminalFocusNextGroup
                | CommandId::TerminalFocusPreviousGroup
                | CommandId::SidebarToggle
                | CommandId::ConfigOpenSettings
                | CommandId::ConfigOpenKeybindings
        );
        if window_command {
            return true;
        }
        // The terminal's own commands need a terminal on screen to act on.
        self.attached.on_screen.is_some()
    }

    fn run_in_terminal(&mut self, command: CommandId, cx: &mut Context<Self>) {
        let Some(terminals) = self.terminals.clone() else {
            return;
        };
        let Some(terminal) = self.attached.on_screen.clone() else {
            return;
        };
        match command {
            CommandId::TerminalCopy => {
                let selection = terminals.update(cx, |view, _| view.selection(&terminal));
                if let Some(text) = selection {
                    let copied = ClipboardItem::new_string(text);
                    cx.write_to_clipboard(copied);
                }
            }
            CommandId::TerminalPaste => {
                let Some(clipboard) = cx.read_from_clipboard() else {
                    return;
                };
                let Some(text) = clipboard.text() else {
                    return;
                };
                terminals.update(cx, |view, cx| view.paste(&terminal, &text, cx));
            }
            CommandId::TerminalClear => terminals.update(cx, |view, _| view.clear(&terminal)),
            _ => {
                let Some(bytes) = line_editing_bytes(command) else {
                    return;
                };
                terminals.update(cx, |view, _| view.type_bytes(&terminal, bytes));
            }
        }
    }

    fn run(&mut self, command: CommandId, cx: &mut Context<Self>) {
        match command {
            CommandId::TerminalNew => {
                let request = WorkspaceRequest::OpenTerminal { file: None };
                self.port.send(request, None);
            }
            CommandId::TerminalClose => {
                let drawn = self.drawn();
                let selected = selected_terminal(drawn.arrangement.as_ref());
                if let Some(terminal) = selected {
                    self.close(terminal);
                }
            }
            CommandId::TerminalFocusNextGroup => self.move_focus(FocusMove::NextRow, cx),
            CommandId::TerminalFocusPreviousGroup => self.move_focus(FocusMove::PreviousRow, cx),
            CommandId::SidebarToggle => self.toggle_panel(Panel::Terminals),
            CommandId::ConfigOpenSettings => self.toggle_settings(cx),
            CommandId::ConfigOpenKeybindings => self.open_file(ConfigFile::Keybindings, cx),
            _ => self.run_in_terminal(command, cx),
        }
    }

    fn move_focus(&mut self, focus_move: FocusMove, cx: &mut Context<Self>) {
        let drawn = self.drawn();
        let Some(arrangement) = &drawn.arrangement else {
            return;
        };
        let Some(target) = focus_target(arrangement, focus_move) else {
            return;
        };
        self.select(&target, cx);
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
        let middle = self.middle(drawn, cx);
        let mut center = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(middle);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Bottom) {
            center = center.child(self.panel(panel, place, drawn, mark, cx));
        }

        let settings_open = self.pages.contains(&Page::Settings);
        let side_strip = strip(&drawn.panels, settings_open, look, cx);
        let mut body = div().relative().flex().flex_1().min_h_0().child(side_strip);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Left) {
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        body = body.child(center);
        if let Some((panel, place)) = open_on(&drawn.panels, Edge::Right) {
            body = body.child(self.panel(panel, place, drawn, mark, cx));
        }
        if let Some((panel, edge)) = self.panel_target {
            let place = place_of(&drawn.panels, panel);
            let size = bounded_size(edge, place.size, self.viewport);
            let preview = landing_preview(panel, edge, size, look);
            body = body.child(preview);
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
        if let Some((resized, live)) = self.resizing
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
                    slot: self.slot,
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
    fn middle(&self, drawn: &DrawnState, cx: &mut Context<Self>) -> Div {
        let look = &self.look;
        let workspace_problem = drawn.problem.as_deref();
        let page = self.pages.last().copied();
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
        match page {
            Some(Page::Settings) => {
                let input = SettingsInput {
                    settings: &self.shown.settings,
                    problems: &self.shown.notices.config_problems,
                    unread: &self.shown.unread,
                    refused: &self.refused,
                    fields: &self.fields,
                    look,
                };
                let shown_page = settings_page(&input, cx).track_focus(&self.page_focus);
                terminal = terminal.child(shown_page);
            }
            None => terminal = terminal.child(self.terminal_or_placeholder()),
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

    /// Tells the terminal view what changed, and keeps the keyboard in the terminal on screen.
    fn show_terminals(&mut self, drawn: &DrawnState, window: &mut Window, cx: &mut Context<Self>) {
        let attached = attachable(drawn);
        let refocused = drawn.refocus != self.refocus;
        self.refocus = drawn.refocus;
        let switched = attached.on_screen != self.attached.on_screen || refocused;
        if let Some(terminals) = &self.terminals
            && attached != self.attached
        {
            // Only on a change: showing redraws the view, which would redraw the window again.
            terminals.update(cx, |view, cx| {
                view.show(&attached.open, attached.on_screen.as_ref(), cx);
            });
        }
        self.attached = attached;
        // A key pressed with nothing focused reaches no listener, so the keyboard goes back to the terminal, or the window.
        let focused = window.focused(cx);
        let unfocused = focused.is_none();
        // A page takes the keyboard on the frame after it opens, or whenever nothing holds it.
        if !self.pages.is_empty() {
            if self.page_wants_keyboard || unfocused {
                self.page_wants_keyboard = false;
                window.focus(&self.page_focus, cx);
            }
            return;
        }
        let page_closed = self.terminal_wants_keyboard;
        self.terminal_wants_keyboard = false;
        if !switched && !unfocused && !page_closed {
            return;
        }
        let on_screen = self.attached.on_screen.clone();
        if let (Some(terminals), Some(terminal)) = (&self.terminals, &on_screen)
            && self.popup.is_none()
        {
            terminals.update(cx, |view, cx| view.focus(terminal, window, cx));
            return;
        }
        if unfocused {
            window.focus(&self.focus, cx);
        }
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
        self.watch_field_focus(window, cx);
        self.viewport = window.viewport_size();
        let (drawn, kept) = drawn_with(&self.shown.drawn, self.ahead.as_ref());
        self.ahead = kept;
        // Rendering is the one place the drawn selection is final, whether it came from the workspace or a move ahead of it.
        self.watch_activity(&drawn);
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

fn settings_fields(
    settings: &Settings,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> SettingsFields {
    let font = typed_field_for(TypedSetting::Font, settings, look, cx);
    let scrollback = typed_field_for(TypedSetting::Scrollback, settings, look, cx);
    SettingsFields { font, scrollback }
}

fn typed_field_for(
    setting: TypedSetting,
    settings: &Settings,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Entity<TextField> {
    let text = published_text(setting, settings);
    let look = look.clone();
    let field = cx.new(|cx| TextField::new(&text, look, cx));
    let heard = cx.subscribe(&field, move |view, _, event: &TextFieldEvent, cx| {
        view.field_said(setting, event, cx);
    });
    heard.detach();
    field
}
