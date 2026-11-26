use std::process::Command;

use crate::repo;

pub fn run() -> Result<(), String> {
    let repo_root = repo::root()?;
    let mut cargo = Command::new(repo::cargo_program());
    cargo.args([
        "run",
        "-p",
        "taco-ghostty",
        "--features",
        "bindings",
        "--bin",
        "ghostty-bindings",
    ]);
    cargo.current_dir(&repo_root);
    repo::run(&mut cargo)
}
