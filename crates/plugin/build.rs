const CHANNEL_VARIABLE: &str = "TACO_SHELLS_CHANNEL";

fn main() {
    println!("cargo::rerun-if-env-changed={CHANNEL_VARIABLE}");
    println!("cargo::rustc-check-cfg=cfg(taco_stable)");

    let channel = std::env::var(CHANNEL_VARIABLE);
    // Anything but exactly "stable" builds the dev app, so a build that lost the variable never replaces the app in daily use.
    let is_stable = channel.as_deref() == Ok("stable");
    if is_stable {
        println!("cargo::rustc-cfg=taco_stable");
    }
}
