use std::env;
use std::ffi::OsString;
use std::process::Command;

use crate::repo;

pub fn run() -> Result<(), String> {
    let repo_root = repo::root()?;
    let cargo_path = env::var_os("CARGO");
    let program = match cargo_path {
        Some(path) => path,
        None => OsString::from("cargo"),
    };

    let mut cargo = Command::new(program);
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
