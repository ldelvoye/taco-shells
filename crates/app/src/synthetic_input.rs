//! The mouse events a made-up gesture is built from, shared by the renderer and the smoke tests' driver.

use gpui::{
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point,
};

/// How many moves a held button makes between where it was pressed and where it goes.
const DRAG_STEPS: u16 = 8;

pub fn press(at: Point<Pixels>, button: MouseButton) -> PlatformInput {
    let pressed = MouseDownEvent {
        button,
        position: at,
        click_count: 1,
        ..Default::default()
    };
    PlatformInput::MouseDown(pressed)
}

pub fn release(at: Point<Pixels>, button: MouseButton) -> PlatformInput {
    let released = MouseUpEvent {
        button,
        position: at,
        click_count: 1,
        ..Default::default()
    };
    PlatformInput::MouseUp(released)
}

/// The moves of the held left button from `from` to `to`, the last one on `to`.
pub fn moves_through(from: Point<Pixels>, to: Point<Pixels>) -> Vec<PlatformInput> {
    let mut moves = Vec::new();
    for step in 1..=DRAG_STEPS {
        let share = f32::from(step) / f32::from(DRAG_STEPS);
        let at = from + (to - from) * share;
        let moved = MouseMoveEvent {
            position: at,
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        };
        moves.push(PlatformInput::MouseMove(moved));
    }
    moves
}
