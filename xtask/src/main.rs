mod build;
mod bundle;
mod ghostty_bindings;
mod identity;
mod install;
mod measure;
mod rehearse;
mod repo;
mod run;
mod smoke;
mod version;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use build::{Channel, Profile};
use bundle::Options;

const USAGE: &str = "usage: cargo xtask <run [-- <app args>] | bundle [--release] [--stable] [--zip] [--config-dir <absolute path>] | install | rehearse-update [--electron <app> | --from-rust] [--until-ready] | smoke | measure [--terminals=N ...] | ghostty-bindings>";

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
