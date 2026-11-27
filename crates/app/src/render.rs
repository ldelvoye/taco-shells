//! The window drawn to an image with no window on screen, after any gestures played into it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyWindowHandle, AppContext, HeadlessAppContext, Keystroke, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PlatformInput, Point, Result, ScrollDelta, ScrollEvent,
    point, px, size,
};
use taco_settings::Appearance;

use crate::assets::{Assets, load_fonts};
use crate::fixture::Fixture;
use crate::look::Look;
use crate::platform::TrafficLights;
use crate::stand_in::StandIn;
use crate::text_field;
use crate::window::WindowView;
use crate::{WINDOW_HEIGHT, WINDOW_WIDTH};

const HEADLESS: bool = true;
// Images load in the background, so the frame is drawn again once they have.
const DRAWS: usize = 3;
const DRAG_STEPS: usize = 8;

/// Something done to the window before it is drawn, with points in window points.
#[derive(Clone, Debug)]
pub enum Gesture {
    Click(Point<Pixels>),
    RightClick(Point<Pixels>),
    Drag(Point<Pixels>, Point<Pixels>),
    /// A drag whose button is still down when the window is drawn.
    Hold(Point<Pixels>, Point<Pixels>),
    /// Keystrokes in gpui's notation, such as `cmd-a` or `enter`, or a single character to type.
    Keys(Vec<String>),
    /// Time passing, drawn once at the end, for something that opens or moves over time.
    Wait(Duration),
    /// The wheel turned over a point, by a distance in window points: positive scrolls down.
    Scroll(Point<Pixels>, Pixels),
}

/// Writes the window, drawn from `fixture` in `appearance` after `gestures`, to a PNG at `out`.
/// `width` in window points, the app's usual width when none is given.
pub fn render_png(
    fixture: Fixture,
    appearance: Appearance,
    gestures: &[Gesture],
    out: &Path,
    width: Option<f32>,
) -> Result<()> {
    let platform = gpui_platform::current_platform(HEADLESS);
    let assets = Assets::for_this_build();
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| load_fonts(cx))?;
    cx.update(text_field::bind_keys);

    let mut window_width = WINDOW_WIDTH;
    if let Some(asked) = width {
        window_width = asked;
    }
    let image_size = size(px(window_width), px(WINDOW_HEIGHT));
    let window = cx.open_window(image_size, |_, cx| {
        let shown = fixture.shown();
        let look = Look::new(appearance);
        let ports = StandIn::start(&shown, fixture.logs());
        let lights = TrafficLights::StandIns;
        cx.new(|cx| WindowView::new(shown, look, lights, ports, cx))
    })?;
    let any_window = window.into();
    draw(&mut cx, any_window)?;
    for gesture in gestures {
        play(&mut cx, any_window, gesture)?;
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

fn play(cx: &mut HeadlessAppContext, window: AnyWindowHandle, gesture: &Gesture) -> Result<()> {
    match gesture {
        Gesture::Click(at) => {
            send(cx, window, press(*at, MouseButton::Left))?;
            send(cx, window, release(*at, MouseButton::Left))
        }
        Gesture::RightClick(at) => {
            send(cx, window, press(*at, MouseButton::Right))?;
            send(cx, window, release(*at, MouseButton::Right))
        }
        Gesture::Drag(from, to) => {
            send(cx, window, press(*from, MouseButton::Left))?;
            move_through(cx, window, *from, *to)?;
            send(cx, window, release(*to, MouseButton::Left))
        }
        Gesture::Hold(from, to) => {
            send(cx, window, press(*from, MouseButton::Left))?;
            move_through(cx, window, *from, *to)
        }
        Gesture::Keys(keys) => {
            for key in keys {
                type_key(cx, window, key)?;
            }
            Ok(())
        }
        Gesture::Wait(time) => {
            std::thread::sleep(*time);
            draw(cx, window)
        }
        Gesture::Scroll(at, by) => {
            // gpui's wheel delta moves the content, so scrolling down is a negative one.
            let moved = point(px(0.), -*by);
            let scrolled = ScrollEvent {
                position: *at,
                delta: ScrollDelta::Pixels(moved),
                ..Default::default()
            };
            send(cx, window, PlatformInput::Scroll(scrolled))?;
            draw(cx, window)
        }
    }
}

fn type_key(cx: &mut HeadlessAppContext, window: AnyWindowHandle, key: &str) -> Result<()> {
    let is_character = key.chars().count() == 1;
    cx.update_window(window, |_, window, cx| -> Result<()> {
        let os = cx.platform_info().os;
        let mut keystroke = Keystroke::parse_with_os(key, os)?;
        if is_character {
            keystroke.key_char = Some(key.to_string());
        }
        window.dispatch_keystroke(keystroke, cx);
        Ok(())
    })??;
    draw(cx, window)
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

fn press(at: Point<Pixels>, button: MouseButton) -> PlatformInput {
    let pressed = MouseDownEvent {
        button,
        position: at,
        click_count: 1,
        ..Default::default()
    };
    PlatformInput::MouseDown(pressed)
}

fn release(at: Point<Pixels>, button: MouseButton) -> PlatformInput {
    let released = MouseUpEvent {
        button,
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
