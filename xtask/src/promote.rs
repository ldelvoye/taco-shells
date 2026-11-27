use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use taco_identity::{
    RELEASE_KEYS, RELEASE_SIGNING_NAMESPACE, STABLE, release_signature_name, release_zip_name,
};

use crate::build::{Channel, Profile};
use crate::bump::Version;
use crate::bundle::{self, Options as BundleOptions};
use crate::install;
use crate::release;
use crate::repo;
use crate::smoke;

const SSH_KEYGEN: &str = "/usr/bin/ssh-keygen";
const KEY_FOLDER: &str = ".config/taco-shells-release";
const KEY_FILE: &str = "release_key";
const INSTALL_SCRIPT: &str = "install.sh";
const PORT_FILE: &str = "focus-port";
const UPDATE_CHECK_REQUEST: &str = "POST /update/check HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const UPDATE_CHECK_TIMEOUT: Duration = Duration::from_secs(5);
const SMOKE_PACKAGE: &str = "taco-smoke";
const INSTALL_NOTES: &str = "## Installing\n\n    curl -fsSL https://github.com/ldelvoye/taco-shells/releases/latest/download/install.sh | sh\n\nThat puts the latest stable release in Applications and opens it. From then on the app updates itself.";

pub struct Options {
    pub publish: bool,
    pub notes_file: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

/// What a promotion has built and checked before it installs or publishes anything.
struct Built {
    repo_root: PathBuf,
    tag: String,
    version: String,
    installed: PathBuf,
    install_script: PathBuf,
    app: PathBuf,
    zip: PathBuf,
    key: PathBuf,
}

/// Tests the tagged HEAD, builds the stable bundle and installs it here; with `--publish`, also signs it and makes its
/// release the Latest one.
pub fn run(options: &Options) -> Result<(), String> {
    let built = build_and_check(options)?;
    if options.publish {
        return publish(&built, options.notes_file.as_deref());
    }
    install::put_in_place(&built.app, &built.installed, &STABLE)?;
    println!(
        "\nDry run: {} was not promoted on GitHub, and nothing was signed.\n\n{} is now {}, built and installed here.\n\nAdding --publish would sign {} with {}, attach it, its signature and {INSTALL_SCRIPT} to the {} release, and mark it Latest.",
        built.tag,
        built.installed.display(),
        built.version,
        built.zip.display(),
        built.key.display(),
        built.tag
    );
    Ok(())
}

fn build_and_check(options: &Options) -> Result<Built, String> {
    let repo_root = repo::root()?;
    let Some(tag) = exact_tag(&repo_root) else {
        return Err(
            "HEAD is not exactly on a version tag; check out the tag to promote first".to_string(),
        );
    };
    let Some(version) = Version::parse(&tag) else {
        return Err(format!("{tag} is not a plain version tag"));
    };
    let version_text = version.to_string();

    if let Some(notes_file) = &options.notes_file
        && !notes_file.is_file()
    {
        return Err(format!(
            "there is no notes file at {}",
            notes_file.display()
        ));
    }
    let install_script = repo_root.join(INSTALL_SCRIPT);
    if !install_script.is_file() {
        return Err(format!(
            "there is no {INSTALL_SCRIPT} at the repo root to attach"
        ));
    }
    let uncommitted = uncommitted_changes(&repo_root)?;
    if !uncommitted.is_empty() {
        return Err(format!(
            "the working tree has uncommitted changes, so a build would not be {tag}:\n{uncommitted}"
        ));
    }

    let installed = install::installed_path(&STABLE);
    // A dry run publishes nothing, so a running app would have no release to update to.
    if !options.publish && install::is_running(&installed)? {
        return Err(format!(
            "{} is running. Quit it so this build can replace it, or pass --publish to update the running app.",
            installed.display()
        ));
    }
    let key = key_path(options)?;
    if options.publish {
        check_publishing(&repo_root, &tag, &key)?;
    }

    println!("Promoting {version_text}. Running the tests first.");
    let mut tests = Command::new(repo::cargo_program());
    tests.args(["test", "--workspace", "--locked"]);
    tests.current_dir(&repo_root);
    repo::run(&mut tests)?;
    smoke::run()?;

    let stable_bundle = BundleOptions {
        profile: Profile::Release,
        channel: Channel::Stable,
        zip: true,
        icon: true,
        drive: false,
        config_dir: None,
    };
    let bundled = bundle::create(&stable_bundle)?;
    let Some(zip) = bundled.zip else {
        return Err("the build made no zip".to_string());
    };
    let wanted_zip = release_zip_name(&version_text);
    if !zip.ends_with(&wanted_zip) {
        return Err(format!("the zip is {}, not {wanted_zip}", zip.display()));
    }
    check_stable_bundle(&repo_root, &bundled.app)?;

    Ok(Built {
        repo_root,
        tag,
        version: version_text,
        installed,
        install_script,
        app: bundled.app,
        zip,
        key,
    })
}

fn publish(built: &Built, notes_file: Option<&Path>) -> Result<(), String> {
    // Signed and checked against the app's own keys before anything changes, here or on GitHub.
    let signature = sign(&built.zip, &built.key, &built.version)?;
    let allowed = allowed_signers(RELEASE_KEYS)?;
    verify(&built.zip, &signature, &allowed)?;

    // Asked again now, since the tests and the build take minutes: a running app is left alone and updates itself
    // from the release published below.
    let app_updates_itself = install::is_running(&built.installed)?;
    if !app_updates_itself {
        install::put_in_place(&built.app, &built.installed, &STABLE)?;
    }

    let notes = release_notes(&built.repo_root, &built.tag, notes_file)?;
    let mut upload = Command::new("gh");
    upload.args(["release", "upload", &built.tag]);
    upload.arg(&built.install_script);
    upload.arg(&built.zip);
    upload.arg(&signature);
    upload.arg("--clobber");
    upload.current_dir(&built.repo_root);
    repo::run(&mut upload)?;
    let mut edit = Command::new("gh");
    edit.args([
        "release",
        "edit",
        &built.tag,
        "--prerelease=false",
        "--latest",
    ]);
    edit.args(["--notes", &notes]);
    edit.current_dir(&built.repo_root);
    repo::run(&mut edit)?;

    println!(
        "\nPromoted {}: {} is now Latest, carrying {INSTALL_SCRIPT}, the zip and its signature.",
        built.version, built.tag
    );
    if !app_updates_itself {
        println!("Installed {}.", built.installed.display());
        return Ok(());
    }
    let answered = running_app_answered_an_update_check();
    if answered {
        println!(
            "The running app is checking for it; an Update menu appears once it has downloaded."
        );
    } else {
        println!(
            "The running app did not answer; relaunch it in a minute and it will offer the update."
        );
    }
    Ok(())
}

fn exact_tag(repo_root: &Path) -> Option<String> {
    let mut git = Command::new("git");
    git.args([
        "describe",
        "--tags",
        "--exact-match",
        "--match",
        "v*",
        "HEAD",
    ]);
    git.current_dir(repo_root);
    repo::capture(&mut git)
}

fn uncommitted_changes(repo_root: &Path) -> Result<String, String> {
    let mut git = Command::new("git");
    git.args(["status", "--porcelain"]);
    git.current_dir(repo_root);
    match repo::capture(&mut git) {
        Some(changes) => Ok(changes),
        None => Err("git status failed".to_string()),
    }
}

fn key_path(options: &Options) -> Result<PathBuf, String> {
    if let Some(key) = &options.key {
        return Ok(key.clone());
    }
    let Some(home) = env::home_dir() else {
        return Err("there is no home folder to find the release key in".to_string());
    };
    Ok(home.join(KEY_FOLDER).join(KEY_FILE))
}

fn check_publishing(repo_root: &Path, tag: &str, key: &Path) -> Result<(), String> {
    let mut auth = Command::new("gh");
    auth.args(["auth", "status"]);
    if repo::capture(&mut auth).is_none() {
        return Err("gh is not authenticated; run `gh auth login`".to_string());
    }
    let mut view = Command::new("gh");
    view.args(["release", "view", tag]);
    view.current_dir(repo_root);
    if repo::capture(&mut view).is_none() {
        return Err(format!("there is no GitHub release for {tag} to promote"));
    }
    // Only whether the file is there: its contents are for ssh-keygen alone.
    if !key.is_file() {
        return Err(format!("there is no release key at {}", key.display()));
    }
    Ok(())
}

fn check_stable_bundle(repo_root: &Path, app: &Path) -> Result<(), String> {
    let target = repo::target_dir()?;
    let mut checks = Command::new(repo::cargo_program());
    checks.args(["run", "--quiet", "-p", SMOKE_PACKAGE, "--target-dir"]);
    checks.arg(&target);
    checks.args(["--", "--stable"]);
    checks.arg(app);
    checks.current_dir(repo_root);
    repo::run(&mut checks)
}

// ssh-keygen asks for the key's passphrase on this terminal, or uses the key from ssh-agent when one is loaded.
fn sign(zip: &Path, key: &Path, version: &str) -> Result<PathBuf, String> {
    let Some(folder) = zip.parent() else {
        return Err(format!("{} has no folder", zip.display()));
    };
    let signature = folder.join(release_signature_name(version));
    if signature.exists() {
        fs::remove_file(&signature)
            .map_err(|error| format!("could not remove {}: {error}", signature.display()))?;
    }
    let mut keygen = Command::new(SSH_KEYGEN);
    keygen.args(["-Y", "sign", "-n", RELEASE_SIGNING_NAMESPACE, "-f"]);
    keygen.arg(key);
    keygen.arg(zip);
    repo::run(&mut keygen)?;
    if !signature.is_file() {
        return Err(format!("ssh-keygen made no {}", signature.display()));
    }
    Ok(signature)
}

/// An allowed-signers file naming `keys` for the release namespace only, written fresh with mode 0600.
fn allowed_signers(keys: &[&str]) -> Result<PathBuf, String> {
    let target = repo::target_dir()?;
    let folder = target.join("promote.noindex");
    fs::create_dir_all(&folder)
        .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
    let path = folder.join("allowed_signers");
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
    }
    let text = allowed_signers_text(keys);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true).mode(0o600);
    let mut file = options
        .open(&path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    Ok(path)
}

fn allowed_signers_text(keys: &[&str]) -> String {
    let mut text = String::new();
    for key in keys {
        text.push_str(&format!(
            "{RELEASE_SIGNING_NAMESPACE} namespaces=\"{RELEASE_SIGNING_NAMESPACE}\" {key}\n"
        ));
    }
    text
}

// The same check the app makes before it takes an update, so a release signed with the wrong key never goes out.
fn verify(zip: &Path, signature: &Path, allowed: &Path) -> Result<(), String> {
    let contents =
        File::open(zip).map_err(|error| format!("could not open {}: {error}", zip.display()))?;
    let mut keygen = Command::new(SSH_KEYGEN);
    keygen.args(["-Y", "verify", "-f"]);
    keygen.arg(allowed);
    keygen.args([
        "-I",
        RELEASE_SIGNING_NAMESPACE,
        "-n",
        RELEASE_SIGNING_NAMESPACE,
        "-s",
    ]);
    keygen.arg(signature);
    keygen.stdin(Stdio::from(contents));
    keygen.stdout(Stdio::null());
    keygen.stderr(Stdio::null());
    let status = keygen
        .status()
        .map_err(|error| format!("could not run {SSH_KEYGEN}: {error}"))?;
    if !status.success() {
        return Err(format!(
            "{} does not verify against the app's release keys; nothing was published",
            signature.display()
        ));
    }
    Ok(())
}

fn release_notes(repo_root: &Path, tag: &str, notes_file: Option<&Path>) -> Result<String, String> {
    if let Some(notes_file) = notes_file {
        return fs::read_to_string(notes_file)
            .map_err(|error| format!("could not read {}: {error}", notes_file.display()));
    }
    let before = format!("{tag}^");
    let previous = release::last_tag_at(repo_root, &before)?;
    let messages = release::messages_since(repo_root, previous.as_deref())?;
    let changed = release::change_list(&messages);
    Ok(format!("{changed}\n\n{INSTALL_NOTES}"))
}

// Whether the running stable app answered 200 to an update check on the port it wrote into its config folder. The
// Electron app answers the same request, which is what tells it about the first Rust release.
fn running_app_answered_an_update_check() -> bool {
    let Some(home) = env::home_dir() else {
        return false;
    };
    let port_file = home.join(STABLE.config_directory_name).join(PORT_FILE);
    let Ok(port_text) = fs::read_to_string(port_file) else {
        return false;
    };
    let Ok(port) = port_text.trim().parse::<u16>() else {
        return false;
    };
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, UPDATE_CHECK_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(UPDATE_CHECK_TIMEOUT));
    if stream.write_all(UPDATE_CHECK_REQUEST.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    let status_line = answer.lines().next();
    status_line.is_some_and(|line| line.contains(" 200"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SCRATCH_COUNT: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let count = SCRATCH_COUNT.fetch_add(1, Ordering::Relaxed);
        let name = format!("taco-promote-test-{}-{count}", std::process::id());
        let path = env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn throwaway_key(folder: &Path, name: &str) -> (PathBuf, String) {
        let key = folder.join(name);
        let mut keygen = Command::new(SSH_KEYGEN);
        keygen.args(["-q", "-t", "ed25519", "-N", "", "-C", name, "-f"]);
        keygen.arg(&key);
        let made = keygen.status().unwrap();
        assert!(made.success());
        let public = fs::read_to_string(key.with_extension("pub")).unwrap();
        (key, public.trim().to_string())
    }

    fn signed_with(key: &Path, zip: &Path, namespace: &str) -> PathBuf {
        let mut keygen = Command::new(SSH_KEYGEN);
        keygen.args(["-Y", "sign", "-q", "-n", namespace, "-f"]);
        keygen.arg(key);
        keygen.arg(zip);
        let signed = keygen.status().unwrap();
        assert!(signed.success());
        let mut signature = zip.as_os_str().to_os_string();
        signature.push(".sig");
        PathBuf::from(signature)
    }

    #[test]
    fn a_release_verifies_only_when_a_listed_key_signed_it_for_releases() {
        let folder = scratch();
        let (release_key, release_public) = throwaway_key(&folder, "release");
        let (other_key, _) = throwaway_key(&folder, "other");
        let allowed = folder.join("allowed_signers");
        fs::write(&allowed, allowed_signers_text(&[&release_public])).unwrap();

        let cases = [
            ("listed key", &release_key, RELEASE_SIGNING_NAMESPACE, true),
            ("unlisted key", &other_key, RELEASE_SIGNING_NAMESPACE, false),
            ("listed key, another namespace", &release_key, "file", false),
        ];
        for (name, key, namespace, expected) in cases {
            let zip = folder.join(format!("{}.zip", name.replace(' ', "-")));
            fs::write(&zip, "a release").unwrap();
            let signature = signed_with(key, &zip, namespace);
            let verified = verify(&zip, &signature, &allowed);
            assert_eq!(verified.is_ok(), expected, "{name}");
        }

        let zip = folder.join("changed.zip");
        fs::write(&zip, "a release").unwrap();
        let signature = signed_with(&release_key, &zip, RELEASE_SIGNING_NAMESPACE);
        fs::write(&zip, "another release").unwrap();
        assert!(verify(&zip, &signature, &allowed).is_err(), "a changed zip");
        let _ = fs::remove_dir_all(&folder);
    }
}
