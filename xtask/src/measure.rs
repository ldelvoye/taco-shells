use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::{Channel, Profile};
use crate::bundle::{self, Options};
use crate::repo;

const MEASURE_PACKAGE: &str = "taco-smoke";
const MEASURE_BINARY: &str = "taco-measure";
const SETTINGS: &str = "{ \"scrollback\": 100000, \"restoreOnLaunch\": false }\n";

pub fn run(flags: &[String]) -> Result<(), String> {
    let options = Options {
        profile: Profile::Release,
        channel: Channel::Development,
        zip: false,
        icon: false,
        drive: true,
        config_dir: None,
    };
    let bundled = bundle::create(&options)?;

    let repo_root = repo::root()?;
    let target = repo::target_dir()?;
    let work = prepare_work(&target)?;
    let report = report_path(&repo_root, &work)?;

    let mut measure = Command::new(repo::cargo_program());
    measure.args(["run", "--release", "--quiet", "-p", MEASURE_PACKAGE]);
    measure.args(["--bin", MEASURE_BINARY, "--target-dir"]);
    measure.arg(&target);
    measure.arg("--");
    measure.arg(&bundled.app);
    measure.arg(&work);
    measure.arg(&report);
    measure.args(flags);
    measure.current_dir(&repo_root);
    repo::run(&mut measure)
}

fn prepare_work(target: &Path) -> Result<PathBuf, String> {
    let work = target.join("measure.noindex");
    if work.exists() {
        fs::remove_dir_all(&work)
            .map_err(|error| format!("could not remove {}: {error}", work.display()))?;
    }
    let config = work.join("config");
    fs::create_dir_all(&config)
        .map_err(|error| format!("could not create {}: {error}", config.display()))?;

    let settings = config.join("settings.json");
    fs::write(&settings, SETTINGS)
        .map_err(|error| format!("could not write {}: {error}", settings.display()))?;
    Ok(work)
}

fn report_path(repo_root: &Path, work: &Path) -> Result<PathBuf, String> {
    let mut date_command = Command::new("date");
    date_command.arg("+%F");
    let date = match repo::capture(&mut date_command) {
        Some(date) => date,
        None => return Err("could not read the date".to_string()),
    };

    let mut git = Command::new("git");
    git.args(["rev-parse", "--short", "HEAD"]);
    git.current_dir(repo_root);
    let commit = match repo::capture(&mut git) {
        Some(commit) => commit,
        None => return Err("could not read the current commit".to_string()),
    };

    Ok(work.join(format!("{date}-{commit}.md")))
}
