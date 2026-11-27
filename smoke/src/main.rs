//! The tests that drive the real app: one launch of a development bundle, the checks in order, stopping at the first
//! failure. Run through `cargo xtask smoke`, which builds the bundle and the config directory it starts from. With
//! `--stable`, only the checks a stable bundle can take, which is never launched here: `cargo xtask promote` runs them.

mod bundle;
mod plugin;
mod window;

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use taco_smoke::driver::Driver;
use taco_smoke::launch;

const USAGE: &str = "usage: taco-smoke <path to the .app> <config directory to start from> | taco-smoke --stable <path to the .app>";
const STABLE_FLAG: &str = "--stable";
const APP_LOG: &str = "app.log";

const STABLE_BUNDLE: [Check; 2] = [
    Check {
        name: "the stable bundle holds only the stable app's names, carries the release key and no extra-key variable",
        run: bundle::holds_only_stable_names_and_the_release_key,
    },
    Check {
        name: "the bundled plugin program is the stable app's: it lists note, now and share for the stable name, none for the dev one, and approves its own tool",
        run: plugin::is_built_for_the_stable_channel,
    },
];

const BEFORE_LAUNCH: [Check; 2] = [
    Check {
        name: "the dev bundle holds only the dev app's names",
        run: bundle::holds_only_dev_names,
    },
    Check {
        name: "the bundled plugin program is the dev app's: it lists note, now and share for the dev name, none for the stable one, and approves its own tool",
        run: plugin::is_built_for_the_dev_channel,
    },
];

const LAUNCHED: [LaunchedCheck; 10] = [
    LaunchedCheck {
        name: "a command typed into the terminal is answered on screen",
        run: window::a_typed_command_is_answered_on_screen,
    },
    LaunchedCheck {
        name: "a bound chord runs its command once and types nothing; an unbound one reaches the shell",
        run: window::chords_reach_their_command_or_the_shell,
    },
    LaunchedCheck {
        name: "the settings page keeps what is typed on it and gives the keyboard back to the terminal",
        run: window::the_settings_page_keeps_and_returns_the_keyboard,
    },
    LaunchedCheck {
        name: "a scrollback typed on the settings page is written to settings.json",
        run: window::a_typed_scrollback_is_written_to_the_settings_file,
    },
    LaunchedCheck {
        name: "a shortcut set by pressing keys on its row is written to keybindings.json",
        run: window::a_shortcut_set_by_pressing_keys_is_written_to_the_keybindings_file,
    },
    LaunchedCheck {
        name: "a closing page fades where it was and a click on it changes nothing",
        run: window::a_closing_page_stays_in_place_and_takes_no_clicks,
    },
    LaunchedCheck {
        name: "a row dragged down the list moves",
        run: window::a_dragged_row_moves,
    },
    LaunchedCheck {
        name: "a panel dragged to the right edge moves there",
        run: window::a_dragged_panel_moves_to_the_edge_it_is_dropped_on,
    },
    LaunchedCheck {
        name: "the window draws in light and in dark, and the two differ",
        run: window::draws_in_light_and_dark,
    },
    LaunchedCheck {
        name: "closing the last terminal leaves the app open, and a new one answers",
        run: window::the_app_outlives_its_last_terminal,
    },
];

/// What every check is given: the bundle under test and the throwaway config directory it starts from.
pub struct Smoke {
    pub app: PathBuf,
    pub config: PathBuf,
}

/// A check that reads the bundle and never launches the app.
struct Check {
    name: &'static str,
    run: fn(&Path) -> Result<(), String>,
}

struct LaunchedCheck {
    name: &'static str,
    run: fn(&Smoke, &mut Driver) -> Result<(), String>,
}

fn main() -> ExitCode {
    let arguments: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
    let (app, config) = match arguments.as_slice() {
        [flag, app] if flag == Path::new(STABLE_FLAG) => return checked_stable_bundle(app),
        [app, config] => (app, config),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let smoke = Smoke {
        app: app.clone(),
        config: config.clone(),
    };

    for check in BEFORE_LAUNCH {
        let outcome = (check.run)(&smoke.app);
        if !reported(check.name, outcome) {
            return ExitCode::FAILURE;
        }
    }

    let log = log_path(&smoke);
    let mut launched = match launch::launch(&smoke.app, &smoke.config, &log) {
        Ok(launched) => launched,
        Err(reason) => {
            println!(
                "FAIL  the app starts with its driver: {reason} (log: {})",
                log.display()
            );
            return ExitCode::FAILURE;
        }
    };
    let mut passed = true;
    for check in LAUNCHED {
        let outcome = (check.run)(&smoke, &mut launched.driver);
        if !reported(check.name, outcome) {
            passed = false;
            println!("      the app's log: {}", log.display());
            break;
        }
    }
    launch::quit(launched);

    if !passed {
        return ExitCode::FAILURE;
    }
    let total = BEFORE_LAUNCH.len() + LAUNCHED.len();
    println!("{total} passed");
    ExitCode::SUCCESS
}

fn checked_stable_bundle(app: &Path) -> ExitCode {
    for check in STABLE_BUNDLE {
        let outcome = (check.run)(app);
        if !reported(check.name, outcome) {
            return ExitCode::FAILURE;
        }
    }
    println!("{} passed", STABLE_BUNDLE.len());
    ExitCode::SUCCESS
}

fn reported(name: &str, outcome: Result<(), String>) -> bool {
    match outcome {
        Ok(()) => {
            println!("ok    {name}");
            true
        }
        Err(reason) => {
            println!("FAIL  {name}: {reason}");
            false
        }
    }
}

fn log_path(smoke: &Smoke) -> PathBuf {
    match smoke.config.parent() {
        Some(work) => work.join(APP_LOG),
        None => smoke.config.join(APP_LOG),
    }
}
