use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::identity::{CHANNEL_VARIABLE, STABLE_CHANNEL};
use crate::repo;

pub const APP_EXECUTABLE: &str = "taco-shells";
pub const RUNTIME_EXECUTABLE: &str = "taco-shells-runtime";
const APP_PACKAGE: &str = "taco-app";
const RUNTIME_PACKAGE: &str = "taco-runtime";

#[derive(Clone, Copy)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    pub fn directory(self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

#[derive(Clone, Copy)]
pub enum Channel {
    Development,
    Stable,
}

pub fn binaries(profile: Profile, channel: Channel) -> Result<PathBuf, String> {
    let repo_root = repo::root()?;
    let target = repo::target_dir()?;
    let cargo_path = env::var_os("CARGO");
    let program = match cargo_path {
        Some(path) => path,
        None => OsString::from("cargo"),
    };

    let mut cargo = cargo_command(program, profile, channel, &target);
    cargo.current_dir(&repo_root);
    repo::run(&mut cargo)?;

    Ok(target.join(profile.directory()))
}

fn cargo_command(
    program: OsString,
    profile: Profile,
    channel: Channel,
    target_dir: &Path,
) -> Command {
    let mut cargo = Command::new(program);
    cargo.args(["build", "-p", APP_PACKAGE, "-p", RUNTIME_PACKAGE]);
    cargo.arg("--target-dir");
    cargo.arg(target_dir);
    match profile {
        Profile::Release => {
            cargo.arg("--release");
        }
        Profile::Debug => {}
    }
    match channel {
        Channel::Stable => {
            cargo.env(CHANNEL_VARIABLE, STABLE_CHANNEL);
        }
        Channel::Development => {
            // So a variable left in someone's shell cannot produce the stable app.
            cargo.env_remove(CHANNEL_VARIABLE);
        }
    }
    cargo
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn channel_entry(command: &Command) -> Option<Option<OsString>> {
        let variable = OsStr::new(CHANNEL_VARIABLE);
        for (name, value) in command.get_envs() {
            if name == variable {
                return Some(value.map(OsString::from));
            }
        }
        None
    }

    #[test]
    fn only_a_stable_build_carries_the_channel() {
        let target = Path::new("target");
        let development = cargo_command(
            OsString::from("cargo"),
            Profile::Debug,
            Channel::Development,
            target,
        );
        let stable = cargo_command(
            OsString::from("cargo"),
            Profile::Debug,
            Channel::Stable,
            target,
        );

        assert_eq!(channel_entry(&development), Some(None));
        assert_eq!(
            channel_entry(&stable),
            Some(Some(OsString::from(STABLE_CHANNEL)))
        );
    }
}
