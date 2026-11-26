use std::process::{self, Command};

use crate::build::{self, APP_EXECUTABLE, Channel, Profile};

pub fn run(arguments: &[String]) -> Result<(), String> {
    let binaries = build::binaries(Profile::Debug, Channel::Development)?;
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
