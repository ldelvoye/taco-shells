mod build;
mod bundle;
mod ghostty_bindings;
mod identity;
mod install;
mod repo;
mod run;
mod smoke;
mod version;

use std::env;
use std::process::ExitCode;

use build::{Channel, Profile};
use bundle::Options;

const USAGE: &str = "usage: cargo xtask <run [-- <app args>] | bundle [--release] [--stable] [--zip] | install | smoke | ghostty-bindings>";

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
        "smoke" if rest.is_empty() => smoke::run(),
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

fn bundle_options(rest: &[String]) -> Option<Options> {
    let mut options = Options {
        profile: Profile::Debug,
        channel: Channel::Development,
        zip: false,
        icon: true,
    };
    for argument in rest {
        match argument.as_str() {
            "--release" => options.profile = Profile::Release,
            "--stable" => options.channel = Channel::Stable,
            "--zip" => options.zip = true,
            _ => return None,
        }
    }
    Some(options)
}
