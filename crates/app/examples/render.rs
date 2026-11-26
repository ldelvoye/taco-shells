//! `cargo run -p taco-app --features render --example render -- <mixed|quiet|empty> <light|dark> <out.png> [gesture]...`
//!
//! Gestures, in window points, played in order: `click:X,Y`, `rightclick:X,Y`, `drag:X,Y:X,Y`,
//! `hold:X,Y:X,Y` for a drag still under way when the window is drawn, `type:TEXT` to type each
//! character, and `key:NAME` for one keystroke such as `enter`.

use std::path::PathBuf;
use std::process::ExitCode;

use gpui::{Pixels, Point, point, px};
use taco_app::Fixture;
use taco_app::render::{Gesture, render_png};
use taco_settings::Appearance;

const USAGE: &str = "usage: render <mixed|quiet|empty> <light|dark> <out.png> [click:X,Y|rightclick:X,Y|drag:X,Y:X,Y|hold:X,Y:X,Y|type:TEXT|key:NAME]...";

struct Request {
    fixture: Fixture,
    appearance: Appearance,
    out: PathBuf,
    gestures: Vec<Gesture>,
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(request) = parsed(&arguments) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let rendered = render_png(
        request.fixture,
        request.appearance,
        &request.gestures,
        &request.out,
    );
    if let Err(error) = rendered {
        eprintln!("render failed: {error:#}");
        return ExitCode::FAILURE;
    }
    println!("{}", request.out.display());
    ExitCode::SUCCESS
}

fn parsed(arguments: &[String]) -> Option<Request> {
    let [fixture_name, appearance_name, out, extras @ ..] = arguments else {
        return None;
    };
    let fixture = Fixture::named(fixture_name)?;
    let appearance = match appearance_name.as_str() {
        "light" => Appearance::Light,
        "dark" => Appearance::Dark,
        _ => return None,
    };
    let mut gestures = Vec::new();
    for extra in extras {
        gestures.push(gesture_from(extra)?);
    }
    let request = Request {
        fixture,
        appearance,
        out: PathBuf::from(out),
        gestures,
    };
    Some(request)
}

fn gesture_from(text: &str) -> Option<Gesture> {
    let (kind, points) = text.split_once(':')?;
    let parts: Vec<&str> = points.split(':').collect();
    if kind == "type" {
        let characters = points.chars().map(|character| character.to_string());
        return Some(Gesture::Keys(characters.collect()));
    }
    if kind == "key" {
        return Some(Gesture::Keys(vec![points.to_string()]));
    }
    if kind == "wait" {
        let millis = points.parse().ok()?;
        return Some(Gesture::Wait(std::time::Duration::from_millis(millis)));
    }
    match (kind, parts.as_slice()) {
        ("click", [at]) => Some(Gesture::Click(point_from(at)?)),
        ("rightclick", [at]) => Some(Gesture::RightClick(point_from(at)?)),
        ("drag", [from, to]) => Some(Gesture::Drag(point_from(from)?, point_from(to)?)),
        ("hold", [from, to]) => Some(Gesture::Hold(point_from(from)?, point_from(to)?)),
        _ => None,
    }
}

fn point_from(text: &str) -> Option<Point<Pixels>> {
    let (x, y) = text.split_once(',')?;
    let x: f32 = x.parse().ok()?;
    let y: f32 = y.parse().ok()?;
    Some(point(px(x), px(y)))
}
