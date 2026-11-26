use std::process::Command;

use crate::build::{Channel, Profile};
use crate::bundle::{self, Options};
use crate::repo;

const SMOKE_PACKAGE: &str = "taco-smoke";

pub fn run() -> Result<(), String> {
    let options = Options {
        profile: Profile::Debug,
        channel: Channel::Development,
        zip: false,
        icon: false,
    };
    let bundled = bundle::create(&options)?;

    let repo_root = repo::root()?;
    let target = repo::target_dir()?;
    let mut smoke = Command::new(repo::cargo_program());
    smoke.args(["run", "--quiet", "-p", SMOKE_PACKAGE, "--target-dir"]);
    smoke.arg(&target);
    smoke.arg("--");
    smoke.arg(&bundled.app);
    smoke.current_dir(&repo_root);
    repo::run(&mut smoke)
}
