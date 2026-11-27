mod build;
mod bump;
mod bundle;
mod ghostty_bindings;
mod identity;
mod install;
mod measure;
mod promote;
mod rehearse;
mod release;
mod repo;
mod run;
mod smoke;
mod try_stable;
mod version;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use build::{Channel, Profile};
use bundle::Options;

const USAGE: &str = "usage: cargo xtask <run [-- <app args>] | bundle [--release] [--stable] [--zip] [--config-dir <absolute path>] | install | rehearse-update [--electron <app> | --from-rust] [--until-ready] | smoke | measure [--terminals=N ...] | ghostty-bindings | release [--dry-run] | promote [--publish] [--notes-file=<path>] [--key=<path>] | try-stable>";

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let Some((command, rest)) = arguments.split_first() else {
        return usage();
    };

    let result = match command.as_str() {
        "run" => match app_arguments(rest) {
            Some(app_arguments) => run::run(app_arguments),
            None => return usage(),
        },
        "bundle" => match bundle_options(rest) {
            Some(options) => bundle::run(&options),
            None => return usage(),
        },
        "install" if rest.is_empty() => install::run(),
        "rehearse-update" => match rehearse_options(rest) {
            Some(options) => rehearse::run(&options),
            None => return usage(),
        },
        "smoke" if rest.is_empty() => smoke::run(),
        "measure" => measure::run(rest),
        "ghostty-bindings" if rest.is_empty() => ghostty_bindings::run(),
        "release" => match rest {
            [] => release::run(false),
            [flag] if flag == "--dry-run" => release::run(true),
            _ => return usage(),
        },
        "promote" => match promote_options(rest) {
            Some(options) => promote::run(&options),
            None => return usage(),
        },
        "try-stable" if rest.is_empty() => try_stable::run(),
        _ => return usage(),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::from(1)
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn promote_options(rest: &[String]) -> Option<promote::Options> {
    let mut options = promote::Options {
        publish: false,
        notes_file: None,
        key: None,
    };
    for argument in rest {
        if argument == "--publish" {
            options.publish = true;
            continue;
        }
        if let Some(path) = argument.strip_prefix("--notes-file=") {
            options.notes_file = Some(PathBuf::from(path));
            continue;
        }
        let key = argument.strip_prefix("--key=")?;
        options.key = Some(PathBuf::from(key));
    }
    Some(options)
}

fn app_arguments(rest: &[String]) -> Option<&[String]> {
    match rest {
        [] => Some(rest),
        [separator, app_arguments @ ..] if separator == "--" => Some(app_arguments),
        _ => None,
    }
}

fn rehearse_options(rest: &[String]) -> Option<rehearse::Options> {
    let mut options = rehearse::Options {
        electron: None,
        from_rust: false,
        until_ready: false,
    };
    let mut remaining = rest.iter();
    while let Some(argument) = remaining.next() {
        match argument.as_str() {
            "--electron" => {
                let path = remaining.next()?;
                options.electron = Some(PathBuf::from(path));
            }
            "--until-ready" => options.until_ready = true,
            "--from-rust" => options.from_rust = true,
            _ => return None,
        }
    }
    Some(options)
}

fn bundle_options(rest: &[String]) -> Option<Options> {
    let mut options = Options {
        profile: Profile::Debug,
        channel: Channel::Development,
        zip: false,
        icon: true,
        drive: false,
        config_dir: None,
    };
    let mut remaining = rest.iter();
    while let Some(argument) = remaining.next() {
        match argument.as_str() {
            "--release" => options.profile = Profile::Release,
            "--stable" => options.channel = Channel::Stable,
            "--zip" => options.zip = true,
            "--config-dir" => {
                let directory = PathBuf::from(remaining.next()?);
                if !directory.is_absolute() {
                    return None;
                }
                options.config_dir = Some(directory);
            }
            _ => return None,
        }
    }
    Some(options)
}
