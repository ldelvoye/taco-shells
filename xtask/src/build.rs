use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use taco_identity::Channel;
use taco_identity::{CHANNEL_VARIABLE, DEV_CONFIG_DIR_VARIABLE, STABLE_CHANNEL};

use crate::repo;

const APP_PACKAGE: &str = "taco-app";
const RUNTIME_PACKAGE: &str = "taco-runtime";
const PLUGIN_PACKAGE: &str = "taco-plugin";
const DRIVE_FEATURE: &str = "taco-app/drive";

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

/// What the programs are built as. `config_dir` is compiled into a development build as its config directory.
pub struct Build<'a> {
    pub profile: Profile,
    pub channel: Channel,
    pub drive: bool,
    pub config_dir: Option<&'a Path>,
}

pub fn binaries(build: &Build) -> Result<PathBuf, String> {
    allowed(build)?;
    let repo_root = repo::root()?;
    let target = repo::target_dir()?;
    let program = repo::cargo_program();

    let mut cargo = cargo_command(program, build, &target);
    cargo.current_dir(&repo_root);
    repo::run(&mut cargo)?;

    Ok(target.join(build.profile.directory()))
}

fn allowed(build: &Build) -> Result<(), String> {
    let stable = matches!(build.channel, Channel::Stable);
    if stable && build.drive {
        return Err("the test driver is never built into the stable app".to_string());
    }
    if stable && build.config_dir.is_some() {
        return Err("the stable app always uses its own config directory".to_string());
    }
    Ok(())
}

fn cargo_command(program: OsString, build: &Build, target_dir: &Path) -> Command {
    let mut cargo = Command::new(program);
    // One cargo command, so the plugin program is always built for the app's channel.
    cargo.args([
        "build",
        "-p",
        APP_PACKAGE,
        "-p",
        RUNTIME_PACKAGE,
        "-p",
        PLUGIN_PACKAGE,
    ]);
    if build.drive {
        cargo.args(["--features", DRIVE_FEATURE]);
    }
    cargo.arg("--target-dir");
    cargo.arg(target_dir);
    match build.profile {
        Profile::Release => {
            cargo.arg("--release");
        }
        Profile::Debug => {}
    }
    // Each is set only when asked for, so a variable left in someone's shell cannot change what is built.
    match build.channel {
        Channel::Stable => {
            cargo.env(CHANNEL_VARIABLE, STABLE_CHANNEL);
        }
        Channel::Development => {
            cargo.env_remove(CHANNEL_VARIABLE);
        }
    }
    match build.config_dir {
        Some(directory) => {
            cargo.env(DEV_CONFIG_DIR_VARIABLE, directory);
        }
        None => {
            cargo.env_remove(DEV_CONFIG_DIR_VARIABLE);
        }
    }
    cargo
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn entry(command: &Command, variable: &str) -> Option<Option<OsString>> {
        let wanted = OsStr::new(variable);
        for (name, value) in command.get_envs() {
            if name == wanted {
                return Some(value.map(OsString::from));
            }
        }
        None
    }

    fn command_for(channel: Channel, config_dir: Option<&Path>) -> Command {
        let build = Build {
            profile: Profile::Debug,
            channel,
            drive: false,
            config_dir,
        };
        cargo_command(OsString::from("cargo"), &build, Path::new("target"))
    }

    #[test]
    fn a_build_carries_only_the_identity_it_was_asked_for() {
        let baked = Path::new("/tmp/rehearsal/config");
        let development = command_for(Channel::Development, None);
        let rehearsal = command_for(Channel::Development, Some(baked));
        let stable = command_for(Channel::Stable, None);

        assert_eq!(entry(&development, CHANNEL_VARIABLE), Some(None));
        assert_eq!(entry(&development, DEV_CONFIG_DIR_VARIABLE), Some(None));
        assert_eq!(
            entry(&rehearsal, DEV_CONFIG_DIR_VARIABLE),
            Some(Some(OsString::from(baked)))
        );
        assert_eq!(
            entry(&stable, CHANNEL_VARIABLE),
            Some(Some(OsString::from(STABLE_CHANNEL)))
        );
        assert_eq!(entry(&stable, DEV_CONFIG_DIR_VARIABLE), Some(None));
    }

    #[test]
    fn the_stable_app_is_never_built_with_the_driver_or_a_baked_config_directory() {
        let baked = Path::new("/tmp/rehearsal/config");
        let cases = [
            (Channel::Stable, true, None, false),
            (Channel::Stable, false, Some(baked), false),
            (Channel::Stable, false, None, true),
            (Channel::Development, true, Some(baked), true),
        ];
        for (channel, drive, config_dir, expected) in cases {
            let build = Build {
                profile: Profile::Release,
                channel,
                drive,
                config_dir,
            };
            let outcome = allowed(&build);
            assert_eq!(
                outcome.is_ok(),
                expected,
                "drive {drive}, config {config_dir:?}"
            );
        }
    }
}
