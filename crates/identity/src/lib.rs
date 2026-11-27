//! What the app's programs and its build tooling must agree on: each channel's names, the programs in a bundle and
//! where things sit inside it, the release asset names, and the variables passed between them. Which channel this
//! build is comes from `build.rs`, the only place that reads `TACO_SHELLS_CHANNEL`.

#[cfg(test)]
mod build_identity;
#[allow(dead_code)]
mod build_names;

use std::path::PathBuf;

pub use build_names::{CHANNEL_VARIABLE, DEV_CONFIG_DIR_VARIABLE, STABLE_CHANNEL};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Development,
    Stable,
}

/// One channel's names. The two never share a value: two bundles claiming one id leaves LaunchServices to pick, and a
/// shared config folder would let a dev build touch the app in daily use.
#[derive(Clone, Copy, Debug)]
pub struct Identity {
    pub channel: Channel,
    pub application_name: &'static str,
    pub bundle_id: &'static str,
    pub config_directory_name: &'static str,
    /// What the shells' `TACO_SHELLS_APP` holds, which is also the agent plugin's name.
    pub agent_app_name: &'static str,
}

pub const STABLE: Identity = Identity {
    channel: Channel::Stable,
    application_name: "Taco Shells",
    bundle_id: "com.ldelvoye.tacoshells",
    config_directory_name: ".taco-shells",
    agent_app_name: "taco-shells",
};

pub const DEVELOPMENT: Identity = Identity {
    channel: Channel::Development,
    application_name: "Taco Shells Dev",
    bundle_id: "com.ldelvoye.tacoshells.dev",
    config_directory_name: ".taco-shells-dev",
    agent_app_name: "taco-shells-dev",
};

#[cfg(taco_stable)]
pub const CURRENT: Identity = STABLE;
#[cfg(not(taco_stable))]
pub const CURRENT: Identity = DEVELOPMENT;

pub const IS_STABLE: bool = cfg!(taco_stable);

pub const APP_PROGRAM: &str = "taco-shells";
pub const RUNTIME_PROGRAM: &str = "taco-shells-runtime";
pub const PLUGIN_PROGRAM: &str = "taco-shells-plugin";
pub const PROGRAMS_PATH: &str = "Contents/MacOS";
pub const RESOURCES_PATH: &str = "Contents/Resources";
pub const INFO_PLIST_PATH: &str = "Contents/Info.plist";
pub const UPDATE_HELPER_PATH: &str = "Contents/Resources/update-helper.sh";

/// The public keys a release's signature may be made with, as `ssh-ed25519 AAAA… comment` lines. Two at most, so a
/// new key can be brought in by a release the old one signed.
pub const RELEASE_KEYS: &[&str] = &[
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAVX3dNSITcCxYYJrR/VyyQRxZ8nqjZvJGJddvrMFYkf taco-shells release",
];
/// The `ssh-keygen -Y` namespace and signer identity a release is signed under.
pub const RELEASE_SIGNING_NAMESPACE: &str = "taco-shells-release";

// A stable app with no key would refuse every update it is ever offered.
#[cfg(taco_stable)]
const _: () = assert!(
    !RELEASE_KEYS.is_empty(),
    "a stable build needs at least one release key"
);

/// Set to `1` by the tooling, the app runs with its window never shown and no Dock icon.
pub const BACKGROUND_WINDOW_VARIABLE: &str = "TACO_SHELLS_BACKGROUND_WINDOW";
/// Overrides the config folder at launch, in every build.
pub const CONFIG_DIRECTORY_VARIABLE: &str = "TACO_SHELLS_CONFIG_DIR";
/// A release JSON URL a development build checks in place of GitHub's; a stable build ignores it.
pub const UPDATE_FEED_VARIABLE: &str = "TACO_SHELLS_UPDATE_FEED";

/// The path of an extra public key a development build accepts an update signed with. None in a stable build, so
/// the stable binary does not even hold its name.
#[cfg(not(taco_stable))]
pub const UPDATE_KEY_VARIABLE: Option<&str> = Some("TACO_SHELLS_UPDATE_KEY");
#[cfg(taco_stable)]
pub const UPDATE_KEY_VARIABLE: Option<&str> = None;

const BAKED_CONFIG_DIRECTORY: &str = env!("TACO_SHELLS_BAKED_CONFIG_DIR");

/// `taco-shells-1.2.3-arm64.zip`: the zip a release carries, which the updater and `install.sh` look for.
pub fn release_zip_name(version: &str) -> String {
    format!("taco-shells-{version}-arm64.zip")
}

/// The signature published beside the zip.
pub fn release_signature_name(version: &str) -> String {
    let zip = release_zip_name(version);
    format!("{zip}.sig")
}

/// The config folder: `TACO_SHELLS_CONFIG_DIR` when set, then a folder a development build was compiled with, then
/// `~/<config_directory_name>`.
pub fn config_directory() -> Option<PathBuf> {
    let override_value = std::env::var_os(CONFIG_DIRECTORY_VARIABLE);
    if let Some(value) = override_value
        && !value.is_empty()
    {
        return Some(PathBuf::from(value));
    }

    if !BAKED_CONFIG_DIRECTORY.is_empty() {
        return Some(PathBuf::from(BAKED_CONFIG_DIRECTORY));
    }

    let home = std::env::home_dir()?;
    Some(home.join(CURRENT.config_directory_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_channels_share_no_name() {
        let pairs = [
            (STABLE.application_name, DEVELOPMENT.application_name),
            (STABLE.bundle_id, DEVELOPMENT.bundle_id),
            (
                STABLE.config_directory_name,
                DEVELOPMENT.config_directory_name,
            ),
            (STABLE.agent_app_name, DEVELOPMENT.agent_app_name),
        ];
        for (stable, development) in pairs {
            assert_ne!(stable, development);
        }
        assert_ne!(STABLE.channel, DEVELOPMENT.channel);
    }
}
