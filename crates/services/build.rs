#[path = "src/build_identity.rs"]
mod build_identity;

const CHANNEL_VARIABLE: &str = "TACO_SHELLS_CHANNEL";
const DEV_CONFIG_VARIABLE: &str = "TACO_SHELLS_DEV_CONFIG_DIR";
const BAKED_CONFIG_VARIABLE: &str = "TACO_SHELLS_BAKED_CONFIG_DIR";

fn main() {
    println!("cargo::rerun-if-env-changed={CHANNEL_VARIABLE}");
    println!("cargo::rerun-if-env-changed={DEV_CONFIG_VARIABLE}");
    println!("cargo::rustc-check-cfg=cfg(taco_stable)");

    let channel = std::env::var(CHANNEL_VARIABLE);
    // Anything but exactly "stable" builds the dev app, so a build that lost the variable never replaces the app in daily use.
    let is_stable = channel.as_deref() == Ok("stable");
    if is_stable {
        println!("cargo::rustc-cfg=taco_stable");
    }

    let asked = std::env::var(DEV_CONFIG_VARIABLE).ok();
    let baked = build_identity::baked_config_directory(is_stable, asked.as_deref());
    let directory = match baked {
        Ok(Some(directory)) => directory,
        Ok(None) => String::new(),
        Err(problem) => {
            println!("cargo::error={problem}");
            return;
        }
    };
    // Always set, empty meaning none, so a variable of the same name in the shell can never reach option_env!.
    println!("cargo::rustc-env={BAKED_CONFIG_VARIABLE}={directory}");
}
