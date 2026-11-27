use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use taco_protocol::ConfigFile;
use taco_settings::{Config, resolve_config};

use crate::config_files::{config_file_path, read_config_files, seed_config_files};
use crate::sync::lock;

const WATCH_QUIET: Duration = Duration::from_millis(100);
const WATCH_LONGEST_WAIT: Duration = Duration::from_secs(1);

type OnChange = Box<dyn Fn(Config) + Send + Sync>;
type WatchEvents = Receiver<notify::Result<notify::Event>>;

struct State {
    config: Config,
    system_is_dark: bool,
}

struct Shared {
    directory: PathBuf,
    start_problems: Vec<String>,
    state: Mutex<State>,
    // Held from resolving to publishing, so two refreshes never publish out of order.
    refresh_order: Mutex<()>,
    on_change: OnChange,
}

pub struct ConfigStore {
    shared: Arc<Shared>,
    watcher: Option<RecommendedWatcher>,
    thread: Option<JoinHandle<()>>,
}

fn resolve(directory: &Path, start_problems: &[String], system_is_dark: bool) -> Config {
    let read = read_config_files(directory);
    let resolved = resolve_config(&read.texts, system_is_dark);

    let mut problems = start_problems.to_vec();
    problems.extend(read.unreadable);
    problems.extend(resolved.problems);
    Config {
        problems,
        ..resolved
    }
}

fn refresh(shared: &Shared) {
    let _order = lock(&shared.refresh_order);

    let system_is_dark = {
        let state = lock(&shared.state);
        state.system_is_dark
    };
    let next = resolve(&shared.directory, &shared.start_problems, system_is_dark);

    let changed = {
        let mut state = lock(&shared.state);
        if state.config == next {
            false
        } else {
            state.config = next.clone();
            true
        }
    };
    if changed {
        let on_change = &shared.on_change;
        on_change(next);
    }
}

fn wait_for_quiet(events: &WatchEvents) -> bool {
    let deadline = Instant::now() + WATCH_LONGEST_WAIT;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        let left = deadline - now;
        let wait = WATCH_QUIET.min(left);
        let next_event = events.recv_timeout(wait);
        match next_event {
            Ok(_) => continue,
            Err(RecvTimeoutError::Timeout) => return true,
            Err(RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn watch_loop(shared: &Shared, events: WatchEvents) {
    while events.recv().is_ok() {
        let still_watching = wait_for_quiet(&events);
        if !still_watching {
            return;
        }
        refresh(shared);
    }
}

fn symlink_target_directories(directory: &Path) -> Vec<PathBuf> {
    let mut targets: Vec<PathBuf> = Vec::new();
    for which in [ConfigFile::Settings, ConfigFile::Keybindings] {
        let path = config_file_path(directory, which);
        let entry = fs::symlink_metadata(&path);
        let is_symlink = match entry {
            Ok(metadata) => metadata.file_type().is_symlink(),
            Err(_) => false,
        };
        if !is_symlink {
            continue;
        }
        let Ok(target) = fs::canonicalize(&path) else {
            continue;
        };
        if let Some(parent) = target.parent() {
            targets.push(parent.to_path_buf());
        }
    }
    targets
}

fn watch_directory(
    directory: &Path,
    events: mpsc::Sender<notify::Result<notify::Event>>,
) -> Option<RecommendedWatcher> {
    let created = notify::recommended_watcher(events);
    let Ok(mut watcher) = created else {
        return None;
    };
    let watched = watcher.watch(directory, RecursiveMode::NonRecursive);
    if watched.is_err() {
        return None;
    }
    let targets = symlink_target_directories(directory);
    for target in targets {
        let _ = watcher.watch(&target, RecursiveMode::NonRecursive);
    }
    Some(watcher)
}

impl ConfigStore {
    /// The store; `on_change` runs on the watcher's thread, and inside `set_system_dark` on its caller's, so it must not call back into the store or drop it.
    pub fn start(directory: PathBuf, system_is_dark: bool, on_change: OnChange) -> ConfigStore {
        let mut start_problems = seed_config_files(&directory);

        let (sender, receiver) = mpsc::channel();
        let watcher = watch_directory(&directory, sender);
        if watcher.is_none() {
            start_problems.push(format!(
                "{} cannot be watched, so changes to the config files need a relaunch",
                directory.display()
            ));
        }

        let first = resolve(&directory, &start_problems, system_is_dark);
        let state = State {
            config: first,
            system_is_dark,
        };
        let shared = Arc::new(Shared {
            directory,
            start_problems,
            state: Mutex::new(state),
            refresh_order: Mutex::new(()),
            on_change,
        });

        let mut thread = None;
        if watcher.is_some() {
            let thread_shared = Arc::clone(&shared);
            let handle = thread::spawn(move || watch_loop(&thread_shared, receiver));
            thread = Some(handle);
        }
        ConfigStore {
            shared,
            watcher,
            thread,
        }
    }

    pub fn current(&self) -> Config {
        let state = lock(&self.shared.state);
        state.config.clone()
    }

    pub fn directory(&self) -> &Path {
        &self.shared.directory
    }

    pub fn set_system_dark(&self, is_dark: bool) {
        {
            let mut state = lock(&self.shared.state);
            state.system_is_dark = is_dark;
        }
        refresh(&self.shared);
    }

    pub fn refresh_now(&self) {
        refresh(&self.shared);
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        // The watcher goes first: dropping it closes the channel the thread is blocked on.
        self.watcher = None;
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ConfigStore {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_settings::Appearance;

    #[test]
    fn only_a_change_is_published_and_the_system_flag_reaches_the_theme() {
        let temp = tempfile::TempDir::new().unwrap();
        let published: Arc<Mutex<Vec<Appearance>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        let store = ConfigStore::start(
            temp.path().to_path_buf(),
            false,
            Box::new(move |config: Config| {
                let mut seen = lock(&sink);
                seen.push(config.appearance);
            }),
        );

        store.set_system_dark(false);
        store.set_system_dark(true);
        store.set_system_dark(true);

        let seen = lock(&published).clone();
        assert_eq!(seen, vec![Appearance::Dark]);
        assert_eq!(store.current().appearance, Appearance::Dark);
        store.stop();
    }
}
