//! The tests that drive the real app: one launch of a development bundle, the checks in order, stopping at the first
//! failure. Run through `cargo xtask smoke`, which builds the bundle first.

mod bundle;

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const CHECKS: [Check; 1] = [Check {
    name: "the dev bundle holds only the dev app's names",
    run: bundle::holds_only_dev_names,
}];

struct Check {
    name: &'static str,
    run: fn(&Path) -> Result<(), String>,
}

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let Some(app_argument) = arguments.next() else {
        eprintln!("usage: taco-smoke <path to the .app>");
        return ExitCode::from(2);
    };
    let app = PathBuf::from(app_argument);

    for check in CHECKS {
        let outcome = (check.run)(&app);
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
