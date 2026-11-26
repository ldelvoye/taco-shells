use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::identity::IS_STABLE;
use crate::mac;
use crate::sync::lock;

pub(crate) const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
pub(crate) const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
// Past the 60 seconds GitHub's shared cache can keep serving the previous release.
pub(crate) const RECHECK_DELAY: Duration = Duration::from_secs(75);

const DEFAULT_FEED_URL: &str = "https://api.github.com/repos/ldelvoye/taco-shells/releases/latest";
const FEED_VARIABLE: &str = "TACO_SHELLS_UPDATE_FEED";
const USER_AGENT: &str = "taco-shells";
const FEED_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(120);
const UPDATES_DIRECTORY_NAME: &str = "updates";
const ZIP_NAME: &str = "taco-shells.zip";
const STAGING_SUFFIX: &str = ".partial";
const APP_SUFFIX: &str = ".app";
const TAG_PREFIX: &str = "v";
const HELPER_PATH: &str = "Contents/Resources/update-helper.sh";
const UNVERSIONED: &str = "0.0.0";
const NOT_IN_APPLICATIONS: &str = "Taco Shells can only update itself from /Applications.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateEvent {
    Ready(String),
    Problem(String),
}

pub struct UpdaterHost {
    pub executable: PathBuf,
    // Called on the updater's thread, and once on the caller's inside start for a download kept from before.
    pub on_event: Box<dyn Fn(UpdateEvent) + Send + Sync>,
    pub quit: Box<dyn Fn() + Send + Sync>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseInfo {
    pub(crate) version: String,
    pub(crate) asset_url: String,
    pub(crate) asset_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadyApp {
    pub(crate) version: String,
    pub(crate) path: PathBuf,
}

pub(crate) trait Feed: Send + Sync {
    fn latest(&self) -> Result<serde_json::Value, String>;
    fn download(&self, url: &str, to: &Path) -> Result<u64, String>;
}

pub(crate) struct HttpFeed {
    pub(crate) feed_url: String,
}

pub(crate) struct Schedule {
    periodic: Instant,
    extra: Vec<Instant>,
}

enum Message {
    CheckNow,
    Stop,
}

enum Applying {
    NothingReady,
    Refused(String),
    Started,
}

struct Shared {
    ready: Mutex<Option<ReadyApp>>,
    applying: AtomicBool,
    stopping: AtomicBool,
    host: UpdaterHost,
}

pub struct Updater {
    shared: Arc<Shared>,
    sender: Option<Sender<Message>>,
    thread: Option<JoinHandle<()>>,
}

fn version_parts(text: &str) -> Option<[u64; 3]> {
    let pieces: Vec<&str> = text.split('.').collect();
    if pieces.len() != 3 {
        return None;
    }
    let mut parts = [0u64; 3];
    for (slot, piece) in parts.iter_mut().zip(pieces) {
        if piece.is_empty() || !piece.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let parsed = piece.parse::<u64>();
        let Ok(number) = parsed else {
            return None;
        };
        *slot = number;
    }
    Some(parts)
}

pub(crate) fn is_version(text: &str) -> bool {
    version_parts(text).is_some()
}

pub(crate) fn is_newer_version(candidate: &str, current: &str) -> bool {
    let Some(candidate_parts) = version_parts(candidate) else {
        return false;
    };
    let Some(current_parts) = version_parts(current) else {
        return false;
    };
    candidate_parts > current_parts
}

// Must match the zip name xtask produces, or no release ever offers an update.
fn asset_name_for(version: &str) -> String {
    format!("taco-shells-{version}-arm64.zip")
}

pub(crate) fn parse_release(payload: &serde_json::Value) -> Option<ReleaseInfo> {
    let release = payload.as_object()?;
    let prerelease = release.get("prerelease");
    if prerelease == Some(&serde_json::Value::Bool(true)) {
        return None;
    }
    let tag_value = release.get("tag_name")?;
    let tag = tag_value.as_str()?;
    let version = tag.strip_prefix(TAG_PREFIX)?;
    if !is_version(version) {
        return None;
    }
    let assets_value = release.get("assets")?;
    let assets = assets_value.as_array()?;
    let wanted = asset_name_for(version);
    for asset in assets {
        let Some(name_value) = asset.get("name") else {
            continue;
        };
        let Some(name) = name_value.as_str() else {
            continue;
        };
        if name != wanted {
            continue;
        }
        let url_value = asset.get("browser_download_url")?;
        let url = url_value.as_str()?;
        let size_value = asset.get("size")?;
        let size = size_value.as_u64()?;
        if size == 0 {
            return None;
        }
        return Some(ReleaseInfo {
            version: version.to_string(),
            asset_url: url.to_string(),
            asset_size: size,
        });
    }
    None
}

impl HttpFeed {
    fn agent(timeout: Duration) -> ureq::Agent {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .user_agent(USER_AGENT)
            .build();
        ureq::Agent::new_with_config(config)
    }
}

impl Feed for HttpFeed {
    fn latest(&self) -> Result<serde_json::Value, String> {
        let agent = HttpFeed::agent(FEED_TIMEOUT);
        let request = agent.get(&self.feed_url);
        let called = request.call();
        let mut response = called.map_err(|error| error.to_string())?;
        let body = response.body_mut();
        let read = body.read_to_string();
        let text = read.map_err(|error| error.to_string())?;
        let parsed = serde_json::from_str(&text);
        parsed.map_err(|error| error.to_string())
    }

    fn download(&self, url: &str, to: &Path) -> Result<u64, String> {
        let agent = HttpFeed::agent(DOWNLOAD_TIMEOUT);
        let request = agent.get(url);
        let called = request.call();
        let response = called.map_err(|error| error.to_string())?;
        let body = response.into_body();
        let mut reader = body.into_reader();
        let created = File::create(to);
        let mut file = created.map_err(|error| error.to_string())?;
        let copied = io::copy(&mut reader, &mut file);
        copied.map_err(|error| error.to_string())
    }
}

fn remove_path(path: &Path) {
    let metadata = fs::symlink_metadata(path);
    let Ok(metadata) = metadata else {
        return;
    };
    if metadata.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn child_names(directory: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(text) = name.to_str() {
            names.push(text.to_string());
        }
    }
    names
}

fn app_in(directory: &Path) -> Option<PathBuf> {
    let names = child_names(directory);
    let apps: Vec<&String> = names
        .iter()
        .filter(|name| name.ends_with(APP_SUFFIX))
        .collect();
    if apps.len() != 1 {
        return None;
    }
    Some(directory.join(apps[0]))
}

fn remove_other_versions(updates: &Path, keep: &str) {
    for name in child_names(updates) {
        if name == keep {
            continue;
        }
        remove_path(&updates.join(name));
    }
}

pub(crate) fn prune_and_adopt(updates: &Path, current: &str) -> Option<ReadyApp> {
    let mut adopted: Option<ReadyApp> = None;
    for name in child_names(updates) {
        let child = updates.join(&name);
        if !is_newer_version(&name, current) {
            remove_path(&child);
            continue;
        }
        let Some(path) = app_in(&child) else {
            remove_path(&child);
            continue;
        };
        let is_newest = match &adopted {
            Some(ready) => is_newer_version(&name, &ready.version),
            None => true,
        };
        if is_newest {
            adopted = Some(ReadyApp {
                version: name,
                path,
            });
        }
    }
    if let Some(newest) = &adopted {
        remove_other_versions(updates, &newest.version);
    }
    adopted
}

fn fetch_and_unpack(release: &ReleaseInfo, staging: &Path, feed: &dyn Feed) -> Option<()> {
    remove_path(staging);
    let created = fs::create_dir_all(staging);
    if created.is_err() {
        return None;
    }

    let zip = staging.join(ZIP_NAME);
    let downloaded = feed.download(&release.asset_url, &zip);
    let Ok(bytes) = downloaded else {
        return None;
    };
    if bytes != release.asset_size {
        return None;
    }

    let extracted = mac::extract_zip(&zip, staging, EXTRACT_TIMEOUT);
    if extracted.is_err() {
        return None;
    }
    let _ = fs::remove_file(&zip);
    app_in(staging)?;
    Some(())
}

pub(crate) fn download_and_extract(
    release: &ReleaseInfo,
    updates: &Path,
    feed: &dyn Feed,
) -> Option<PathBuf> {
    let version_directory = updates.join(&release.version);
    // Unpacked under a name that is not a version, so a crash midway leaves nothing a later launch could adopt.
    let staging_name = format!("{}{STAGING_SUFFIX}", release.version);
    let staging = updates.join(staging_name);

    let unpacked = fetch_and_unpack(release, &staging, feed);
    if unpacked.is_none() {
        remove_path(&staging);
        return None;
    }

    remove_path(&version_directory);
    let renamed = fs::rename(&staging, &version_directory);
    if renamed.is_err() {
        remove_path(&staging);
        return None;
    }
    app_in(&version_directory)
}

impl Schedule {
    pub(crate) fn new(started: Instant) -> Schedule {
        Schedule {
            periodic: started + FIRST_CHECK_DELAY,
            extra: Vec::new(),
        }
    }

    pub(crate) fn check_now(&mut self, now: Instant) {
        self.extra.push(now);
        self.recheck_later(now);
    }

    pub(crate) fn recheck_later(&mut self, now: Instant) {
        self.extra.push(now + RECHECK_DELAY);
    }

    pub(crate) fn due(&mut self, now: Instant) -> bool {
        let mut is_due = false;
        if now >= self.periodic {
            self.periodic = now + CHECK_INTERVAL;
            is_due = true;
        }
        let before = self.extra.len();
        self.extra.retain(|at| *at > now);
        if self.extra.len() < before {
            is_due = true;
        }
        is_due
    }

    pub(crate) fn next_wake(&self) -> Instant {
        let mut wake = self.periodic;
        for at in &self.extra {
            if *at < wake {
                wake = *at;
            }
        }
        wake
    }
}

fn feed_for(is_stable: bool, override_url: Option<String>, current: &str) -> Option<String> {
    if is_stable {
        // A stable build that does not know its own version would take any release as newer.
        if current == UNVERSIONED {
            return None;
        }
        return Some(DEFAULT_FEED_URL.to_string());
    }
    let url = override_url?;
    if url.is_empty() {
        return None;
    }
    Some(url)
}

fn current_version(executable: &Path) -> String {
    let version = mac::bundle_version(executable);
    match version {
        Some(version) => version,
        None => UNVERSIONED.to_string(),
    }
}

fn emit(shared: &Shared, event: UpdateEvent) {
    if shared.stopping.load(Ordering::SeqCst) {
        return;
    }
    let on_event = &shared.host.on_event;
    on_event(event);
}

fn begin_apply(shared: &Shared, slot: &mut Option<ReadyApp>) -> Applying {
    let Some(ready) = slot.clone() else {
        return Applying::NothingReady;
    };
    let Some(installed) = mac::installed_bundle(&shared.host.executable) else {
        return Applying::Refused(NOT_IN_APPLICATIONS.to_string());
    };
    if !ready.path.is_dir() {
        *slot = None;
        let message = "The update could not be started: the downloaded update is missing";
        return Applying::Refused(message.to_string());
    }
    let helper = installed.join(HELPER_PATH);
    if !helper.is_file() {
        let message = "The update could not be started: the update helper is missing";
        return Applying::Refused(message.to_string());
    }

    shared.applying.store(true, Ordering::SeqCst);
    let pid = process::id();
    let spawned = mac::spawn_update_helper(&helper, pid, &installed, &ready.path);
    if let Err(error) = spawned {
        shared.applying.store(false, Ordering::SeqCst);
        return Applying::Refused(format!("Taco Shells could not start the update: {error}"));
    }
    Applying::Started
}

fn run_check(shared: &Shared, updates: &Path, current: &str, feed: &dyn Feed) {
    let Ok(payload) = feed.latest() else {
        return;
    };
    let Some(release) = parse_release(&payload) else {
        return;
    };
    if !is_newer_version(&release.version, current) {
        return;
    }
    let ready_version = {
        let ready = lock(&shared.ready);
        ready.as_ref().map(|app| app.version.clone())
    };
    if let Some(version) = ready_version
        && !is_newer_version(&release.version, &version)
    {
        return;
    }

    let Some(path) = download_and_extract(&release, updates, feed) else {
        return;
    };
    {
        let mut ready = lock(&shared.ready);
        // Swapped and pruned under the lock apply_ready holds, and not at all once an apply has started, so the folder it hands the helper is never removed.
        let is_applying = shared.applying.load(Ordering::SeqCst);
        if is_applying {
            return;
        }
        *ready = Some(ReadyApp {
            version: release.version.clone(),
            path,
        });
        remove_other_versions(updates, &release.version);
    }
    emit(shared, UpdateEvent::Ready(release.version));
}

// A check asked for while one runs is folded into it, but its 75-second recheck still has to come, or a release just promoted waits for the next six-hourly check.
fn drain_after_check(messages: &Receiver<Message>, schedule: &mut Schedule) -> bool {
    while let Ok(message) = messages.try_recv() {
        match message {
            Message::Stop => return false,
            Message::CheckNow => {
                let now = Instant::now();
                schedule.recheck_later(now);
            }
        }
    }
    true
}

fn run_thread(
    shared: Arc<Shared>,
    updates: PathBuf,
    current: String,
    feed: Box<dyn Feed>,
    messages: Receiver<Message>,
    started: Instant,
) {
    let mut schedule = Schedule::new(started);
    loop {
        let wake = schedule.next_wake();
        let before_wait = Instant::now();
        let wait = wake.saturating_duration_since(before_wait);
        match messages.recv_timeout(wait) {
            Ok(Message::CheckNow) => {
                let asked_at = Instant::now();
                schedule.check_now(asked_at);
            }
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if !schedule.due(now) {
            continue;
        }
        run_check(&shared, &updates, &current, feed.as_ref());
        if !drain_after_check(&messages, &mut schedule) {
            return;
        }
    }
}

impl Updater {
    pub fn start(config_directory: &Path, host: UpdaterHost) -> Updater {
        let variable = std::env::var(FEED_VARIABLE);
        let override_url = variable.ok();
        let current = current_version(&host.executable);
        let feed_url = feed_for(IS_STABLE, override_url, &current);
        let Some(feed_url) = feed_url else {
            return Updater::idle(host);
        };

        let updates = config_directory.join(UPDATES_DIRECTORY_NAME);
        let feed = HttpFeed { feed_url };
        Updater::start_with(updates, current, Box::new(feed), host, Instant::now())
    }

    pub(crate) fn start_with(
        updates: PathBuf,
        current: String,
        feed: Box<dyn Feed>,
        host: UpdaterHost,
        started: Instant,
    ) -> Updater {
        let adopted = prune_and_adopt(&updates, &current);
        let adopted_version = adopted.as_ref().map(|app| app.version.clone());
        let shared = Arc::new(Shared {
            ready: Mutex::new(adopted),
            applying: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            host,
        });
        if let Some(version) = adopted_version {
            emit(&shared, UpdateEvent::Ready(version));
        }

        let (sender, receiver) = mpsc::channel();
        let thread_shared = Arc::clone(&shared);
        let handle = thread::spawn(move || {
            run_thread(thread_shared, updates, current, feed, receiver, started);
        });
        Updater {
            shared,
            sender: Some(sender),
            thread: Some(handle),
        }
    }

    fn idle(host: UpdaterHost) -> Updater {
        let shared = Shared {
            ready: Mutex::new(None),
            applying: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            host,
        };
        Updater {
            shared: Arc::new(shared),
            sender: None,
            thread: None,
        }
    }

    pub fn check_now(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(Message::CheckNow);
        }
    }

    pub fn ready_version(&self) -> Option<String> {
        let ready = lock(&self.shared.ready);
        ready.as_ref().map(|app| app.version.clone())
    }

    pub fn apply_ready(&self) {
        let outcome = {
            let mut slot = lock(&self.shared.ready);
            begin_apply(&self.shared, &mut slot)
        };
        match outcome {
            Applying::NothingReady => {}
            Applying::Refused(problem) => emit(&self.shared, UpdateEvent::Problem(problem)),
            Applying::Started => {
                let quit = &self.shared.host.quit;
                quit();
            }
        }
    }

    pub fn stop(mut self) {
        self.shut_down();
    }

    fn shut_down(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(Message::Stop);
        }
        // Not joined: a check can be mid-download for many minutes, and quitting must not wait for it.
        let detached = self.thread.take();
        drop(detached);
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.shut_down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::process::Command;
    use std::sync::atomic::AtomicUsize;

    struct ZipFeed {
        release: Arc<Mutex<serde_json::Value>>,
        zip: PathBuf,
        latest_calls: Mutex<Sender<()>>,
        downloads: Arc<AtomicUsize>,
    }

    impl Feed for ZipFeed {
        fn latest(&self) -> Result<serde_json::Value, String> {
            let calls = lock(&self.latest_calls);
            let _ = calls.send(());
            let release = lock(&self.release);
            Ok(release.clone())
        }

        fn download(&self, _url: &str, to: &Path) -> Result<u64, String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            let copied = fs::copy(&self.zip, to);
            copied.map_err(|error| error.to_string())
        }
    }

    fn zip_feed(release: serde_json::Value, zip: &Path, latest_calls: Sender<()>) -> ZipFeed {
        ZipFeed {
            release: Arc::new(Mutex::new(release)),
            zip: zip.to_path_buf(),
            latest_calls: Mutex::new(latest_calls),
            downloads: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn recording_host(
        executable: PathBuf,
    ) -> (UpdaterHost, Receiver<UpdateEvent>, Arc<AtomicBool>) {
        let (events, received) = mpsc::channel();
        let quit_called = Arc::new(AtomicBool::new(false));
        let quit_flag = Arc::clone(&quit_called);
        let host = UpdaterHost {
            executable,
            on_event: Box::new(move |event| {
                let _ = events.send(event);
            }),
            quit: Box::new(move || quit_flag.store(true, Ordering::SeqCst)),
        };
        (host, received, quit_called)
    }

    fn long_ago() -> Instant {
        let now = Instant::now();
        match now.checked_sub(Duration::from_secs(60)) {
            Some(earlier) => earlier,
            None => now,
        }
    }

    fn release_payload(tag: &str, asset_name: &str, size: serde_json::Value) -> serde_json::Value {
        json!({
            "tag_name": tag,
            "prerelease": false,
            "assets": [
                { "name": "other.dmg", "size": 5, "browser_download_url": "https://example.test/other" },
                { "name": asset_name, "size": size, "browser_download_url": "https://example.test/zip" }
            ]
        })
    }

    fn make_app(directory: &Path, name: &str) -> PathBuf {
        let app = directory.join(name);
        let contents = app.join("Contents");
        fs::create_dir_all(&contents).unwrap();
        let file = contents.join("file");
        fs::write(file, "x").unwrap();
        app
    }

    fn make_zip(temp: &Path, app_names: &[&str]) -> PathBuf {
        let source = temp.join("zip-source");
        fs::create_dir_all(&source).unwrap();
        let zip = temp.join("release.zip");
        let mut command = Command::new("ditto");
        command.arg("-c").arg("-k");
        if let [only] = app_names {
            let app = make_app(&source, only);
            command.arg("--keepParent").arg(app);
        } else {
            for name in app_names {
                make_app(&source, name);
            }
            command.arg(&source);
        }
        let status = command.arg(&zip).status().unwrap();
        assert!(status.success());
        zip
    }

    fn file_size(path: &Path) -> u64 {
        let metadata = fs::metadata(path).unwrap();
        metadata.len()
    }

    fn release_info(version: &str, size: u64) -> ReleaseInfo {
        ReleaseInfo {
            version: version.to_string(),
            asset_url: "https://example.test/zip".to_string(),
            asset_size: size,
        }
    }

    #[test]
    fn a_release_is_taken_only_when_it_is_a_stable_plain_version_with_its_zip() {
        let good = release_payload("v1.2.3", "taco-shells-1.2.3-arm64.zip", json!(1234));
        let parsed = parse_release(&good);
        let expected = ReleaseInfo {
            version: "1.2.3".to_string(),
            asset_url: "https://example.test/zip".to_string(),
            asset_size: 1234,
        };
        assert_eq!(parsed, Some(expected));

        let mut prerelease = good.clone();
        prerelease["prerelease"] = json!(true);
        let refused = [
            prerelease,
            release_payload("1.2.3", "taco-shells-1.2.3-arm64.zip", json!(1234)),
            release_payload("v1.2", "taco-shells-1.2-arm64.zip", json!(1234)),
            release_payload(
                "v1.2.3-beta",
                "taco-shells-1.2.3-beta-arm64.zip",
                json!(1234),
            ),
            release_payload("v1.2.3", "taco-shells-1.2.4-arm64.zip", json!(1234)),
            release_payload("v1.2.3", "taco-shells-1.2.3-arm64.zip", json!(0)),
            release_payload("v1.2.3", "taco-shells-1.2.3-arm64.zip", json!(null)),
        ];
        for payload in refused {
            assert_eq!(parse_release(&payload), None, "{payload}");
        }

        let text = json!("nope");
        assert_eq!(parse_release(&text), None);
        let numbers = json!([1, 2]);
        assert_eq!(parse_release(&numbers), None);
    }

    #[test]
    fn versions_compare_numerically_and_only_when_plain() {
        assert!(is_newer_version("0.10.0", "0.9.9"));
        assert!(!is_newer_version("0.9.9", "0.10.0"));
        assert!(!is_newer_version("1.2.3", "1.2.3"));
        assert!(!is_newer_version("1.2.4-beta", "1.2.3"));
        assert!(!is_newer_version("1.2.3", "1.2.4-beta"));
        assert!(!is_newer_version("1.2", "1.1.0"));
    }

    #[test]
    fn a_stable_build_always_uses_github_and_a_dev_build_only_an_override() {
        let url = "http://127.0.0.1:1/feed".to_string();
        let github = Some(DEFAULT_FEED_URL.to_string());
        let empty = Some(String::new());

        let stable_ignores_override = feed_for(true, Some(url.clone()), "1.0.0");
        assert_eq!(stable_ignores_override, github);
        let stable_unversioned = feed_for(true, None, UNVERSIONED);
        assert_eq!(stable_unversioned, None);
        let dev_without_override = feed_for(false, None, "1.0.0");
        assert_eq!(dev_without_override, None);
        let dev_with_empty_override = feed_for(false, empty, "1.0.0");
        assert_eq!(dev_with_empty_override, None);
        let dev_with_override = feed_for(false, Some(url.clone()), "1.0.0");
        assert_eq!(dev_with_override, Some(url));
    }

    #[test]
    fn the_schedule_checks_at_ten_seconds_then_every_six_hours_and_twice_on_request() {
        let start = Instant::now();
        let mut schedule = Schedule::new(start);
        assert!(!schedule.due(start + Duration::from_secs(9)));
        assert!(schedule.due(start + FIRST_CHECK_DELAY));
        assert!(!schedule.due(start + Duration::from_secs(3600)));
        assert!(schedule.due(start + FIRST_CHECK_DELAY + CHECK_INTERVAL));

        let mut schedule = Schedule::new(start);
        assert!(schedule.due(start + FIRST_CHECK_DELAY));
        let asked = start + Duration::from_secs(100);
        schedule.check_now(asked);
        assert!(schedule.due(asked));
        let almost = RECHECK_DELAY - Duration::from_secs(1);
        assert!(!schedule.due(asked + almost));
        assert!(schedule.due(asked + RECHECK_DELAY));
        assert!(!schedule.due(asked + RECHECK_DELAY + Duration::from_secs(1)));
        assert!(schedule.due(start + FIRST_CHECK_DELAY + CHECK_INTERVAL));

        let (sender, messages) = mpsc::channel();
        sender.send(Message::CheckNow).unwrap();
        let drained_at = Instant::now();
        let mut after_a_check = Schedule::new(drained_at);
        let keeps_running = drain_after_check(&messages, &mut after_a_check);
        assert!(keeps_running);
        assert!(!after_a_check.due(drained_at + Duration::from_secs(9)));
        let recheck = drained_at + RECHECK_DELAY + Duration::from_secs(1);
        assert!(after_a_check.due(recheck));
    }

    fn serve_twice(listener: std::net::TcpListener, bodies: [Vec<u8>; 2]) -> Vec<String> {
        use std::io::{BufRead, BufReader, Write};
        let mut agents = Vec::new();
        for body in bodies {
            let (stream, _) = listener.accept().unwrap();
            let cloned = stream.try_clone().unwrap();
            let mut reader = BufReader::new(cloned);
            let mut line = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).unwrap();
                let lowered = line.to_ascii_lowercase();
                if let Some(agent) = lowered.strip_prefix("user-agent:") {
                    agents.push(agent.trim().to_string());
                }
                if line == "\r\n" {
                    break;
                }
            }
            let length = body.len();
            let head =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
            let mut writer = stream;
            writer.write_all(head.as_bytes()).unwrap();
            writer.write_all(&body).unwrap();
        }
        agents
    }

    #[test]
    fn the_http_feed_names_itself_and_streams_the_download() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let port = address.port();
        let release = br#"{"tag_name":"v9.9.9","prerelease":false,"assets":[]}"#.to_vec();
        let sent = vec![7u8; 300_000];
        let served = sent.clone();
        let server = std::thread::spawn(move || serve_twice(listener, [release, served]));

        let feed = HttpFeed {
            feed_url: format!("http://127.0.0.1:{port}/feed"),
        };
        feed.latest().unwrap();
        let temp = tempfile::TempDir::new().unwrap();
        let target = temp.path().join("taco-shells.zip");
        let url = format!("http://127.0.0.1:{port}/asset");
        feed.download(&url, &target).unwrap();

        let on_disk = fs::read(&target).unwrap();
        assert!(on_disk == sent);
        let agents = server.join().unwrap();
        assert_eq!(agents, vec![USER_AGENT, USER_AGENT]);
    }

    #[test]
    fn at_start_only_the_newest_whole_newer_version_is_kept() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path();
        for name in ["1.0.0", "0.9.0", "1.1.0.partial", "1.2.0", "1.3.0", "1.4.0"] {
            let folder = updates.join(name);
            fs::create_dir_all(folder).unwrap();
        }
        let newest_folder = updates.join("1.3.0");
        let two_apps = updates.join("1.4.0");
        let older_folder = updates.join("1.2.0");
        make_app(&older_folder, "Taco.app");
        make_app(&newest_folder, "Taco.app");
        make_app(&two_apps, "One.app");
        make_app(&two_apps, "Two.app");

        let adopted = prune_and_adopt(updates, "1.0.0").unwrap();

        assert_eq!(adopted.version, "1.3.0");
        assert_eq!(adopted.path, newest_folder.join("Taco.app"));
        assert_eq!(child_names(updates), vec!["1.3.0"]);

        let numeric = tempfile::TempDir::new().unwrap();
        let versions = numeric.path();
        for name in ["1.2.0", "1.9.0", "1.10.0"] {
            let folder = versions.join(name);
            make_app(&folder, "Taco.app");
        }

        let adopted = prune_and_adopt(versions, "1.0.0").unwrap();

        assert_eq!(adopted.version, "1.10.0");
        assert_eq!(child_names(versions), vec!["1.10.0"]);
    }

    #[test]
    fn a_download_is_offered_only_when_whole_and_holding_one_app() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path().join("updates");
        let one_directory = temp.path().join("one");
        let one = make_zip(&one_directory, &["Foo.app"]);
        let two_directory = temp.path().join("two");
        let two = make_zip(&two_directory, &["Foo.app", "Bar.app"]);
        let (silent, _calls) = mpsc::channel();
        let feed = |zip: &Path| zip_feed(json!(null), zip, silent.clone());
        let size = file_size(&one);

        let leftover = updates.join("1.2.3.partial");
        fs::create_dir_all(&leftover).unwrap();
        let junk = leftover.join(ZIP_NAME);
        fs::write(junk, "junk").unwrap();
        let older = updates.join("1.2.3");
        make_app(&older, "Old.app");

        let release = release_info("1.2.3", size);
        let whole = download_and_extract(&release, &updates, &feed(&one));
        let replaced = updates.join("1.2.3").join("Foo.app");
        assert_eq!(whole, Some(replaced));
        assert_eq!(child_names(&updates), vec!["1.2.3"]);

        let release = release_info("1.2.4", size + 1);
        let short = download_and_extract(&release, &updates, &feed(&one));
        assert_eq!(short, None);
        assert_eq!(child_names(&updates), vec!["1.2.3"]);

        let two_size = file_size(&two);
        let release = release_info("1.2.5", two_size);
        let doubled = download_and_extract(&release, &updates, &feed(&two));
        assert_eq!(doubled, None);
        assert_eq!(child_names(&updates), vec!["1.2.3"]);
    }

    #[test]
    fn a_staged_version_is_not_downloaded_again_and_a_newer_one_replaces_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path().join("updates");
        let zip_directory = temp.path().join("zip");
        let zip = make_zip(&zip_directory, &["Taco.app"]);
        let size = file_size(&zip);
        let (silent, _calls) = mpsc::channel();
        let first = release_payload("v1.0.1", "taco-shells-1.0.1-arm64.zip", json!(size));
        let feed = zip_feed(first, &zip, silent);
        let release = Arc::clone(&feed.release);
        let downloads = Arc::clone(&feed.downloads);
        let (host, received, _quit_called) = recording_host(temp.path().join("taco"));
        let shared = Shared {
            ready: Mutex::new(None),
            applying: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            host,
        };

        run_check(&shared, &updates, "1.0.0", &feed);
        run_check(&shared, &updates, "1.0.0", &feed);

        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        let events: Vec<UpdateEvent> = received.try_iter().collect();
        assert_eq!(events, vec![UpdateEvent::Ready("1.0.1".to_string())]);

        {
            let mut slot = lock(&release);
            *slot = release_payload("v1.0.2", "taco-shells-1.0.2-arm64.zip", json!(size));
        }
        run_check(&shared, &updates, "1.0.0", &feed);

        assert_eq!(downloads.load(Ordering::SeqCst), 2);
        let ready_version = {
            let ready = lock(&shared.ready);
            ready.as_ref().map(|app| app.version.clone())
        };
        assert_eq!(ready_version, Some("1.0.2".to_string()));
        assert_eq!(child_names(&updates), vec!["1.0.2"]);
        let events: Vec<UpdateEvent> = received.try_iter().collect();
        assert_eq!(events, vec![UpdateEvent::Ready("1.0.2".to_string())]);

        shared.applying.store(true, Ordering::SeqCst);
        {
            let mut slot = lock(&release);
            *slot = release_payload("v1.0.3", "taco-shells-1.0.3-arm64.zip", json!(size));
        }
        run_check(&shared, &updates, "1.0.0", &feed);

        let while_applying = {
            let ready = lock(&shared.ready);
            ready.as_ref().map(|app| app.version.clone())
        };
        assert_eq!(while_applying, Some("1.0.2".to_string()));
        let staged = updates.join("1.0.2");
        assert!(staged.is_dir());
        let events: Vec<UpdateEvent> = received.try_iter().collect();
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn the_service_offers_newer_releases_once_and_will_not_apply_outside_applications() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path().join("updates");
        let zip_directory = temp.path().join("zip");
        let zip = make_zip(&zip_directory, &["Taco.app"]);
        let size = file_size(&zip);
        let started = long_ago();

        let (calls, call_log) = mpsc::channel();
        let (host, received, quit_called) = recording_host(temp.path().join("taco"));
        let newer = release_payload("v1.0.1", "taco-shells-1.0.1-arm64.zip", json!(size));
        let feed = zip_feed(newer, &zip, calls);
        let updater =
            Updater::start_with(updates, "1.0.0".to_string(), Box::new(feed), host, started);

        let wait = Duration::from_secs(5);
        let first = received.recv_timeout(wait).unwrap();
        assert_eq!(first, UpdateEvent::Ready("1.0.1".to_string()));
        assert_eq!(updater.ready_version(), Some("1.0.1".to_string()));
        call_log.recv_timeout(wait).unwrap();

        updater.apply_ready();
        let problem = received.recv_timeout(wait).unwrap();
        assert_eq!(
            problem,
            UpdateEvent::Problem(NOT_IN_APPLICATIONS.to_string())
        );
        assert!(!quit_called.load(Ordering::SeqCst));
        updater.stop();

        let (calls, call_log) = mpsc::channel();
        let (host, received, _quit_called) = recording_host(temp.path().join("taco"));
        let same = release_payload("v1.0.0", "taco-shells-1.0.0-arm64.zip", json!(size));
        let feed = zip_feed(same, &zip, calls);
        let updater = Updater::start_with(
            temp.path().join("other-updates"),
            "1.0.0".to_string(),
            Box::new(feed),
            host,
            started,
        );
        call_log.recv_timeout(wait).unwrap();
        let mut second_check_ran = false;
        for _ in 0..100 {
            updater.check_now();
            if call_log.recv_timeout(Duration::from_millis(50)).is_ok() {
                second_check_ran = true;
                break;
            }
        }
        assert!(second_check_ran);
        updater.stop();
        assert!(received.try_recv().is_err());
    }

    #[test]
    fn applying_with_the_ready_app_gone_reports_it_and_never_quits() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path().join("updates");
        let staged = updates.join("1.0.1");
        make_app(&staged, "Taco.app");
        let zip = temp.path().join("unused.zip");
        let (silent, _calls) = mpsc::channel();
        let not_newer = release_payload("v1.0.0", "taco-shells-1.0.0-arm64.zip", json!(1));
        let feed = zip_feed(not_newer, &zip, silent);
        let executable = PathBuf::from("/Applications/Taco Shells.app/Contents/MacOS/taco");
        let (host, received, quit_called) = recording_host(executable);
        let started = long_ago();
        let updater =
            Updater::start_with(updates, "1.0.0".to_string(), Box::new(feed), host, started);
        let wait = Duration::from_secs(5);
        let first = received.recv_timeout(wait).unwrap();
        assert_eq!(first, UpdateEvent::Ready("1.0.1".to_string()));
        fs::remove_dir_all(&staged).unwrap();

        updater.apply_ready();

        let problem = received.recv_timeout(wait).unwrap();
        let message = "The update could not be started: the downloaded update is missing";
        assert_eq!(problem, UpdateEvent::Problem(message.to_string()));
        assert_eq!(updater.ready_version(), None);
        assert!(!quit_called.load(Ordering::SeqCst));
        updater.stop();
    }
}
