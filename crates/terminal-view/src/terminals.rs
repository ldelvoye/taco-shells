//! The terminals in the middle of the window: the views kept, the one on screen drawn, and what the person does to it.

mod ime;
mod pointer;

use std::io;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::Receiver;
use gpui::{
    Bounds, Context, ElementInputHandler, EventEmitter, FocusHandle, InteractiveElement as _,
    IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Pixels, Render, Styled as _, Task,
    Window, canvas, div, fill, px,
};
use taco_ghostty::input::{CellPoint, KeyEncoder, KeyPress};
use taco_protocol::{TerminalColors, TerminalEvent, TerminalId, TerminalSize};
use taco_settings::Settings;

use crate::connection::{Requests, connect};
use crate::draw::{Prepared, cursor_as_shown, hsla, paint, paint_marked_text};
use crate::grid::{CellMetrics, font_family, grid_that_fits, measure};
use crate::keys::key_press;
use crate::view::ViewConfig;
use crate::views::{Fit, Views};

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
    _events: Task<()>,
    /// Runs only while the view has focus and its cursor blinks, so an idle app does not wake.
    blink: Option<Task<()>>,
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

        let installed = cx.text_system().all_font_names();
        let family = font_family(&settings.font_family, &installed);
        let font_size = settings.font_size as f32;
        let cell = measure(&family, font_size, cx);
        let config = view_config(settings, colors);
        Terminals {
            requests,
            views: Views::new(config, cell.size_px()),
            keys,
            focus: cx.focus_handle(),
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
            _events: task,
            blink: None,
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
        self.views.configure(view_config(settings, colors));
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

    /// The terminal's rows as shown, top to bottom, without trailing blanks. None while it has no attached view.
    pub fn visible_lines(&mut self, id: &TerminalId) -> Option<Vec<String>> {
        let view = self.views.view(id)?;
        let read = view.visible_lines().ok();
        read.flatten()
    }

    /// Pastes `text`, bracketed when the program asked for it, so a multi-line paste is not run line by line.
    pub fn paste(&mut self, id: &TerminalId, text: &str, cx: &mut Context<Terminals>) {
        let Some(view) = self.views.view(id) else {
            return;
        };
        if let Ok(bytes) = view.paste_bytes(text) {
            self.views.type_into(id.clone(), bytes);
        }
        self.send_requests();
        cx.notify();
    }

    /// Bytes as if typed: the line-editing commands come through here.
    pub fn type_bytes(&mut self, id: &TerminalId, bytes: &[u8]) {
        self.views.type_into(id.clone(), bytes.to_vec());
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
    fn before_paint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Terminals>,
    ) -> Option<Prepared> {
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
        self.views.report_focus(focused);
        let prepared = self.draw_on_screen();
        self.blink_while_wanted(cx);
        self.send_requests();
        prepared
    }

    fn blink_while_wanted(&mut self, cx: &mut Context<Terminals>) {
        let wanted = self.focused && self.cursor_blinks;
        if !wanted {
            self.blink = None;
            return;
        }
        if self.blink.is_some() {
            return;
        }
        // A cursor that starts blinking again, such as after a program hid it to redraw, starts shown.
        self.blink_on = true;
        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK_PHASE).await;
                let blinked = this.update(cx, |terminals, cx| {
                    terminals.blink_on = !terminals.blink_on;
                    cx.notify();
                });
                if blinked.is_err() {
                    break;
                }
            }
        });
        self.blink = Some(blink);
    }

    fn draw_on_screen(&mut self) -> Option<Prepared> {
        self.cursor_cell = None;
        self.cursor_blinks = false;
        let id = self.views.on_screen().cloned()?;
        let selection = self.views.config().colors.selection;
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
            self.views.type_into(id, bytes);
        }
        self.send_requests();
    }
}

impl Render for Terminals {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Terminals>) -> impl IntoElement {
        let before_paint_entity = cx.entity();
        let input_entity = cx.entity();
        let release_entity = cx.entity();
        let focus = self.focus.clone();
        // The theme's background until a view draws; a program can change its own, which the frame then paints.
        let background = hsla(self.views.config().colors.background);
        let terminal = canvas(
            move |bounds, window, cx| {
                before_paint_entity.update(cx, |terminals, cx| {
                    terminals.before_paint(bounds, window, cx)
                })
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

fn view_config(settings: &Settings, colors: &TerminalColors) -> ViewConfig {
    ViewConfig {
        scrollback: settings.scrollback,
        colors: colors.clone(),
    }
}
