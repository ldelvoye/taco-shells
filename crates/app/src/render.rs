//! The window drawn to an image with no window on screen.

use std::path::Path;
use std::sync::Arc;

use gpui::{AppContext, HeadlessAppContext, Result, px, size};
use taco_settings::Appearance;

use crate::assets::{Assets, load_fonts};
use crate::fixture::Fixture;
use crate::look::Look;
use crate::platform::TrafficLights;
use crate::trial::Trial;
use crate::window::WindowView;
use crate::{WINDOW_HEIGHT, WINDOW_WIDTH};

const HEADLESS: bool = true;
// Images load in the background, so the frame is drawn again once they have.
const DRAWS: usize = 3;

/// Writes the window, drawn from `fixture` in `appearance`, to a PNG at `out`.
pub fn render_png(
    fixture: Fixture,
    appearance: Appearance,
    trial: Trial,
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
        let view = WindowView::new(shown, look, trial, TrafficLights::StandIns);
        cx.new(|_| view)
    })?;
    let any_window = window.into();
    // An off-screen window is sent no frames, so each one is forced.
    for _ in 0..DRAWS {
        cx.update_window(any_window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        })?;
        cx.run_until_parked();
    }
    let image = cx.capture_screenshot(any_window)?;
    image.save(out)?;
    Ok(())
}
