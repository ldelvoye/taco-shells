//! The tests that drive the real app: one launch of a development bundle, the checks in order, stopping at the first
//! failure. Run through `cargo xtask smoke`, which builds the bundle and the config directory it starts from.

mod bundle;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: taco-smoke <path to the .app> <config directory to start from>";

const CHECKS: [Check; 1] = [Check {
    name: "the dev bundle holds only the dev app's names",
    run: bundle::holds_only_dev_names,
}];

/// What every check is given: the bundle under test and the throwaway config directory it starts from.
pub struct Smoke {
    pub app: PathBuf,
    pub config: PathBuf,
}

struct Check {
    name: &'static str,
    run: fn(&Smoke) -> Result<(), String>,
}

fn main() -> ExitCode {
    let arguments: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    let [app, config] = arguments.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let smoke = Smoke {
        app: app.clone(),
        config: config.clone(),
    };

    for check in CHECKS {
        let outcome = (check.run)(&smoke);
        match outcome {
            Ok(()) => println!("ok    {}", check.name),
            Err(reason) => {
                println!("FAIL  {}: {reason}", check.name);
                return ExitCode::FAILURE;
            }
        }
    }
    println!("{} passed", CHECKS.len());
    ExitCode::SUCCESS
}
