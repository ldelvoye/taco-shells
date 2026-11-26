//! The tests that drive the real app: one launch of a development bundle, the checks in order, stopping at the first
//! failure. Run through `cargo xtask smoke`, which builds the bundle and the config directory it starts from.

mod bundle;
mod window;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use taco_smoke::driver::Driver;
use taco_smoke::launch;

const USAGE: &str = "usage: taco-smoke <path to the .app> <config directory to start from>";
const APP_LOG: &str = "app.log";

const BEFORE_LAUNCH: [Check; 1] = [Check {
    name: "the dev bundle holds only the dev app's names",
    run: bundle::holds_only_dev_names,
}];

const LAUNCHED: [LaunchedCheck; 4] = [
    LaunchedCheck {
        name: "the window draws rows",
        run: window::draws_rows,
    },
    LaunchedCheck {
        name: "every marked part of the window has bounds, and nothing else does",
        run: window::every_marked_part_has_bounds,
    },
    LaunchedCheck {
        name: "a row dragged down the list moves",
        run: window::a_dragged_row_moves,
    },
    LaunchedCheck {
        name: "the window draws in light and in dark",
        run: window::draws_in_light_and_dark,
    },
];

/// What every check is given: the bundle under test and the throwaway config directory it starts from.
pub struct Smoke {
    pub app: PathBuf,
    pub config: PathBuf,
}

struct Check {
    name: &'static str,
    run: fn(&Smoke) -> Result<(), String>,
}

struct LaunchedCheck {
    name: &'static str,
    run: fn(&Smoke, &mut Driver) -> Result<(), String>,
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

    for check in BEFORE_LAUNCH {
        let outcome = (check.run)(&smoke);
        if !reported(check.name, outcome) {
            return ExitCode::FAILURE;
        }
    }

    let log = log_path(&smoke);
    let mut launched = match launch::launch(&smoke.app, &smoke.config, &log) {
        Ok(launched) => launched,
        Err(reason) => {
            println!(
                "FAIL  the app starts with its driver: {reason} (log: {})",
                log.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let mut passed = true;
    for check in LAUNCHED {
        let outcome = (check.run)(&smoke, &mut launched.driver);
        if !reported(check.name, outcome) {
            passed = false;
            println!("      the app's log: {}", log.display());
            break;
        }
    }
    launch::quit(launched);

    if !passed {
        return ExitCode::FAILURE;
    }
    let total = BEFORE_LAUNCH.len() + LAUNCHED.len();
    println!("{total} passed");
    ExitCode::SUCCESS
}

fn reported(name: &str, outcome: Result<(), String>) -> bool {
    match outcome {
        Ok(()) => {
            println!("ok    {name}");
            true
        }
        Err(reason) => {
            println!("FAIL  {name}: {reason}");
            false
        }
    }
}

fn log_path(smoke: &Smoke) -> PathBuf {
    match smoke.config.parent() {
        Some(work) => work.join(APP_LOG),
        None => smoke.config.join(APP_LOG),
    }
}
