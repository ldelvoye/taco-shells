use std::process::{self, Command};

use crate::build::{self, APP_EXECUTABLE, Build, Channel, Profile};

pub fn run(arguments: &[String]) -> Result<(), String> {
    let build = Build {
        profile: Profile::Debug,
        channel: Channel::Development,
        drive: false,
        config_dir: None,
    };
    let binaries = build::binaries(&build)?;
    let app = binaries.join(APP_EXECUTABLE);

    let mut command = Command::new(&app);
    command.args(arguments);
    let status = command
        .status()
        .map_err(|error| format!("could not start {}: {error}", app.display()))?;
    if status.success() {
        return Ok(());
    }
    match status.code() {
        Some(code) => process::exit(code),
        None => Err(format!("{} ended with {status}", app.display())),
    }
}
