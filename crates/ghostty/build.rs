//! Fetches Ghostty's prebuilt libghostty-vt into a cache directory shared by every checkout, and links it statically.

use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

const VERSION: &str = "1.0.2";
/// The hash listed for this zip in the release's signed `SHA256SUMS`.
const SHA256: &str = "15477209ce86ad1c944f217e925c84acf796ecc51aba7a935aaa1a7769890bd6";
const MACOS_SLICE: &str = "ghostty-vt.xcframework/macos-arm64_x86_64";
const DIRECTORY_VARIABLE: &str = "TACO_GHOSTTY_DIR";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={DIRECTORY_VARIABLE}");

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("cargo sets the target");
    if target_os != "macos" {
        panic!(
            "Ghostty publishes a static libghostty-vt for macOS only; for {target_os}, build it from source with Zig and set {DIRECTORY_VARIABLE}"
        );
    }

    let directory = library_directory();
    if !is_complete(&directory) {
        // Never cleared to make room: it may be a directory someone named by mistake.
        if directory.exists() {
            panic!(
                "{} exists but does not hold lib/libghostty-vt.a and include/ghostty/vt.h. Remove it, or set {DIRECTORY_VARIABLE} elsewhere",
                directory.display()
            );
        }
        fetch_into(&directory);
    }

    let lib = directory.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");
    // For the bindings generator in src/bin, so it reads the headers of the library it links.
    let include = directory.join("include");
    println!("cargo:rustc-env=TACO_GHOSTTY_INCLUDE={}", include.display());
}

fn library_directory() -> PathBuf {
    if let Some(chosen) = env::var_os(DIRECTORY_VARIABLE) {
        return PathBuf::from(chosen);
    }
    let home = env::var_os("HOME")
        .expect("HOME is not set, so the library has no cache directory: set TACO_GHOSTTY_DIR");
    let short_hash = &SHA256[..8];
    let name = format!("libghostty-vt-{VERSION}-{short_hash}");
    Path::new(&home)
        .join("Library/Caches/taco-shells")
        .join(name)
}

fn is_complete(directory: &Path) -> bool {
    let library = directory.join("lib/libghostty-vt.a");
    let header = directory.join("include/ghostty/vt.h");
    library.is_file() && header.is_file()
}

fn fetch_into(directory: &Path) {
    let parent = directory
        .parent()
        .expect("the library directory has a parent");
    fs::create_dir_all(parent).expect("create the library's cache directory");

    // Several checkouts can build at once: one fetches while the others wait, then find it done.
    let lock_path = parent.join(format!("libghostty-vt-{VERSION}.lock"));
    let lock = File::create(&lock_path).expect("create the fetch lock");
    lock.lock().expect("take the fetch lock");
    if directory.exists() {
        return;
    }

    let staging = parent.join(format!("libghostty-vt-{VERSION}.partial"));
    if staging.exists() {
        fs::remove_dir_all(&staging).expect("remove an earlier unfinished fetch");
    }
    fs::create_dir_all(&staging).expect("create the staging directory");

    let zip = staging.join("ghostty-vt.xcframework.zip");
    let url = format!(
        "https://release.files.ghostty.org/libghostty-vt/{VERSION}/ghostty-vt-{VERSION}.xcframework.zip"
    );
    run(Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--output",
        ])
        .arg(&zip)
        .arg(&url));
    check_hash(&zip);

    let unpacked = staging.join("unpacked");
    run(Command::new("unzip")
        .arg("-q")
        .arg(&zip)
        .arg("-d")
        .arg(&unpacked));

    let slice = unpacked.join(MACOS_SLICE);
    let lib = staging.join("lib");
    let include = staging.join("include");
    // Only the static library goes in lib/: beside a dylib, the linker would take the dylib.
    fs::create_dir_all(&lib).expect("create lib/");
    fs::copy(slice.join("libghostty-vt.a"), lib.join("libghostty-vt.a"))
        .expect("copy the static library");
    copy_tree(&slice.join("Headers"), &include);
    fs::remove_dir_all(&unpacked).expect("remove the unpacked zip");
    fs::remove_file(&zip).expect("remove the zip");

    fs::rename(&staging, directory).expect("move the fetched library into place");
}

fn check_hash(zip: &Path) {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(zip)
        .output()
        .expect("run shasum");
    if !output.status.success() {
        panic!("shasum failed on {} ({})", zip.display(), output.status);
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    let Some(found) = listing.split_whitespace().next() else {
        panic!("shasum printed nothing for {}", zip.display());
    };
    if found != SHA256 {
        panic!(
            "{} has sha256 {found}, not the pinned {SHA256}",
            zip.display()
        );
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create a header directory");
    let entries = fs::read_dir(from).expect("read a header directory");
    for entry in entries {
        let entry = entry.expect("read a header entry");
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target);
        } else {
            fs::copy(&source, &target).expect("copy a header");
        }
    }
}

fn run(command: &mut Command) {
    let status = command.status().expect("start a fetch step");
    if !status.success() {
        panic!(
            "{command:?} failed ({status}). Offline? Set TACO_GHOSTTY_DIR to a directory holding lib/libghostty-vt.a and include/ghostty/vt.h"
        );
    }
}
