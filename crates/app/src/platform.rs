//! The window's macOS chrome: the traffic lights, the name it gives this machine, and keeping out of sight when asked.

use std::ffi::OsStr;

use gpui::{ActivationPolicy, App, Pixels, Point, point, px};
use taco_identity::{BACKGROUND_WINDOW_VARIABLE, IS_STABLE};

use crate::look::TITLE_BAR_HEIGHT;

/// The machine a local terminal runs on, as the title bar names it.
pub const THIS_MACHINE: &str = "this Mac";

const BACKGROUND_WINDOW_ON: &str = "1";

// Where macOS draws the three traffic lights, which the title bar leaves room for.
const TRAFFIC_LIGHT_INSET: f32 = 14.;
const TRAFFIC_LIGHT_HEIGHT: f32 = 14.;
const TRAFFIC_LIGHTS_WIDTH: f32 = 52.;
const TRAFFIC_LIGHT_GAP: f32 = 16.;
pub const TRAFFIC_LIGHT_DIAMETER: f32 = TRAFFIC_LIGHT_HEIGHT - 2.;

/// Who draws the traffic lights: the system, or the window itself in an image drawn with no window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficLights {
    Native,
    StandIns,
}

pub fn is_background_window() -> bool {
    let asked = std::env::var_os(BACKGROUND_WINDOW_VARIABLE);
    keeps_out_of_sight(asked.as_deref(), IS_STABLE)
}

/// Whether a launch with this value of the variable keeps its window out of sight: never in the stable app.
fn keeps_out_of_sight(asked: Option<&OsStr>, stable: bool) -> bool {
    if stable {
        return false;
    }
    let Some(value) = asked else {
        return false;
    };
    value == BACKGROUND_WINDOW_ON
}

/// Takes the app out of the Dock and the app switcher.
pub fn keep_out_of_the_dock(cx: &mut App) {
    cx.set_activation_policy(ActivationPolicy::Accessory);
}

pub fn traffic_light_position() -> Point<Pixels> {
    let top = (TITLE_BAR_HEIGHT - TRAFFIC_LIGHT_HEIGHT) / 2.;
    point(px(TRAFFIC_LIGHT_INSET), px(top))
}

/// The room the traffic lights take at the start of the title bar, which full screen hides.
pub fn traffic_lights_room(full_screen: bool) -> f32 {
    if full_screen {
        return 0.;
    }
    TRAFFIC_LIGHT_INSET + TRAFFIC_LIGHTS_WIDTH + TRAFFIC_LIGHT_GAP
}

/// Each traffic light's center, for the stand-ins.
pub fn traffic_light_centers() -> [Point<Pixels>; 3] {
    let top = traffic_light_position().y + px(TRAFFIC_LIGHT_HEIGHT / 2.);
    let step = (TRAFFIC_LIGHTS_WIDTH - TRAFFIC_LIGHT_HEIGHT) / 2.;
    let first = TRAFFIC_LIGHT_INSET + TRAFFIC_LIGHT_HEIGHT / 2.;
    [0., 1., 2.].map(|index| point(px(first + step * index), top))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::keeps_out_of_sight;

    #[test]
    fn only_a_development_build_asked_with_1_keeps_out_of_sight() {
        let on = Some(OsStr::new("1"));
        let other = Some(OsStr::new("yes"));
        let cases = [
            ("development, asked", on, false, true),
            ("stable, asked", on, true, false),
            ("development, not asked", None, false, false),
            ("development, another value", other, false, false),
        ];
        for (case, asked, stable, hidden) in cases {
            assert_eq!(keeps_out_of_sight(asked, stable), hidden, "{case}");
        }
    }
}
