use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
const NOT_A_TERMINAL: &str = "the click-through waits for you to press Enter, and this has no terminal to read it from; run it in a terminal window, or use --until-ready";

pub struct Options {
    pub electron: Option<PathBuf>,
    pub until_ready: bool,
}

struct ElectronApp {
    path: PathBuf,
    executable: String,
}

struct Rehearsal {
    work: PathBuf,
    config: PathBuf,
    feed_url: String,
}

pub fn run(options: &Options) -> Result<(), String> {
    let stdin_is_a_terminal = io::stdin().is_terminal();
    if !options.until_ready && !stdin_is_a_terminal {
        return Err(NOT_A_TERMINAL.to_string());
    }
    let electron = check_electron(&options.electron)?;

    let target = repo::target_dir()?;
    let work = target.join("rehearsal.noindex");
    let config = work.join("config");
    if work.exists() {
        remove_dir(&work)?;
    }
    create_dir(&config)?;

    let profile = if options.until_ready {
        Profile::Debug
    } else {
        Profile::Release
    };
    let zip = build_zip(&work, &config, profile)?;
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
    ditto.arg(&electron.path);
    ditto.arg(rehearsal_app);
    repo::run(&mut ditto)?;

    let rehearsal = Rehearsal {
        work,
        config,
        feed_url,
    };
    if options.until_ready {
        run_until_ready(&electron, &rehearsal)
    } else {
        run_click_through(&electron, &rehearsal)
    }
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
// rehearsal's config directory compiled in and opens on what the Electron copy left there.
fn build_zip(work: &Path, config: &Path, profile: Profile) -> Result<PathBuf, String> {
    let options = BundleOptions {
        profile,
        channel: Channel::Development,
        zip: false,
        icon: false,
        drive: false,
        config_dir: Some(config.to_path_buf()),
    };
    let bundled = bundle::create(&options)?;
    let zip = work.join(zip_name());
    let mut ditto = Command::new("ditto");
    ditto.args(["-c", "-k", "--keepParent"]);
    ditto.arg(&bundled.app);
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

fn launch(
    electron: &ElectronApp,
    rehearsal: &Rehearsal,
    background: bool,
) -> Result<Child, String> {
    let executable = Path::new(REHEARSAL_APP)
        .join("Contents/MacOS")
        .join(&electron.executable);
    let log_path = rehearsal.work.join("electron.log");
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

fn run_until_ready(electron: &ElectronApp, rehearsal: &Rehearsal) -> Result<(), String> {
    let mut child = match launch(electron, rehearsal, true) {
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
                "ready: the Electron Dev build downloaded and unpacked the Rust bundle at {}",
                app.display()
            );
            Ok(())
        }
        None => Err(format!(
            "no update appeared in {} within {} seconds, see {}",
            updates.display(),
            READY_TIMEOUT.as_secs(),
            rehearsal.work.join("electron.log").display()
        )),
    }
}

// A person's run: the Electron copy is shown, they click Restart to Update, and the Rust app opens in its place.
fn run_click_through(electron: &ElectronApp, rehearsal: &Rehearsal) -> Result<(), String> {
    let mut child = match launch(electron, rehearsal, false) {
        Ok(child) => child,
        Err(error) => {
            remove_rehearsal_app();
            return Err(error);
        }
    };

    println!(
        "Taco Shells Rehearsal is open: the Electron Dev build, on {}.",
        rehearsal.config.display()
    );
    println!("Wait for the Update menu (about 15 s), then click Restart to Update.");
    println!(
        "It should reopen as the Rust app, on the same config. The Electron copy goes to the Trash."
    );
    println!("Press Enter here once you have looked.");
    let mut answer = String::new();
    let read = io::stdin().read_line(&mut answer);
    let answered = matches!(read, Ok(count) if count > 0);
    if !answered {
        return Err(format!(
            "no answer came: {REHEARSAL_APP} and the Electron copy are left as they are; quit the app and delete it by hand"
        ));
    }

    let electron_still_running = matches!(child.try_wait(), Ok(None));
    if electron_still_running {
        end_child(&mut child);
    }
    let became_rust = Path::new(REHEARSAL_APP).join(RUST_EXECUTABLE).exists();
    if became_rust {
        println!("{REHEARSAL_APP} is now the Rust app.");
    } else {
        println!("{REHEARSAL_APP} was not replaced: the update did not happen.");
    }
    let rehearsal_app = Path::new(REHEARSAL_APP);
    match install::is_running(rehearsal_app) {
        Ok(false) => remove_rehearsal_app(),
        Ok(true) => println!("{REHEARSAL_APP} is still running. Quit it, then delete it."),
        Err(error) => println!("{error}; delete {REHEARSAL_APP} by hand once it has quit."),
    }
    Ok(())
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
