//! The window's macOS chrome: the traffic lights, and the name it gives this machine.

use gpui::{Pixels, Point, point, px};

use crate::look::TITLE_BAR_HEIGHT;

/// The machine a local terminal runs on, as the title bar names it.
pub const THIS_MACHINE: &str = "this Mac";

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
