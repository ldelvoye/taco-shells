//! The window drawn to an image with no window on screen, after any pointer gestures played into it.

use std::path::Path;
use std::sync::Arc;

use gpui::{
    AnyWindowHandle, AppContext, HeadlessAppContext, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, PlatformInput, Point, Result, px, size,
};
use taco_settings::Appearance;

use crate::assets::{Assets, load_fonts};
use crate::fixture::Fixture;
use crate::look::Look;
use crate::platform::TrafficLights;
use crate::stand_in::StandIn;
use crate::trial::Trial;
use crate::window::WindowView;
use crate::{WINDOW_HEIGHT, WINDOW_WIDTH};

const HEADLESS: bool = true;
// Images load in the background, so the frame is drawn again once they have.
const DRAWS: usize = 3;
const DRAG_STEPS: usize = 8;

/// A pointer gesture, in window points.
#[derive(Clone, Copy, Debug)]
pub enum Gesture {
    Click(Point<Pixels>),
    Drag(Point<Pixels>, Point<Pixels>),
    /// A drag whose button is still down when the window is drawn.
    Hold(Point<Pixels>, Point<Pixels>),
}

/// Writes the window, drawn from `fixture` in `appearance` after `gestures`, to a PNG at `out`.
pub fn render_png(
    fixture: Fixture,
    appearance: Appearance,
    trial: Trial,
    gestures: &[Gesture],
    out: &Path,
) -> Result<()> {
    let platform = gpui_platform::current_platform(HEADLESS);
    let assets = Assets::for_this_build();
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| load_fonts(cx))?;

    let image_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let window = cx.open_window(image_size, |_, cx| {
        let shown = fixture.shown();
        let look = Look::new(appearance);
        let (port, published) = StandIn::start(shown.drawn.clone());
        let lights = TrafficLights::StandIns;
        cx.new(|cx| WindowView::new(shown, look, trial, lights, port, published, cx))
    })?;
    let any_window = window.into();
    draw(&mut cx, any_window)?;
    for gesture in gestures {
        play(&mut cx, any_window, *gesture)?;
    }
    for _ in 0..DRAWS {
        draw(&mut cx, any_window)?;
    }
    let image = cx.capture_screenshot(any_window)?;
    image.save(out)?;
    Ok(())
}

// An off-screen window is sent no frames, so each one is forced.
fn draw(cx: &mut HeadlessAppContext, window: AnyWindowHandle) -> Result<()> {
    cx.update_window(window, |_, window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    })?;
    cx.run_until_parked();
    Ok(())
}

fn play(cx: &mut HeadlessAppContext, window: AnyWindowHandle, gesture: Gesture) -> Result<()> {
    match gesture {
        Gesture::Click(at) => {
            send(cx, window, press(at))?;
            send(cx, window, release(at))
        }
        Gesture::Drag(from, to) => {
            send(cx, window, press(from))?;
            move_through(cx, window, from, to)?;
            send(cx, window, release(to))
        }
        Gesture::Hold(from, to) => {
            send(cx, window, press(from))?;
            move_through(cx, window, from, to)
        }
    }
}

fn move_through(
    cx: &mut HeadlessAppContext,
    window: AnyWindowHandle,
    from: Point<Pixels>,
    to: Point<Pixels>,
) -> Result<()> {
    for step in 1..=DRAG_STEPS {
        let share = step as f32 / DRAG_STEPS as f32;
        let at = from + (to - from) * share;
        let moved = MouseMoveEvent {
            position: at,
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        };
        send(cx, window, PlatformInput::MouseMove(moved))?;
    }
    Ok(())
}

fn press(at: Point<Pixels>) -> PlatformInput {
    let pressed = MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        click_count: 1,
        ..Default::default()
    };
    PlatformInput::MouseDown(pressed)
}

fn release(at: Point<Pixels>) -> PlatformInput {
    let released = MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        click_count: 1,
        ..Default::default()
    };
    PlatformInput::MouseUp(released)
}

// Each event is hit-tested against the last frame, so a frame is drawn after every one.
fn send(cx: &mut HeadlessAppContext, window: AnyWindowHandle, input: PlatformInput) -> Result<()> {
    cx.update_window(window, |_, window, cx| {
        window.dispatch_event(input, cx);
    })?;
    draw(cx, window)
}
