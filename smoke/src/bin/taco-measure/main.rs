mod flags;
mod report;
mod stages;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use taco_smoke::launch;

const USAGE: &str = "usage: taco-measure <app> <work dir> <report path> [--terminals=24] [--full=8] [--flooding=4] [--flood-seconds=10] [--samples=50]";
const POSITIONAL: usize = 3;

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    if arguments.len() < POSITIONAL {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let (positional, flag_arguments) = arguments.split_at(POSITIONAL);
    let app = PathBuf::from(&positional[0]);
    let work = PathBuf::from(&positional[1]);
    let report_path = PathBuf::from(&positional[2]);

    let parsed = flags::parse(flag_arguments);
    let flags = match parsed {
        Ok(flags) => flags,
        Err(reason) => {
            eprintln!("{reason}");
            return ExitCode::from(2);
        }
    };

    println!("stage: launch");
    let config = work.join("config");
    let log = work.join("app.log");
    let launched = match launch::launch(&app, &config, &log) {
        Ok(launched) => launched,
        Err(reason) => {
            eprintln!("stage \"launch\" failed: {reason}");
            return ExitCode::from(1);
        }
    };

    let mut launched = launched;
    let app_pid = launched.child.id();
    let outcome = stages::run(&mut launched.driver, app_pid, &flags, &work);
    launch::quit(launched);

    let measured = match outcome {
        Ok(measured) => measured,
        Err(reason) => {
            eprintln!("{reason}");
            return ExitCode::from(1);
        }
    };

    let report = report::render(&flags, &measured);
    println!("{report}");
    if let Err(error) = fs::write(&report_path, &report) {
        eprintln!("could not write {}: {error}", report_path.display());
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
