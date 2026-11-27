use std::env;
use std::fs::{self, File};
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use taco_identity::{APP_PROGRAM, CONFIG_DIRECTORY_VARIABLE, PROGRAMS_PATH, STABLE};

use crate::build::{Channel, Profile};
use crate::bundle::{self, Options};
use crate::repo;

const CLAUDE_CONFIG_VARIABLE: &str = "CLAUDE_CONFIG_DIR";
const CLAUDE_FOLDER: &str = ".claude";
const CLAUDE_SETTINGS: &str = "settings.json";
// Left out of the copy: a pending update, and the running app's port and token, which belong to it alone.
const NOT_COPIED: [&str; 3] = ["updates", "focus-port", "focus-token"];
const APP_LOG: &str = "app.log";
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_POLL: Duration = Duration::from_millis(100);
const NOT_A_TERMINAL: &str = "try-stable opens the stable app and waits for you to press Enter, but there is no terminal to read that from; run it in a terminal window";

/// Opens the stable build on copies of `~/.taco-shells` and Claude Code's `settings.json` to try before promoting; the
/// bundle is deleted afterwards, so LaunchServices is never left with a second copy of the stable app.
pub fn run() -> Result<(), String> {
    if !io::stdin().is_terminal() {
        return Err(NOT_A_TERMINAL.to_string());
    }
    let Some(home) = env::home_dir() else {
        return Err("there is no home folder to copy from".to_string());
    };

    let target = repo::target_dir()?;
    let work = target.join("try-stable.noindex");
    if work.exists() {
        remove_dir(&work)?;
    }
    let config = work.join("config");
    let claude = work.join("claude");
    copy_config(&home.join(STABLE.config_directory_name), &config)?;
    copy_claude_settings(&home.join(CLAUDE_FOLDER), &claude)?;

    let options = Options {
        profile: Profile::Release,
        channel: Channel::Stable,
        zip: false,
        icon: true,
        drive: false,
        config_dir: None,
    };
    let bundled = bundle::create(&options)?;

    let launched = launch(&bundled.app, &config, &claude, &work);
    let mut child = match launched {
        Ok(child) => child,
        Err(error) => {
            remove_bundle(&bundled.app);
            return Err(error);
        }
    };
    println!(
        "{} is open on copies of your folders:",
        STABLE.application_name
    );
    println!("  its config folder   {}", config.display());
    println!("  Claude Code's       {}", claude.display());
    println!(
        "Your real ~/{} and ~/{CLAUDE_FOLDER} are not touched.",
        STABLE.config_directory_name
    );
    println!("Look it over, then press Enter here to quit it.");
    let mut answer = String::new();
    let _ = io::stdin().read_line(&mut answer);

    end(&mut child);
    remove_bundle(&bundled.app);
    println!(
        "Quit, and the bundle is removed. The copies stay in {} until the next run.",
        work.display()
    );
    Ok(())
}

fn copy_config(real: &Path, copy: &Path) -> Result<(), String> {
    if !real.exists() {
        println!(
            "There is no {} yet, so the app starts on an empty folder.",
            real.display()
        );
        return create_dir(copy);
    }
    let mut ditto = Command::new("ditto");
    ditto.arg(real);
    ditto.arg(copy);
    repo::run(&mut ditto)?;
    for name in NOT_COPIED {
        let path = copy.join(name);
        if path.is_dir() {
            remove_dir(&path)?;
        } else if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn copy_claude_settings(real: &Path, copy: &Path) -> Result<(), String> {
    create_dir(copy)?;
    let settings = real.join(CLAUDE_SETTINGS);
    if !settings.is_file() {
        return Ok(());
    }
    let copied = copy.join(CLAUDE_SETTINGS);
    fs::copy(&settings, &copied)
        .map_err(|error| format!("could not copy {}: {error}", settings.display()))?;
    Ok(())
}

fn launch(app: &Path, config: &Path, claude: &Path, work: &Path) -> Result<Child, String> {
    let executable = app.join(PROGRAMS_PATH).join(APP_PROGRAM);
    let log_path = work.join(APP_LOG);
    let log = File::create(&log_path)
        .map_err(|error| format!("could not create {}: {error}", log_path.display()))?;
    let error_log = log
        .try_clone()
        .map_err(|error| format!("could not open {}: {error}", log_path.display()))?;
    let mut command = Command::new(&executable);
    command.env(CONFIG_DIRECTORY_VARIABLE, config);
    command.env(CLAUDE_CONFIG_VARIABLE, claude);
    command.stdout(Stdio::from(log));
    command.stderr(Stdio::from(error_log));
    command
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", executable.display()))
}

fn end(child: &mut Child) {
    let mut terminate = Command::new("kill");
    terminate.args(["-TERM", &child.id().to_string()]);
    let _ = terminate.status();
    let started = Instant::now();
    while started.elapsed() < STOP_TIMEOUT {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(STOP_POLL),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn remove_bundle(app: &Path) {
    if let Err(error) = remove_dir(app) {
        eprintln!("{error}; delete {} by hand", app.display());
    }
}

fn remove_dir(path: &Path) -> Result<(), String> {
    fs::remove_dir_all(path)
        .map_err(|error| format!("could not remove {}: {error}", path.display()))
}

fn create_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))
}
