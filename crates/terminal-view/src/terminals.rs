//! The terminals in the middle of the window: the views kept, the one on screen drawn, and what the person does to it.

use std::io;
use std::ops::Range;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::Receiver;
use gpui::{
    App, Bounds, Context, ElementInputHandler, EntityInputHandler, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, ScrollDelta,
    ScrollEvent, Styled as _, Task, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window,
    canvas, div, fill, font, point, px, size,
};
use taco_ghostty::input::{
    CellPoint, Grid, Key, KeyEncoder, KeyPress, Mods, MouseAction, MouseButton as GridButton,
    MouseInput, Scroll,
};
use taco_protocol::{TerminalColors, TerminalEvent, TerminalId, TerminalSize};
use taco_settings::Settings;

use crate::connection::{Requests, connect};
use crate::draw::{cursor_as_shown, hsla, paint};
use crate::grid::{CellMetrics, font_family, grid_that_fits};
use crate::keys::key_press;
use crate::view::{Drawn, ViewConfig};
use crate::views::{Fit, Views};

/// The width of `m` as a share of the font size, for a font that cannot be measured.
const FALLBACK_ADVANCE: f32 = 0.6;
const MARKED_TEXT_UNDERLINE: f32 = 1.0;
/// How long a blinking cursor stays on, then off.
const BLINK_PHASE: Duration = Duration::from_millis(530);

pub enum TerminalsEvent {
    /// A click put keyboard focus in this terminal. Not sent for a focus the app asked for.
    Focused(TerminalId),
    /// A link was clicked with Cmd held. The app decides which schemes may open.
    LinkClicked(String),
}

/// The views and the terminal channel. The app makes one when it starts the runtime and places it in the middle.
pub struct Terminals {
    requests: Requests,
    views: Views,
    keys: KeyEncoder,
    focus: FocusHandle,
    /// The focus last reported to a program, so each change is reported once and a terminal switched away from hears it lost focus.
    reported_focus: Option<(TerminalId, bool)>,
    /// What an input method is composing, drawn at the cursor until it is committed.
    marked_text: Option<String>,
    /// Where the grid was last drawn, in window pixels, and how many cells it holds.
    grid_bounds: Option<Bounds<Pixels>>,
    grid_size: Option<TerminalSize>,
    cursor_cell: Option<(u16, u16)>,
    /// The cell a drag that selects began on.
    anchor: Option<CellPoint>,
    /// The left button is down after a press the view took, for selecting or for the program.
    button_held: bool,
    /// The press was reported to a program tracking the mouse, so its release is reported too.
    reported_press: bool,
    /// Wheel movement too small for a line yet.
    scroll_remainder: f32,
    scroll_sensitivity: f32,
    /// The window and this view both have keyboard focus, as of the last frame.
    focused: bool,
    /// Which phase a blinking cursor is in; typing starts it on again.
    blink_on: bool,
    /// The cursor last drawn may blink, so the blink timer redraws.
    cursor_blinks: bool,
    family: String,
    font_size: f32,
    cell: CellMetrics,
    config: ViewConfig,
    _events: Task<()>,
    _blink: Task<()>,
}

/// What one frame paints, worked out before painting.
struct Prepared {
    drawn: Drawn,
    marked_text: Option<String>,
    cell: CellMetrics,
    family: String,
    font_size: f32,
}

/// Where the pointer is: the cell under it, and its position in the grid's pixels.
#[derive(Clone, Copy)]
struct Pointer {
    cell: CellPoint,
    x: f32,
    y: f32,
}

/// The terminal channel, connected, from `Terminals::connect`.
pub struct TerminalChannel {
    requests: Requests,
    events: Receiver<TerminalEvent>,
    keys: KeyEncoder,
}

impl EventEmitter<TerminalsEvent> for Terminals {}

impl Terminals {
    /// The part of making `Terminals` that can fail, done before gpui's `cx.new`.
    pub fn connect(connection: UnixStream) -> io::Result<TerminalChannel> {
        let (requests, events) = connect(connection)?;
        let keys = KeyEncoder::new().map_err(io::Error::other)?;
        Ok(TerminalChannel {
            requests,
            events,
            keys,
        })
    }

    pub fn new(
        channel: TerminalChannel,
        settings: &Settings,
        colors: &TerminalColors,
        cx: &mut Context<Terminals>,
    ) -> Terminals {
        let TerminalChannel {
            requests,
            mut events,
            keys,
        } = channel;
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let received = this.update(cx, |terminals, cx| {
                    let redraw = terminals.views.receive(event);
                    terminals.send_requests();
                    if redraw {
                        cx.notify();
                    }
                });
                if received.is_err() {
                    break;
                }
            }
        });

        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_PHASE).await;
                let blinked = this.update(cx, |terminals, cx| {
                    if terminals.focused && terminals.cursor_blinks {
                        terminals.blink_on = !terminals.blink_on;
                        cx.notify();
                    }
                });
                if blinked.is_err() {
                    break;
                }
            }
        });

        let installed = cx.text_system().all_font_names();
        let family = font_family(&settings.font_family, &installed);
        let font_size = settings.font_size as f32;
        let cell = measure(&family, font_size, cx);
        let config = view_config(settings, colors);
        Terminals {
            requests,
            views: Views::new(config.clone(), cell.size_px()),
            keys,
            focus: cx.focus_handle(),
            reported_focus: None,
            marked_text: None,
            grid_bounds: None,
            grid_size: None,
            cursor_cell: None,
            anchor: None,
            button_held: false,
            reported_press: false,
            scroll_remainder: 0.0,
            scroll_sensitivity: settings.scroll_sensitivity as f32,
            focused: false,
            blink_on: true,
            cursor_blinks: false,
            family,
            font_size,
            cell,
            config,
            _events: task,
            _blink: blink,
        }
    }

    /// New settings or theme colors: the font, the scrollback, the wheel and every view's colors. The cursor's defaults are the runtime's.
    pub fn apply(
        &mut self,
        settings: &Settings,
        colors: &TerminalColors,
        cx: &mut Context<Terminals>,
    ) {
        let installed = cx.text_system().all_font_names();
        self.family = font_family(&settings.font_family, &installed);
        self.font_size = settings.font_size as f32;
        self.cell = measure(&self.family, self.font_size, cx);
        self.scroll_sensitivity = settings.scroll_sensitivity as f32;
        self.config = view_config(settings, colors);
        self.views.configure(self.config.clone());
        self.send_requests();
        cx.notify();
    }

    /// The open terminals and the one in the middle. Sent on every change to the arrangement; cheap when nothing changed.
    pub fn show(
        &mut self,
        open: &[TerminalId],
        on_screen: Option<&TerminalId>,
        cx: &mut Context<Terminals>,
    ) {
        let switched = self.views.on_screen() != on_screen;
        if switched {
            // A gesture or a composition belongs to the terminal it started on.
            self.anchor = None;
            self.marked_text = None;
            self.button_held = false;
            self.reported_press = false;
            self.scroll_remainder = 0.0;
        }
        self.views.show(open, on_screen);
        self.send_requests();
        cx.notify();
    }

    /// Moves keyboard focus to the terminal, when it is the one on screen. Not reported back as `Focused`.
    pub fn focus(&mut self, id: &TerminalId, window: &mut Window, cx: &mut Context<Terminals>) {
        if self.views.on_screen() != Some(id) {
            return;
        }
        window.focus(&self.focus, cx);
    }

    /// The text selected in the terminal, as it is copied.
    pub fn selection(&mut self, id: &TerminalId) -> Option<String> {
        let view = self.views.view(id)?;
        view.selection()
    }

    /// Pastes `text`, bracketed when the program asked for it, so a multi-line paste is not run line by line.
    pub fn paste(&mut self, id: &TerminalId, text: &str, cx: &mut Context<Terminals>) {
        let Some(view) = self.views.view(id) else {
            return;
        };
        if let Ok(bytes) = view.paste_bytes(text) {
            self.views.write(id.clone(), bytes);
        }
        self.send_requests();
        cx.notify();
    }

    /// Bytes as if typed: the line-editing commands come through here.
    pub fn type_bytes(&mut self, id: &TerminalId, bytes: &[u8]) {
        self.views.write(id.clone(), bytes.to_vec());
        self.send_requests();
    }

    /// Asks the runtime to clear the terminal; the view clears when `Cleared` comes back at its place in the output.
    pub fn clear(&mut self, id: &TerminalId) {
        self.views.clear(id.clone());
        self.send_requests();
    }

    fn send_requests(&mut self) {
        for request in self.views.take_requests() {
            self.requests.send(request);
        }
    }

    /// The frame for the terminal on screen, after the work every frame does first.
    fn before_paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> Option<Prepared> {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        let fits = grid_that_fits(width, height, self.cell);
        self.grid_bounds = Some(bounds);
        self.grid_size = Some(fits);
        self.views.fit(Fit {
            size: fits,
            cell: self.cell.size_px(),
        });
        let history_left = self.views.step_history();
        if history_left {
            window.request_animation_frame();
        }
        let focused = self.focus.is_focused(window) && window.is_window_active();
        if focused && !self.focused {
            self.blink_on = true;
        }
        self.focused = focused;
        self.report_focus(focused);
        let prepared = self.draw_on_screen();
        self.send_requests();
        prepared
    }

    /// Tells programs that asked (mode 1004) when the terminal on screen gains or loses focus, and the one switched away from that it lost it.
    fn report_focus(&mut self, focused: bool) {
        let Some(id) = self.views.on_screen().cloned() else {
            return;
        };
        let current = (id.clone(), focused);
        if self.reported_focus.as_ref() == Some(&current) {
            return;
        }
        let attached = self.views.view(&id).is_some_and(|view| view.is_attached());
        if !attached {
            return;
        }
        if let Some((previous, true)) = self.reported_focus.clone()
            && previous != id
        {
            self.send_focus(&previous, false);
        }
        self.send_focus(&id, focused);
        self.reported_focus = Some(current);
    }

    fn send_focus(&mut self, id: &TerminalId, focused: bool) {
        let Some(view) = self.views.view(id) else {
            return;
        };
        if let Ok(Some(report)) = view.focus_report(focused) {
            self.views.write(id.clone(), report);
        }
    }

    fn draw_on_screen(&mut self) -> Option<Prepared> {
        self.cursor_cell = None;
        let id = self.views.on_screen().cloned()?;
        let selection = self.config.colors.selection;
        let view = self.views.view(&id)?;
        let mut drawn = match view.draw(selection) {
            Ok(Some(drawn)) => drawn,
            Ok(None) => return None,
            Err(_) => {
                self.views.start_over(id);
                return None;
            }
        };
        let cursor = drawn.painting.cursor;
        if let Some(mark) = cursor {
            self.cursor_cell = Some((mark.column, mark.row));
            self.cursor_blinks = mark.blinking;
        }
        drawn.painting.cursor = cursor_as_shown(cursor, self.focused, self.blink_on);
        Some(Prepared {
            drawn,
            marked_text: self.marked_text.clone(),
            cell: self.cell,
            family: self.family.clone(),
            font_size: self.font_size,
        })
    }

    fn key_down(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        let Some(press) = key_press(&event.keystroke, event.is_held) else {
            return;
        };
        self.send_key(&press);
        cx.stop_propagation();
        cx.notify();
    }

    fn send_key(&mut self, press: &KeyPress) {
        self.blink_on = true;
        let Some(id) = self.views.on_screen().cloned() else {
            return;
        };
        let Some(view) = self.views.view(&id) else {
            return;
        };
        if let Ok(bytes) = view.key(&mut self.keys, press) {
            self.views.write(id, bytes);
        }
        self.send_requests();
    }

    /// Whether the program on screen tracks the mouse, so the pointer is reported to it instead of selecting.
    fn program_tracks_mouse(&mut self) -> bool {
        let Some(view) = self.views.on_screen_view() else {
            return false;
        };
        let tracking = view.tracks_mouse();
        matches!(tracking, Ok(true))
    }

    /// Reports a pointer event to the program on screen.
    fn send_mouse(
        &mut self,
        action: MouseAction,
        button: Option<GridButton>,
        held: bool,
        modifiers: Modifiers,
        pointer: Pointer,
    ) {
        let grid = self.grid();
        let Some(id) = self.views.on_screen().cloned() else {
            return;
        };
        let Some(view) = self.views.view(&id) else {
            return;
        };
        let input = MouseInput {
            action,
            button,
            mods: mods_of(modifiers),
            x: pointer.x,
            y: pointer.y,
            any_pressed: held,
            grid,
        };
        if let Ok(bytes) = view.mouse(&input) {
            self.views.write(id, bytes);
        }
        self.send_requests();
    }

    fn mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        let was_focused = self.focus.is_focused(window);
        window.focus(&self.focus, cx);
        let Some(id) = self.views.on_screen().cloned() else {
            return;
        };
        if !was_focused {
            cx.emit(TerminalsEvent::Focused(id.clone()));
        }
        let Some(pointer) = self.pointer_at(event.position) else {
            return;
        };

        if event.modifiers.platform {
            let Some(view) = self.views.view(&id) else {
                return;
            };
            if let Ok(Some(url)) = view.link_at(pointer.cell) {
                cx.emit(TerminalsEvent::LinkClicked(url));
            }
            return;
        }
        self.button_held = true;
        if self.program_tracks_mouse() {
            self.reported_press = true;
            self.send_mouse(
                MouseAction::Press,
                Some(GridButton::Left),
                true,
                event.modifiers,
                pointer,
            );
            return;
        }

        let Some(view) = self.views.view(&id) else {
            return;
        };
        let selected = match event.click_count {
            1 => {
                self.anchor = Some(pointer.cell);
                view.clear_selection()
            }
            2 => view.select_word(pointer.cell),
            _ => view.select_line(pointer.cell),
        };
        if selected.is_err() {
            self.views.start_over(id);
            self.send_requests();
        }
        cx.notify();
    }

    fn mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        let Some(pointer) = self.pointer_at(event.position) else {
            return;
        };
        let held = self.button_held && event.pressed_button == Some(MouseButton::Left);
        if self.program_tracks_mouse() {
            let mut button = None;
            if held {
                button = Some(GridButton::Left);
            }
            self.send_mouse(MouseAction::Motion, button, held, event.modifiers, pointer);
            return;
        }
        let Some(start) = self.anchor else {
            return;
        };
        if !held {
            return;
        }
        let Some(view) = self.views.on_screen_view() else {
            return;
        };
        let _ = view.select(start, pointer.cell);
        cx.notify();
    }

    fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) {
        self.release(event);
    }

    /// The end of a press, wherever the pointer is: a release outside the grid ends a drag all the same.
    fn release(&mut self, event: &MouseUpEvent) {
        let reported = self.reported_press;
        self.button_held = false;
        self.reported_press = false;
        self.anchor = None;
        if !reported {
            return;
        }
        let Some(pointer) = self.pointer_at(event.position) else {
            return;
        };
        self.send_mouse(
            MouseAction::Release,
            Some(GridButton::Left),
            false,
            event.modifiers,
            pointer,
        );
    }

    fn scroll_wheel(
        &mut self,
        event: &ScrollEvent,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        let lines_moved = match event.delta {
            ScrollDelta::Lines(lines) => lines.y,
            ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / self.cell.height,
        };
        self.scroll_remainder += lines_moved * self.scroll_sensitivity;
        let whole = self.scroll_remainder.trunc();
        if whole == 0.0 {
            return;
        }
        self.scroll_remainder -= whole;
        let count = whole.abs() as usize;
        // A positive wheel delta moves toward older lines.
        let toward_older = whole > 0.0;
        let Some(pointer) = self.pointer_at(event.position) else {
            return;
        };

        if self.program_tracks_mouse() {
            let mut button = GridButton::WheelDown;
            if toward_older {
                button = GridButton::WheelUp;
            }
            for _ in 0..count {
                self.send_mouse(
                    MouseAction::Press,
                    Some(button),
                    false,
                    event.modifiers,
                    pointer,
                );
            }
            return;
        }
        let Some(view) = self.views.on_screen_view() else {
            return;
        };
        if let Ok(true) = view.wheel_sends_arrows() {
            let mut key = Key::Down;
            if toward_older {
                key = Key::Up;
            }
            let press = KeyPress {
                key,
                mods: Mods::default(),
                text: "",
                repeat: false,
            };
            for _ in 0..count {
                self.send_key(&press);
            }
            cx.notify();
            return;
        }
        // The viewport counts toward older lines as negative.
        let mut lines = count as isize;
        if toward_older {
            lines = -lines;
        }
        let _ = view.scroll(Scroll::Lines(lines));
        cx.notify();
    }

    /// The pointer at `position`, kept inside the grid so a drag past its edge keeps selecting up to it.
    fn pointer_at(&self, position: Point<Pixels>) -> Option<Pointer> {
        let bounds = self.grid_bounds?;
        let size = self.grid_size?;
        let x = f32::from(position.x - bounds.origin.x).max(0.0);
        let y = f32::from(position.y - bounds.origin.y).max(0.0);
        let column = (x / self.cell.width).floor() as u16;
        let row = (y / self.cell.height).floor() as u16;
        let last_column = size.cols.saturating_sub(1);
        let last_row = size.rows.saturating_sub(1);
        let cell = CellPoint {
            x: column.min(last_column),
            y: row.min(last_row),
        };
        Some(Pointer { cell, x, y })
    }

    fn grid(&self) -> Grid {
        let mut width = 0.0;
        let mut height = 0.0;
        if let Some(bounds) = self.grid_bounds {
            width = f32::from(bounds.size.width);
            height = f32::from(bounds.size.height);
        }
        let cell = self.cell.size_px();
        Grid {
            width_px: width as u32,
            height_px: height as u32,
            cell_width_px: cell.width_px,
            cell_height_px: cell.height_px,
        }
    }
}

// The view answers an input method with an empty, caret-only text field: the shell owns the line being edited.
impl EntityInputHandler for Terminals {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<Range<usize>> {
        let marked = self.marked_text.as_ref()?;
        let length = marked.encode_utf16().count();
        Some(0..length)
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Terminals>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        self.marked_text = None;
        self.blink_on = true;
        if let Some(view) = self.views.on_screen_view() {
            let _ = view.scroll(Scroll::Bottom);
        }
        self.views.write_on_screen(text.as_bytes().to_vec());
        self.send_requests();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        let mut marked = None;
        if !new_text.is_empty() {
            marked = Some(new_text.to_string());
        }
        self.marked_text = marked;
        cx.notify();
    }

    /// Where an input method puts its candidate window: at the cursor.
    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<Bounds<Pixels>> {
        let (column, row) = self.cursor_cell?;
        let width = px(self.cell.width);
        let height = px(self.cell.height);
        let x = element_bounds.origin.x + width * f32::from(column);
        let y = element_bounds.origin.y + height * f32::from(row);
        let top_left = point(x, y);
        let cell_size = size(width, height);
        Some(Bounds::new(top_left, cell_size))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<usize> {
        None
    }
}

impl Render for Terminals {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Terminals>) -> impl IntoElement {
        let before_paint_entity = cx.entity();
        let input_entity = cx.entity();
        let release_entity = cx.entity();
        let focus = self.focus.clone();
        // The theme's background until a view draws; a program can change its own, which the frame then paints.
        let background = hsla(self.config.colors.background);
        let terminal = canvas(
            move |bounds, window, cx| {
                before_paint_entity
                    .update(cx, |terminals, _| terminals.before_paint(bounds, window))
            },
            move |bounds, prepared: Option<Prepared>, window, cx| {
                let handler = ElementInputHandler::new(bounds, input_entity.clone());
                window.handle_input(&focus, handler, cx);
                let Some(prepared) = prepared else {
                    return;
                };
                window.paint_quad(fill(bounds, hsla(prepared.drawn.background)));
                paint(
                    &prepared.drawn.painting,
                    bounds.origin,
                    prepared.cell,
                    &prepared.family,
                    px(prepared.font_size),
                    window,
                    cx,
                );
                paint_marked_text(&prepared, bounds.origin, window, cx);
            },
        );
        div()
            .size_full()
            .bg(background)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Terminals::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Terminals::mouse_down))
            .on_mouse_move(cx.listener(Terminals::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Terminals::mouse_up))
            .on_mouse_up_out(MouseButton::Left, move |event, _window, cx| {
                release_entity.update(cx, |terminals, _| terminals.release(event));
            })
            .on_scroll(cx.listener(Terminals::scroll_wheel))
            .child(terminal.size_full())
    }
}

/// What an input method is composing, drawn underlined over the cursor.
fn paint_marked_text(
    prepared: &Prepared,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(marked) = prepared.marked_text.as_ref() else {
        return;
    };
    let Some(cursor) = prepared.drawn.painting.cursor else {
        return;
    };
    let cell = prepared.cell;
    let cell_width = px(cell.width);
    let cell_height = px(cell.height);
    let x = origin.x + cell_width * f32::from(cursor.column);
    let y = origin.y + cell_height * f32::from(cursor.row);
    let top_left = point(x, y);
    let color = cursor.color;
    let run = TextRun {
        len: marked.len(),
        font: font(prepared.family.clone()),
        color,
        background_color: None,
        underline: Some(UnderlineStyle {
            thickness: px(MARKED_TEXT_UNDERLINE),
            color: Some(color),
            wavy: false,
        }),
        strikethrough: None,
    };
    let font_size = px(prepared.font_size);
    let shaped = window
        .text_system()
        .shape_line(marked.clone().into(), font_size, &[run], None);
    // Covered at its own width, which counts a wide character as two cells.
    let cover = Bounds::new(top_left, size(shaped.width, cell_height));
    let background = hsla(prepared.drawn.background);
    window.paint_quad(fill(cover, background));
    let _ = shaped.paint(top_left, cell_height, TextAlign::Left, None, window, cx);
}

fn view_config(settings: &Settings, colors: &TerminalColors) -> ViewConfig {
    ViewConfig {
        scrollback: settings.scrollback,
        colors: colors.clone(),
    }
}

fn mods_of(modifiers: Modifiers) -> Mods {
    Mods {
        shift: modifiers.shift,
        ctrl: modifiers.control,
        alt: modifiers.alt,
        super_key: false,
    }
}

/// The cell `family` gives at `size`: the advance of `m` across, ascent and descent down.
fn measure(family: &str, size: f32, cx: &App) -> CellMetrics {
    let text_system = cx.text_system();
    let font_id = text_system.resolve_font(&font(family.to_string()));
    let font_size = px(size);
    let mut width = size * FALLBACK_ADVANCE;
    if let Ok(advance) = text_system.advance(font_id, font_size, 'm') {
        width = f32::from(advance.width);
    }
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    // font-kit reports the descent below the baseline as a negative number.
    let descent = f32::from(text_system.descent(font_id, font_size)).abs();
    let height = (ascent + descent).ceil();
    CellMetrics { width, height }
}
