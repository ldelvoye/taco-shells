mod system;
pub(crate) mod version;

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use taco_identity::{IS_STABLE, UPDATE_FEED_VARIABLE, UPDATE_HELPER_PATH, release_zip_name};

use crate::mac;
use crate::sync::lock;
use system::{extract_zip, spawn_update_helper};
use version::{is_newer_version, is_version};

pub(crate) const FIRST_CHECK_DELAY: Duration = Duration::from_secs(10);
pub(crate) const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
// Past the 60 seconds GitHub's shared cache can keep serving the previous release.
pub(crate) const RECHECK_DELAY: Duration = Duration::from_secs(75);

const DEFAULT_FEED_URL: &str = "https://api.github.com/repos/ldelvoye/taco-shells/releases/latest";
const USER_AGENT: &str = "taco-shells";
const FEED_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(60);
const UPDATES_DIRECTORY_NAME: &str = "updates";
const ZIP_NAME: &str = "taco-shells.zip";
const STAGING_SUFFIX: &str = ".partial";
const APP_SUFFIX: &str = ".app";
const TAG_PREFIX: &str = "v";
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
    pub(crate) digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Download {
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReadyApp {
    pub(crate) version: String,
    pub(crate) path: PathBuf,
}

pub(crate) trait Feed: Send + Sync {
    fn latest(&self) -> Result<serde_json::Value, String>;
    fn download(&self, url: &str, to: &Path) -> Result<Download, String>;
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

impl Shared {
    fn new(ready: Option<ReadyApp>, host: UpdaterHost) -> Shared {
        Shared {
            ready: Mutex::new(ready),
            applying: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            host,
        }
    }
}

fn parse_digest(asset: &serde_json::Value) -> Option<String> {
    let value = asset.get("digest")?;
    let text = value.as_str()?;
    let hex = text.strip_prefix("sha256:")?;
    let is_lowercase_hex = hex
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if hex.len() != 64 || !is_lowercase_hex {
        return None;
    }
    Some(hex.to_string())
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
    let wanted = release_zip_name(version);
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
        let digest = parse_digest(asset);
        return Some(ReleaseInfo {
            version: version.to_string(),
            asset_url: url.to_string(),
            asset_size: size,
            digest,
        });
    }
    None
}

pub(crate) fn copy_and_hash(reader: &mut dyn Read, writer: &mut dyn Write) -> io::Result<Download> {
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = reader.read(&mut buffer);
        let count = match read {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        hasher.update(chunk);
        writer.write_all(chunk)?;
        bytes += count as u64;
    }
    let digest = hasher.finalize();
    let mut sha256 = String::new();
    for byte in digest.iter() {
        sha256.push_str(&format!("{byte:02x}"));
    }
    Ok(Download { bytes, sha256 })
}

impl HttpFeed {
    fn agent(timeout: Duration, stall_timeout: Option<Duration>) -> ureq::Agent {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .timeout_per_read(stall_timeout)
            .user_agent(USER_AGENT)
            .build();
        ureq::Agent::new_with_config(config)
    }
}

impl Feed for HttpFeed {
    fn latest(&self) -> Result<serde_json::Value, String> {
        let agent = HttpFeed::agent(FEED_TIMEOUT, None);
        let request = agent.get(&self.feed_url);
        let called = request.call();
        let mut response = called.map_err(|error| error.to_string())?;
        let body = response.body_mut();
        let read = body.read_to_string();
        let text = read.map_err(|error| error.to_string())?;
        let parsed = serde_json::from_str(&text);
        parsed.map_err(|error| error.to_string())
    }

    fn download(&self, url: &str, to: &Path) -> Result<Download, String> {
        let agent = HttpFeed::agent(DOWNLOAD_TIMEOUT, Some(DOWNLOAD_STALL_TIMEOUT));
        let request = agent.get(url);
        let called = request.call();
        let response = called.map_err(|error| error.to_string())?;
        let body = response.into_body();
        let mut reader = body.into_reader();
        let created = File::create(to);
        let mut file = created.map_err(|error| error.to_string())?;
        let copied = copy_and_hash(&mut reader, &mut file);
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

fn fetch_and_unpack(release: &ReleaseInfo, staging: &Path, feed: &dyn Feed) -> Result<(), String> {
    remove_path(staging);
    let created = fs::create_dir_all(staging);
    if let Err(error) = created {
        return Err(format!("the staging folder could not be created ({error})"));
    }

    let zip = staging.join(ZIP_NAME);
    let downloaded = feed.download(&release.asset_url, &zip);
    let download = match downloaded {
        Ok(download) => download,
        Err(error) => return Err(format!("the download failed ({error})")),
    };
    if download.bytes != release.asset_size {
        let bytes = download.bytes;
        let expected = release.asset_size;
        return Err(format!(
            "the download was {bytes} bytes, expected {expected}"
        ));
    }
    match &release.digest {
        Some(expected) => {
            if &download.sha256 != expected {
                let actual = &download.sha256;
                return Err(format!(
                    "the download's sha256 was {actual}, expected {expected}"
                ));
            }
        }
        None => {
            let asset = release_zip_name(&release.version);
            eprintln!("the update check found no digest for {asset}, so only its size was checked");
        }
    }

    let extracted = extract_zip(&zip, staging);
    if let Err(error) = extracted {
        return Err(format!("the download could not be unpacked ({error})"));
    }
    let _ = fs::remove_file(&zip);
    let app = app_in(staging);
    if app.is_none() {
        return Err("the download did not hold exactly one app".to_string());
    }
    Ok(())
}

pub(crate) fn download_and_extract(
    release: &ReleaseInfo,
    updates: &Path,
    feed: &dyn Feed,
) -> Result<PathBuf, String> {
    let version_directory = updates.join(&release.version);
    // Unpacked under a name that is not a version, so a crash midway leaves nothing a later launch could adopt.
    let staging_name = format!("{}{STAGING_SUFFIX}", release.version);
    let staging = updates.join(staging_name);

    let unpacked = fetch_and_unpack(release, &staging, feed);
    if let Err(reason) = unpacked {
        remove_path(&staging);
        return Err(reason);
    }

    remove_path(&version_directory);
    let renamed = fs::rename(&staging, &version_directory);
    if let Err(error) = renamed {
        remove_path(&staging);
        return Err(format!(
            "the unpacked update could not be moved into place ({error})"
        ));
    }
    let app = app_in(&version_directory);
    match app {
        Some(path) => Ok(path),
        None => Err("the unpacked update held no app after the move".to_string()),
    }
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

fn start_helper(
    shared: &Shared,
    helper: &Path,
    installed: &Path,
    extracted: &Path,
    spawn: &dyn Fn(&Path, &Path, &Path) -> io::Result<()>,
) -> Applying {
    shared.applying.store(true, Ordering::SeqCst);
    let spawned = spawn(helper, installed, extracted);
    if let Err(error) = spawned {
        shared.applying.store(false, Ordering::SeqCst);
        return Applying::Refused(format!("Taco Shells could not start the update: {error}"));
    }
    Applying::Started
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
    let helper = installed.join(UPDATE_HELPER_PATH);
    if !helper.is_file() {
        let message = "The update could not be started: the update helper is missing";
        return Applying::Refused(message.to_string());
    }

    start_helper(
        shared,
        &helper,
        &installed,
        &ready.path,
        &spawn_update_helper,
    )
}

fn report_failure(message: &str) {
    eprintln!("the update check failed: {message}");
}

fn run_check(shared: &Shared, updates: &Path, current: &str, feed: &dyn Feed) {
    let latest = feed.latest();
    let payload = match latest {
        Ok(payload) => payload,
        Err(reason) => {
            let message = format!("the feed could not be read ({reason})");
            report_failure(&message);
            return;
        }
    };
    let parsed = parse_release(&payload);
    let Some(release) = parsed else {
        let message = "the latest release was not usable (not a stable plain version with its zip)";
        report_failure(message);
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

    let downloaded = download_and_extract(&release, updates, feed);
    let path = match downloaded {
        Ok(path) => path,
        Err(message) => {
            report_failure(&message);
            return;
        }
    };
    {
        let mut ready = lock(&shared.ready);
        // Not replaced once an apply has started, so the folder handed to the helper stays.
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

// A check asked for mid-check still gets its recheck.
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
        let variable = std::env::var(UPDATE_FEED_VARIABLE);
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
        let shared = Arc::new(Shared::new(adopted, host));
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
        let shared = Shared::new(None, host);
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
mod test_support {
    use super::{Download, Feed, Shared, UpdateEvent, UpdaterHost, copy_and_hash};
    use crate::sync::lock;
    use serde_json::json;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    pub(super) struct ZipFeed {
        pub(super) release: Arc<Mutex<serde_json::Value>>,
        zip: PathBuf,
        latest_calls: Mutex<Sender<()>>,
        pub(super) downloads: Arc<AtomicUsize>,
    }

    impl Feed for ZipFeed {
        fn latest(&self) -> Result<serde_json::Value, String> {
            let calls = lock(&self.latest_calls);
            let _ = calls.send(());
            let release = lock(&self.release);
            Ok(release.clone())
        }

        fn download(&self, _url: &str, to: &Path) -> Result<Download, String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            let opened = fs::File::open(&self.zip);
            let mut source = opened.map_err(|error| error.to_string())?;
            let created = fs::File::create(to);
            let mut target = created.map_err(|error| error.to_string())?;
            let copied = copy_and_hash(&mut source, &mut target);
            copied.map_err(|error| error.to_string())
        }
    }

    pub(super) fn zip_feed(
        release: serde_json::Value,
        zip: &Path,
        latest_calls: Sender<()>,
    ) -> ZipFeed {
        ZipFeed {
            release: Arc::new(Mutex::new(release)),
            zip: zip.to_path_buf(),
            latest_calls: Mutex::new(latest_calls),
            downloads: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(super) fn recording_host(
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

    pub(super) fn ready_version(shared: &Shared) -> Option<String> {
        let ready = lock(&shared.ready);
        ready.as_ref().map(|app| app.version.clone())
    }

    pub(super) fn long_ago() -> Instant {
        let now = Instant::now();
        match now.checked_sub(Duration::from_secs(60)) {
            Some(earlier) => earlier,
            None => now,
        }
    }

    pub(super) fn release_payload(
        tag: &str,
        asset_name: &str,
        size: serde_json::Value,
    ) -> serde_json::Value {
        json!({
            "tag_name": tag,
            "prerelease": false,
            "assets": [
                { "name": "other.dmg", "size": 5, "browser_download_url": "https://example.test/other" },
                { "name": asset_name, "size": size, "browser_download_url": "https://example.test/zip" }
            ]
        })
    }

    pub(super) fn make_app(directory: &Path, name: &str) -> PathBuf {
        let app = directory.join(name);
        let contents = app.join("Contents");
        fs::create_dir_all(&contents).unwrap();
        let file = contents.join("file");
        fs::write(file, "x").unwrap();
        app
    }

    pub(super) fn make_zip(temp: &Path, app_names: &[&str]) -> PathBuf {
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

    pub(super) fn file_size(path: &Path) -> u64 {
        let metadata = fs::metadata(path).unwrap();
        metadata.len()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{
        file_size, long_ago, make_app, make_zip, ready_version, recording_host, release_payload,
        zip_feed,
    };
    use super::*;
    use serde_json::json;

    struct FailingFeed;

    impl Feed for FailingFeed {
        fn latest(&self) -> Result<serde_json::Value, String> {
            Err("offline".to_string())
        }

        fn download(&self, _url: &str, _to: &Path) -> Result<Download, String> {
            Err("offline".to_string())
        }
    }

    fn release_info(version: &str, size: u64) -> ReleaseInfo {
        ReleaseInfo {
            version: version.to_string(),
            asset_url: "https://example.test/zip".to_string(),
            asset_size: size,
            digest: None,
        }
    }

    fn sha256_of(path: &Path) -> String {
        let mut file = File::open(path).unwrap();
        let hashed = copy_and_hash(&mut file, &mut io::sink()).unwrap();
        hashed.sha256
    }

    fn started_with_a_staged_update(
        temp: &Path,
        executable: PathBuf,
    ) -> (Updater, Receiver<UpdateEvent>, Arc<AtomicBool>) {
        let updates = temp.join("updates");
        let staged = updates.join("1.0.1");
        make_app(&staged, "Taco.app");
        let zip = temp.join("unused.zip");
        let (silent, _calls) = mpsc::channel();
        let not_newer = release_payload("v1.0.0", "taco-shells-1.0.0-arm64.zip", json!(1));
        let feed = zip_feed(not_newer, &zip, silent);
        let (host, received, quit_called) = recording_host(executable);
        let started = long_ago();
        let updater =
            Updater::start_with(updates, "1.0.0".to_string(), Box::new(feed), host, started);
        let wait = Duration::from_secs(5);
        let first = received.recv_timeout(wait).unwrap();
        assert_eq!(first, UpdateEvent::Ready("1.0.1".to_string()));
        (updater, received, quit_called)
    }

    #[test]
    fn a_release_is_taken_only_when_it_is_a_stable_plain_version_with_its_zip() {
        let good = release_payload("v1.2.3", "taco-shells-1.2.3-arm64.zip", json!(1234));
        let parsed = parse_release(&good);
        let expected = ReleaseInfo {
            version: "1.2.3".to_string(),
            asset_url: "https://example.test/zip".to_string(),
            asset_size: 1234,
            digest: None,
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
    fn a_digest_is_kept_only_as_sha256_with_64_lowercase_hex_characters() {
        let hex = "ab".repeat(32);
        let digest_of = |digest: serde_json::Value| {
            let mut payload = release_payload("v1.2.3", "taco-shells-1.2.3-arm64.zip", json!(1234));
            payload["assets"][1]["digest"] = digest;
            let release = parse_release(&payload).unwrap();
            release.digest
        };

        assert_eq!(digest_of(json!(format!("sha256:{hex}"))), Some(hex.clone()));
        assert_eq!(digest_of(json!(null)), None);
        assert_eq!(digest_of(json!(format!("sha1:{hex}"))), None);
        assert_eq!(
            digest_of(json!(format!("sha256:{}", hex.to_uppercase()))),
            None
        );
        assert_eq!(digest_of(json!(format!("sha256:{}", &hex[2..]))), None);

        let good = release_payload("v1.2.3", "taco-shells-1.2.3-arm64.zip", json!(1234));
        let release = parse_release(&good).unwrap();
        assert_eq!(release.digest, None);
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
        assert_eq!(whole, Ok(replaced));
        assert_eq!(child_names(&updates), vec!["1.2.3"]);

        let release = release_info("1.2.4", size + 1);
        let short = download_and_extract(&release, &updates, &feed(&one));
        assert!(short.is_err());
        assert_eq!(child_names(&updates), vec!["1.2.3"]);

        let two_size = file_size(&two);
        let release = release_info("1.2.5", two_size);
        let doubled = download_and_extract(&release, &updates, &feed(&two));
        assert!(doubled.is_err());
        assert_eq!(child_names(&updates), vec!["1.2.3"]);
    }

    #[test]
    fn a_download_must_match_the_published_digest_when_there_is_one() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path().join("updates");
        let zip_directory = temp.path().join("zip");
        let zip = make_zip(&zip_directory, &["Foo.app"]);
        let (silent, _calls) = mpsc::channel();
        let feed = zip_feed(json!(null), &zip, silent);
        let size = file_size(&zip);
        let real = sha256_of(&zip);

        let mut wrong = release_info("1.2.3", size);
        wrong.digest = Some("0".repeat(64));
        let refused = download_and_extract(&wrong, &updates, &feed);
        assert!(refused.is_err());
        assert_eq!(child_names(&updates), Vec::<String>::new());

        let mut matching = release_info("1.2.3", size);
        matching.digest = Some(real);
        let accepted = download_and_extract(&matching, &updates, &feed);
        assert!(accepted.is_ok());

        let without = release_info("1.2.4", size);
        let on_size_alone = download_and_extract(&without, &updates, &feed);
        assert!(on_size_alone.is_ok());
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
        let shared = Shared::new(None, host);

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
        assert_eq!(ready_version(&shared), Some("1.0.2".to_string()));
        assert_eq!(child_names(&updates), vec!["1.0.2"]);
        let events: Vec<UpdateEvent> = received.try_iter().collect();
        assert_eq!(events, vec![UpdateEvent::Ready("1.0.2".to_string())]);

        shared.applying.store(true, Ordering::SeqCst);
        {
            let mut slot = lock(&release);
            *slot = release_payload("v1.0.3", "taco-shells-1.0.3-arm64.zip", json!(size));
        }
        run_check(&shared, &updates, "1.0.0", &feed);

        assert_eq!(ready_version(&shared), Some("1.0.2".to_string()));
        let staged = updates.join("1.0.2");
        assert!(staged.is_dir());
        let events: Vec<UpdateEvent> = received.try_iter().collect();
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn a_failing_feed_ends_the_check_without_an_event() {
        let temp = tempfile::TempDir::new().unwrap();
        let updates = temp.path().join("updates");
        let (host, received, _quit_called) = recording_host(temp.path().join("taco"));
        let shared = Shared::new(None, host);

        run_check(&shared, &updates, "1.0.0", &FailingFeed);

        assert!(received.try_recv().is_err());
        assert_eq!(ready_version(&shared), None);
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
        assert_eq!(ready_version(&updater.shared), Some("1.0.1".to_string()));
        call_log.recv_timeout(wait).unwrap();

        updater.apply_ready();
        let problem = received.recv_timeout(wait).unwrap();
        assert_eq!(
            problem,
            UpdateEvent::Problem(NOT_IN_APPLICATIONS.to_string())
        );
        assert!(!quit_called.load(Ordering::SeqCst));
        updater.stop();
    }

    #[test]
    fn applying_with_the_ready_app_gone_reports_it_and_never_quits() {
        let temp = tempfile::TempDir::new().unwrap();
        let executable = PathBuf::from("/Applications/Taco Shells.app/Contents/MacOS/taco");
        let (updater, received, quit_called) =
            started_with_a_staged_update(temp.path(), executable);
        let staged = temp.path().join("updates").join("1.0.1");
        fs::remove_dir_all(&staged).unwrap();

        updater.apply_ready();

        let wait = Duration::from_secs(5);
        let problem = received.recv_timeout(wait).unwrap();
        let message = "The update could not be started: the downloaded update is missing";
        assert_eq!(problem, UpdateEvent::Problem(message.to_string()));
        assert_eq!(ready_version(&updater.shared), None);
        assert!(!quit_called.load(Ordering::SeqCst));
        updater.stop();
    }

    #[test]
    fn applying_without_an_update_helper_next_to_the_app_reports_it_and_never_quits() {
        let temp = tempfile::TempDir::new().unwrap();
        let executable =
            PathBuf::from("/Applications/Taco Shells Without Helper.app/Contents/MacOS/taco");
        let (updater, received, quit_called) =
            started_with_a_staged_update(temp.path(), executable);

        updater.apply_ready();

        let wait = Duration::from_secs(5);
        let problem = received.recv_timeout(wait).unwrap();
        let message = "The update could not be started: the update helper is missing";
        assert_eq!(problem, UpdateEvent::Problem(message.to_string()));
        assert!(!quit_called.load(Ordering::SeqCst));
        updater.stop();
    }

    #[test]
    fn a_helper_that_fails_to_spawn_leaves_the_app_free_to_apply_again() {
        let temp = tempfile::TempDir::new().unwrap();
        let (host, _received, _quit_called) = recording_host(temp.path().join("taco"));
        let shared = Shared::new(None, host);
        let failing = |_helper: &Path, _installed: &Path, _extracted: &Path| {
            assert!(shared.applying.load(Ordering::SeqCst));
            Err(io::Error::other("no such shell"))
        };
        let path = temp.path();

        let outcome = start_helper(&shared, path, path, path, &failing);

        let Applying::Refused(message) = outcome else {
            panic!("a failed spawn must be refused");
        };
        assert!(message.contains("no such shell"), "{message}");
        assert!(!shared.applying.load(Ordering::SeqCst));
    }
}
