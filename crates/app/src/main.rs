use std::process::ExitCode;

use taco_app::Fixture;

const USAGE: &str = "usage: taco-shells [--fixture mixed|quiet|empty]";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(fixture) = fixture_from(&arguments) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    taco_app::run(fixture);
    ExitCode::SUCCESS
}

fn fixture_from(arguments: &[String]) -> Option<Fixture> {
    match arguments {
        [] => Some(Fixture::Mixed),
        [flag, name] if flag == "--fixture" => Fixture::named(name),
        _ => None,
    }
}
