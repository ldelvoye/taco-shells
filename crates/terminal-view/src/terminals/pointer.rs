//! The pointer and the wheel over the terminal on screen: reported to a program tracking the mouse, or else selecting and scrolling.

use gpui::{
    Context, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    ScrollDelta, ScrollEvent, Window,
};
use taco_ghostty::input::{
    CellPoint, Grid, Key, KeyPress, Mods, MouseAction, MouseButton as GridButton, MouseInput,
    Scroll,
};

use super::{Terminals, TerminalsEvent};
use crate::keys::mods_of;

/// Where the pointer is: the cell under it, and its position in the grid's pixels.
#[derive(Clone, Copy)]
struct Pointer {
    cell: CellPoint,
    x: f32,
    y: f32,
}

impl Terminals {
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

    pub(super) fn mouse_down(
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

    pub(super) fn mouse_move(
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

    pub(super) fn mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) {
        self.release(event);
    }

    /// The end of a press, wherever the pointer is: a release outside the grid ends a drag all the same.
    pub(super) fn release(&mut self, event: &MouseUpEvent) {
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

    pub(super) fn scroll_wheel(
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
