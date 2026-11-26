use std::path::PathBuf;

pub const IS_STABLE: bool = cfg!(taco_stable);

#[cfg(taco_stable)]
pub const APPLICATION_NAME: &str = "Taco Shells";
#[cfg(not(taco_stable))]
pub const APPLICATION_NAME: &str = "Taco Shells Dev";

#[cfg(taco_stable)]
pub const CONFIG_DIRECTORY_NAME: &str = ".taco-shells";
#[cfg(not(taco_stable))]
pub const CONFIG_DIRECTORY_NAME: &str = ".taco-shells-dev";

pub const CONFIG_DIRECTORY_VARIABLE: &str = "TACO_SHELLS_CONFIG_DIR";

pub fn config_directory() -> Option<PathBuf> {
    let override_value = std::env::var_os(CONFIG_DIRECTORY_VARIABLE);
    if let Some(value) = override_value
        && !value.is_empty()
    {
        return Some(PathBuf::from(value));
    }

    let home = std::env::home_dir()?;
    Some(home.join(CONFIG_DIRECTORY_NAME))
}
