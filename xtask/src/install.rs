use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::{Channel, Profile};
use crate::bundle::{self, Options};
use crate::identity::DEVELOPMENT;
use crate::repo;

const INSTALL_DIR_VARIABLE: &str = "TACO_SHELLS_INSTALL_DIR";
const DEFAULT_INSTALL_DIR: &str = "/Applications";

pub fn run() -> Result<(), String> {
    let configured = env::var_os(INSTALL_DIR_VARIABLE);
    let install_dir = match configured {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(DEFAULT_INSTALL_DIR),
    };
    let installed = install_dir.join(format!("{}.app", DEVELOPMENT.product_name));

    if is_running(&installed)? {
        return Err(format!(
            "{} is running. Quit it and run this again.",
            installed.display()
        ));
    }

    let options = Options {
        profile: Profile::Release,
        channel: Channel::Development,
        zip: false,
        icon: true,
        drive: false,
        config_dir: None,
    };
    let bundled = bundle::create(&options)?;

    // Staged in the install folder so the final rename stays on one volume and cannot leave half an app.
    let staged = install_dir.join(format!(".taco-shells-install.{}", std::process::id()));
    let mut ditto = Command::new("ditto");
    ditto.arg(&bundled.app);
    ditto.arg(&staged);
    if let Err(error) = repo::run(&mut ditto) {
        discard(&staged);
        return Err(error);
    }

    let mut trashed = None;
    if installed.exists() {
        let trash_path = trash_path()?;
        if let Err(error) = fs::rename(&installed, &trash_path) {
            discard(&staged);
            return Err(format!(
                "could not move {} to {}: {error}",
                installed.display(),
                trash_path.display()
            ));
        }
        trashed = Some(trash_path);
    }

    if let Err(error) = fs::rename(&staged, &installed) {
        if let Some(trash_path) = trashed {
            let _ = fs::rename(&trash_path, &installed);
        }
        discard(&staged);
        return Err(format!(
            "could not move {} to {}: {error}",
            staged.display(),
            installed.display()
        ));
    }

    println!("installed {}", installed.display());
    Ok(())
}

pub(crate) fn is_running(installed: &Path) -> Result<bool, String> {
    let pattern = format!("{}/Contents/MacOS/", installed.display());
    let mut pgrep = Command::new("pgrep");
    pgrep.args(["-f", &pattern]);
    let output = pgrep
        .output()
        .map_err(|error| format!("could not check whether the app runs, pgrep: {error}"))?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!(
            "could not check whether the app runs, pgrep failed: {}",
            output.status
        )),
    }
}

fn trash_path() -> Result<PathBuf, String> {
    let home = match env::var_os("HOME") {
        Some(home) => home,
        None => return Err("HOME is not set".to_string()),
    };
    let mut date = Command::new("date");
    date.arg("+%Y%m%d-%H%M%S");
    let Some(timestamp) = repo::capture(&mut date) else {
        return Err("date failed".to_string());
    };
    let name = format!("{} {timestamp}.app", DEVELOPMENT.product_name);
    Ok(PathBuf::from(home).join(".Trash").join(name))
}

fn discard(staged: &Path) {
    let _ = fs::remove_dir_all(staged);
}
