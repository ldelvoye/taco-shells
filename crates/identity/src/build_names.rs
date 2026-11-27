// Shared with build.rs, which reads these when the app's programs are compiled.

/// Set to `stable` for the build, by `cargo xtask bundle --stable` alone.
pub const CHANNEL_VARIABLE: &str = "TACO_SHELLS_CHANNEL";
pub const STABLE_CHANNEL: &str = "stable";
/// An absolute path a development build takes as its config directory.
pub const DEV_CONFIG_DIR_VARIABLE: &str = "TACO_SHELLS_DEV_CONFIG_DIR";
pub const BAKED_CONFIG_VARIABLE: &str = "TACO_SHELLS_BAKED_CONFIG_DIR";
