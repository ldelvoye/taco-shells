use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::{Channel, Profile};
use crate::bundle::{self, Options};
use crate::repo;

const SMOKE_PACKAGE: &str = "taco-smoke";
const CONFIG_FILES: [&str; 2] = ["settings.json", "keybindings.json"];
const WORKSPACE_FILE: &str = "workspace.json";
const DIRECTORIES_PLACEHOLDER: &str = "@SMOKE_DIRECTORIES@";
const SAVED_DIRECTORIES: [&str; 3] = ["tacos", "tacos/docs", "notes"];

pub fn run() -> Result<(), String> {
    let options = Options {
        profile: Profile::Debug,
        channel: Channel::Development,
        zip: false,
        icon: false,
        drive: false,
    };
    let bundled = bundle::create(&options)?;

    let repo_root = repo::root()?;
    let target = repo::target_dir()?;
    let config = prepare_config(&repo_root, &target)?;

    let mut smoke = Command::new(repo::cargo_program());
    smoke.args(["run", "--quiet", "-p", SMOKE_PACKAGE, "--target-dir"]);
    smoke.arg(&target);
    smoke.arg("--");
    smoke.arg(&bundled.app);
    smoke.arg(&config);
    smoke.current_dir(&repo_root);
    repo::run(&mut smoke)
}

fn prepare_config(repo_root: &Path, target: &Path) -> Result<PathBuf, String> {
    let work = target.join("smoke.noindex");
    if work.exists() {
        fs::remove_dir_all(&work)
            .map_err(|error| format!("could not remove {}: {error}", work.display()))?;
    }
    let config = work.join("config");
    let directories = work.join("directories");
    create_dir(&config)?;
    for saved in SAVED_DIRECTORIES {
        create_dir(&directories.join(saved))?;
    }

    let fixtures = repo_root.join("smoke").join("fixtures").join("config");
    for file in CONFIG_FILES {
        let from = fixtures.join(file);
        let to = config.join(file);
        fs::copy(&from, &to)
            .map_err(|error| format!("could not copy {}: {error}", from.display()))?;
    }

    let workspace_fixture = fixtures.join(WORKSPACE_FILE);
    let template = fs::read_to_string(&workspace_fixture)
        .map_err(|error| format!("could not read {}: {error}", workspace_fixture.display()))?;
    let directories_text = directories.display().to_string();
    let workspace = template.replace(DIRECTORIES_PLACEHOLDER, &directories_text);
    let workspace_path = config.join(WORKSPACE_FILE);
    fs::write(&workspace_path, workspace)
        .map_err(|error| format!("could not write {}: {error}", workspace_path.display()))?;
    Ok(config)
}

fn create_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))
}
