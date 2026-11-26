//! `cargo run -p taco-app --features render --example render -- <mixed|quiet|empty> <light|dark> <out.png> [cards|strips] [title|head|foot]`

use std::path::PathBuf;
use std::process::ExitCode;

use taco_app::{BannerLayout, Fixture, PlusPlace, Trial};
use taco_settings::Appearance;

const USAGE: &str =
    "usage: render <mixed|quiet|empty> <light|dark> <out.png> [cards|strips] [title|head|foot]";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some((fixture, appearance, out, trial)) = parsed(&arguments) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if let Err(error) = taco_app::render::render_png(fixture, appearance, trial, &out) {
        eprintln!("render failed: {error:#}");
        return ExitCode::FAILURE;
    }
    println!("{}", out.display());
    ExitCode::SUCCESS
}

fn parsed(arguments: &[String]) -> Option<(Fixture, Appearance, PathBuf, Trial)> {
    let [fixture_name, appearance_name, out, choices @ ..] = arguments else {
        return None;
    };
    let fixture = Fixture::named(fixture_name)?;
    let appearance = match appearance_name.as_str() {
        "light" => Appearance::Light,
        "dark" => Appearance::Dark,
        _ => return None,
    };
    let mut trial = Trial::default();
    for choice in choices {
        match choice.as_str() {
            "cards" => trial.banners = BannerLayout::Cards,
            "strips" => trial.banners = BannerLayout::Strips,
            "title" => trial.plus = PlusPlace::Title,
            "head" => trial.plus = PlusPlace::Head,
            "foot" => trial.plus = PlusPlace::Foot,
            _ => return None,
        }
    }
    Some((fixture, appearance, PathBuf::from(out), trial))
}
