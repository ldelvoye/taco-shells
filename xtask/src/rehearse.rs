use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::build::{Channel, Profile};
use crate::bundle::{self, Options as BundleOptions};
use crate::install;
use crate::repo;

const REHEARSAL_VERSION: &str = "999.0.0";
const REHEARSAL_APP: &str = "/Applications/Taco Shells Rehearsal.app";
const DEFAULT_ELECTRON_APP: &str = "/Applications/Taco Shells Dev.app";
const ELECTRON_BUNDLE_IDENTIFIER: &str = "com.ldelvoye.tacoshells.dev";
const CONFIG_DIR_VARIABLE: &str = "TACO_SHELLS_CONFIG_DIR";
const UPDATE_FEED_VARIABLE: &str = "TACO_SHELLS_UPDATE_FEED";
const BACKGROUND_WINDOW_VARIABLE: &str = "TACO_SHELLS_BACKGROUND_WINDOW";
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const READY_POLL: Duration = Duration::from_millis(500);
const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const RUST_EXECUTABLE: &str = "Contents/MacOS/taco-shells";
const RUST_EXECUTABLE_NAME: &str = "taco-shells";
// The Rust copy installed to be updated; any plain version below REHEARSAL_VERSION would do.
const OLD_RUST_VERSION: &str = "0.0.1";
const VERSION_KEY: &str = "CFBundleShortVersionString";
const APP_LOG: &str = "app.log";
const NOT_A_TERMINAL: &str = "the click-through waits for you to press Enter, and this has no terminal to read it from; run it in a terminal window, or use --until-ready";
const ELECTRON_AND_RUST: &str = "--electron and --from-rust do not go together";

pub struct Options {
    pub electron: Option<PathBuf>,
    pub from_rust: bool,
    pub until_ready: bool,
}

struct ElectronApp {
    path: PathBuf,
    executable: String,
}

/// The app the rehearsal installs and then updates: the Electron Dev build, or a Rust build of an older version.
enum OldApp {
    Electron(ElectronApp),
    Rust,
}

struct Rehearsal {
    work: PathBuf,
    config: PathBuf,
    feed_url: String,
    old: OldApp,
}

/// A child process a thread waits on; `exited` hears once it has been reaped.
struct Reaped {
    pid: u32,
    exited: mpsc::Receiver<()>,
}

pub fn run(options: &Options) -> Result<(), String> {
    let stdin_is_a_terminal = io::stdin().is_terminal();
    if !options.until_ready && !stdin_is_a_terminal {
        return Err(NOT_A_TERMINAL.to_string());
    }
    let old = if options.from_rust {
        if options.electron.is_some() {
            return Err(ELECTRON_AND_RUST.to_string());
        }
        OldApp::Rust
    } else {
        let electron = check_electron(&options.electron)?;
        OldApp::Electron(electron)
    };

    let target = repo::target_dir()?;
    let work = target.join("rehearsal.noindex");
    let config = work.join("config");
    if work.exists() {
        remove_dir(&work)?;
    }
    create_dir(&config)?;

    let profile = profile_for(options.until_ready);
    let built = build_rust_bundle(&config, profile)?;
    let (served, installed) = match &old {
        OldApp::Electron(electron) => (built, electron.path.clone()),
        OldApp::Rust => {
            let older = versioned_copy(&built, &work.join("old"), OLD_RUST_VERSION)?;
            let newer = versioned_copy(&built, &work.join("new"), REHEARSAL_VERSION)?;
            (newer, older)
        }
    };
    let zip = zip_up(&served, &work)?;
    let zip_size = file_size(&zip)?;

    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("could not listen on 127.0.0.1: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("could not read the feed port: {error}"))?;
    let port = address.port();
    let feed_url = format!("http://127.0.0.1:{port}/release.json");
    let release = release_json(port, zip_size);
    let zip_for_server = zip.clone();
    thread::spawn(move || serve(listener, &release, &zip_for_server, zip_size));

    let rehearsal_app = Path::new(REHEARSAL_APP);
    if install::is_running(rehearsal_app)? {
        return Err(format!(
            "{} is running. Quit it and run this again.",
            rehearsal_app.display()
        ));
    }
    if rehearsal_app.exists() {
        remove_dir(rehearsal_app)?;
    }
    let mut ditto = Command::new("ditto");
    ditto.arg(&installed);
    ditto.arg(rehearsal_app);
    repo::run(&mut ditto)?;

    let rehearsal = Rehearsal {
        work,
        config,
        feed_url,
        old,
    };
    if options.until_ready {
        run_until_ready(&rehearsal)
    } else {
        run_click_through(&rehearsal)
    }
}

impl OldApp {
    fn executable(&self) -> &str {
        match self {
            OldApp::Electron(electron) => &electron.executable,
            OldApp::Rust => RUST_EXECUTABLE_NAME,
        }
    }

    fn described(&self) -> String {
        match self {
            OldApp::Electron(_) => "the Electron Dev build".to_string(),
            OldApp::Rust => format!("a Rust build at version {OLD_RUST_VERSION}"),
        }
    }

    // Electron is swapped for the Rust app; an older Rust build for the one with the served version.
    fn was_swapped(&self) -> bool {
        let rehearsal_app = Path::new(REHEARSAL_APP);
        match self {
            OldApp::Electron(_) => rehearsal_app.join(RUST_EXECUTABLE).exists(),
            OldApp::Rust => {
                let version = plist_value(rehearsal_app, VERSION_KEY);
                matches!(version, Ok(version) if version == REHEARSAL_VERSION)
            }
        }
    }
}

// The unattended run is a debug build, for speed; a person's click-through gets the release build.
fn profile_for(until_ready: bool) -> Profile {
    if until_ready {
        return Profile::Debug;
    }
    Profile::Release
}

fn check_electron(requested: &Option<PathBuf>) -> Result<ElectronApp, String> {
    let path = match requested {
        Some(path) => path.clone(),
        None => PathBuf::from(DEFAULT_ELECTRON_APP),
    };
    if !path.exists() {
        return Err(format!(
            "the Electron app {} does not exist",
            path.display()
        ));
    }
    let framework = path.join("Contents/Frameworks/Electron Framework.framework");
    if !framework.exists() {
        return Err(format!(
            "{} is not an Electron app: {} is missing",
            path.display(),
            framework.display()
        ));
    }
    let identifier = plist_value(&path, "CFBundleIdentifier")?;
    if identifier != ELECTRON_BUNDLE_IDENTIFIER {
        return Err(format!(
            "{} has bundle identifier {identifier}, expected {ELECTRON_BUNDLE_IDENTIFIER}",
            path.display()
        ));
    }
    let executable = plist_value(&path, "CFBundleExecutable")?;
    Ok(ElectronApp { path, executable })
}

fn plist_value(app: &Path, key: &str) -> Result<String, String> {
    let info_plist = app.join("Contents/Info.plist");
    let mut plutil = Command::new("plutil");
    plutil.args(["-extract", key, "raw", "-o", "-"]);
    plutil.arg(&info_plist);
    match repo::capture(&mut plutil) {
        Some(value) => Ok(value),
        None => Err(format!(
            "could not read {key} from {}",
            info_plist.display()
        )),
    }
}

// The update script reopens the app with `open`, which passes it no environment, so the Rust app carries the
// rehearsal's config directory compiled in and opens on what the old copy left there.
fn build_rust_bundle(config: &Path, profile: Profile) -> Result<PathBuf, String> {
    let options = BundleOptions {
        profile,
        channel: Channel::Development,
        zip: false,
        icon: false,
        drive: false,
        config_dir: Some(config.to_path_buf()),
    };
    let bundled = bundle::create(&options)?;
    Ok(bundled.app)
}

// The updater reads the app's version from its Info.plist, so a copy is told its version there and signed again,
// since the bundle's signature seals that file.
fn versioned_copy(app: &Path, folder: &Path, version: &str) -> Result<PathBuf, String> {
    create_dir(folder)?;
    let Some(name) = app.file_name() else {
        return Err(format!("{} has no file name", app.display()));
    };
    let copy = folder.join(name);
    let mut ditto = Command::new("ditto");
    ditto.arg(app);
    ditto.arg(&copy);
    repo::run(&mut ditto)?;

    let info_plist = copy.join("Contents/Info.plist");
    let mut plutil = Command::new("plutil");
    plutil.args(["-replace", VERSION_KEY, "-string", version]);
    plutil.arg(&info_plist);
    repo::run(&mut plutil)?;

    let mut codesign = Command::new("codesign");
    codesign.args(["--force", "--sign", "-"]);
    codesign.arg(&copy);
    repo::run(&mut codesign)?;
    Ok(copy)
}

fn zip_up(app: &Path, work: &Path) -> Result<PathBuf, String> {
    let zip = work.join(zip_name());
    let mut ditto = Command::new("ditto");
    ditto.args(["-c", "-k", "--keepParent"]);
    ditto.arg(app);
    ditto.arg(&zip);
    repo::run(&mut ditto)?;
    Ok(zip)
}

fn file_size(path: &Path) -> Result<u64, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("could not read the size of {}: {error}", path.display()))?;
    Ok(metadata.len())
}

fn release_json(port: u16, zip_size: u64) -> String {
    let version = REHEARSAL_VERSION;
    let zip_name = zip_name();
    format!(
        "{{\"tag_name\":\"v{version}\",\"prerelease\":false,\"assets\":[{{\"name\":\"{zip_name}\",\"browser_download_url\":\"http://127.0.0.1:{port}/{zip_name}\",\"size\":{zip_size}}}]}}"
    )
}

fn zip_name() -> String {
    format!("taco-shells-{REHEARSAL_VERSION}-arm64.zip")
}

fn serve(listener: TcpListener, release: &str, zip: &Path, zip_size: u64) {
    for connection in listener.incoming() {
        let outcome = match connection {
            Ok(stream) => answer(stream, release, zip, zip_size),
            Err(error) => Err(error),
        };
        if let Err(error) = outcome {
            eprintln!("feed connection failed: {error}");
        }
    }
}

fn answer(mut stream: TcpStream, release: &str, zip: &Path, zip_size: u64) -> io::Result<()> {
    let path = read_request_path(&stream)?;
    println!("served {path}");

    let zip_path = format!("/{}", zip_name());
    if path == "/release.json" {
        let header = response_header("200 OK", "application/json", release.len() as u64);
        stream.write_all(header.as_bytes())?;
        stream.write_all(release.as_bytes())?;
    } else if path == zip_path {
        let header = response_header("200 OK", "application/zip", zip_size);
        stream.write_all(header.as_bytes())?;
        let mut file = File::open(zip)?;
        io::copy(&mut file, &mut stream)?;
    } else {
        let header = response_header("404 Not Found", "text/plain", 0);
        stream.write_all(header.as_bytes())?;
    }
    stream.flush()
}

fn read_request_path(stream: &TcpStream) -> io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 || line.trim().is_empty() {
            break;
        }
    }
    let mut parts = request_line.split_whitespace();
    let _method = parts.next();
    match parts.next() {
        Some(path) => Ok(path.to_string()),
        None => Ok(String::new()),
    }
}

fn response_header(status: &str, content_type: &str, length: u64) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
    )
}

fn launch(rehearsal: &Rehearsal, background: bool) -> Result<Child, String> {
    let executable = Path::new(REHEARSAL_APP)
        .join("Contents/MacOS")
        .join(rehearsal.old.executable());
    let log_path = rehearsal.work.join(APP_LOG);
    let log = File::create(&log_path)
        .map_err(|error| format!("could not create {}: {error}", log_path.display()))?;
    let error_log = log
        .try_clone()
        .map_err(|error| format!("could not open {}: {error}", log_path.display()))?;

    let mut command = Command::new(&executable);
    command.env(CONFIG_DIR_VARIABLE, &rehearsal.config);
    command.env(UPDATE_FEED_VARIABLE, &rehearsal.feed_url);
    if background {
        command.env(BACKGROUND_WINDOW_VARIABLE, "1");
    }
    command.stdout(Stdio::from(log));
    command.stderr(Stdio::from(error_log));
    command
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", executable.display()))
}

fn run_until_ready(rehearsal: &Rehearsal) -> Result<(), String> {
    let mut child = match launch(rehearsal, true) {
        Ok(child) => child,
        Err(error) => {
            remove_rehearsal_app();
            return Err(error);
        }
    };

    let updates = rehearsal.config.join("updates").join(REHEARSAL_VERSION);
    let ready = wait_for_ready_app(&updates);

    end_child(&mut child);
    remove_rehearsal_app();

    match ready {
        Some(app) => {
            println!(
                "ready: {} downloaded and unpacked the Rust bundle at {}",
                rehearsal.old.described(),
                app.display()
            );
            Ok(())
        }
        None => Err(format!(
            "no update appeared in {} within {} seconds, see {}",
            updates.display(),
            READY_TIMEOUT.as_secs(),
            rehearsal.work.join(APP_LOG).display()
        )),
    }
}

// A person's run: the old copy is shown, they click Restart to Update, and the Rust app opens in its place.
fn run_click_through(rehearsal: &Rehearsal) -> Result<(), String> {
    let child = match launch(rehearsal, false) {
        Ok(child) => child,
        Err(error) => {
            remove_rehearsal_app();
            return Err(error);
        }
    };
    let old_copy = reaped_when_it_exits(child);

    println!(
        "Taco Shells Rehearsal is open: {}, on {}.",
        rehearsal.old.described(),
        rehearsal.config.display()
    );
    println!("Wait for the Update menu (about 15 s), then click Restart to Update.");
    println!(
        "It should reopen as the Rust app it was served, on the same config. The old copy goes to the Trash."
    );
    println!("Press Enter here once you have looked.");
    let mut answer = String::new();
    let read = io::stdin().read_line(&mut answer);
    let answered = matches!(read, Ok(count) if count > 0);
    if !answered {
        return Err(format!(
            "no answer came: {REHEARSAL_APP} and the old copy are left as they are; quit the app and delete it by hand"
        ));
    }

    end_reaped(&old_copy);
    if rehearsal.old.was_swapped() {
        println!("{REHEARSAL_APP} is now the Rust app it was served.");
    } else {
        let updates = rehearsal.config.join("updates").join(REHEARSAL_VERSION);
        println!("{REHEARSAL_APP} was not replaced: the update did not happen.");
        println!(
            "The downloaded app, if any, is in {}. The update script gives up without a word if the old app has not quit within 30 s.",
            updates.display()
        );
    }
    let rehearsal_app = Path::new(REHEARSAL_APP);
    match install::is_running(rehearsal_app) {
        Ok(false) => remove_rehearsal_app(),
        Ok(true) => println!("{REHEARSAL_APP} is still running. Quit it, then delete it."),
        Err(error) => println!("{error}; delete {REHEARSAL_APP} by hand once it has quit."),
    }
    Ok(())
}

// The update script waits for the old app's pid to go, and a child nobody has waited on stays a zombie that still
// answers `kill -0`, so the swap would never happen. A thread waits on it, reaping it the moment it quits.
fn reaped_when_it_exits(mut child: Child) -> Reaped {
    let pid = child.id();
    let (sender, exited) = mpsc::channel();
    thread::spawn(move || {
        let _ = child.wait();
        let _ = sender.send(());
    });
    Reaped { pid, exited }
}

fn has_exited(reaped: &Reaped) -> bool {
    let news = reaped.exited.try_recv();
    !matches!(news, Err(TryRecvError::Empty))
}

// Signals go by pid only while the thread has not reaped it, so they never reach a reused pid.
fn end_reaped(reaped: &Reaped) {
    if has_exited(reaped) {
        return;
    }
    signal(reaped.pid, "-TERM");
    let ended = reaped.exited.recv_timeout(STOP_TIMEOUT);
    if ended.is_ok() {
        return;
    }
    signal(reaped.pid, "-KILL");
    let _ = reaped.exited.recv_timeout(STOP_TIMEOUT);
}

fn signal(pid: u32, which: &str) {
    let mut kill = Command::new("kill");
    kill.args([which, &pid.to_string()]);
    let _ = kill.status();
}

fn wait_for_ready_app(updates: &Path) -> Option<PathBuf> {
    let started = Instant::now();
    loop {
        if let Some(app) = find_app(updates) {
            return Some(app);
        }
        if started.elapsed() >= READY_TIMEOUT {
            return None;
        }
        thread::sleep(READY_POLL);
    }
}

fn find_app(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let extension = path.extension();
        if extension == Some(OsStr::new("app")) {
            return Some(path);
        }
    }
    None
}

fn end_child(child: &mut Child) {
    let mut terminate = Command::new("kill");
    terminate.args(["-TERM", &child.id().to_string()]);
    let _ = terminate.status();

    let started = Instant::now();
    while started.elapsed() < STOP_TIMEOUT {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn remove_rehearsal_app() {
    let rehearsal_app = Path::new(REHEARSAL_APP);
    if let Err(error) = remove_dir(rehearsal_app) {
        eprintln!("{error}");
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
