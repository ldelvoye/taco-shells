use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::build::{self, APP_EXECUTABLE, Channel, Profile, RUNTIME_EXECUTABLE};
use crate::identity::{DEVELOPMENT, Identity, STABLE};
use crate::repo;
use crate::version::{self, BundleVersion};

const EXECUTABLES: [&str; 2] = [APP_EXECUTABLE, RUNTIME_EXECUTABLE];

pub struct Options {
    pub profile: Profile,
    pub channel: Channel,
    pub zip: bool,
    pub icon: bool,
}

pub struct Bundled {
    pub app: PathBuf,
    pub zip: Option<PathBuf>,
}

pub fn run(options: &Options) -> Result<(), String> {
    let bundled = create(options)?;
    println!("{}", bundled.app.display());
    if let Some(zip) = bundled.zip {
        println!("{}", zip.display());
    }
    Ok(())
}

pub fn create(options: &Options) -> Result<Bundled, String> {
    let repo_root = repo::root()?;
    let identity = match options.channel {
        Channel::Stable => &STABLE,
        Channel::Development => &DEVELOPMENT,
    };

    let binaries = build::binaries(options.profile, options.channel)?;

    let target = repo::target_dir()?;
    // Spotlight skips a folder ending in .noindex, so the build is not listed as a second copy of the app.
    let output = target
        .join("bundle.noindex")
        .join(options.profile.directory());
    let app = output.join(format!("{}.app", identity.product_name));
    if app.exists() {
        remove_dir(&app)?;
    }

    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    create_dir(&macos)?;
    create_dir(&resources)?;

    for executable in EXECUTABLES {
        copy_file(&binaries.join(executable), &macos.join(executable))?;
    }
    if options.icon {
        let icon = ensure_icon(&repo_root, identity)?;
        copy_file(&icon, &resources.join("icon.icns"))?;
    }
    copy_dir(&repo_root.join("hooks"), &resources.join("hooks"))?;
    let helper = repo_root.join("resources").join("update-helper.sh");
    copy_file(&helper, &resources.join("update-helper.sh"))?;

    let version = version::current(&repo_root);
    let plist = info_plist(identity, &version);
    write_file(&contents.join("Info.plist"), &plist)?;

    // The runtime is signed before the bundle, whose signature seals everything inside it.
    sign(&macos.join(RUNTIME_EXECUTABLE))?;
    sign(&app)?;

    let mut zip = None;
    if options.zip {
        let zip_path = output.join(format!("taco-shells-{}-arm64.zip", version.short));
        make_zip(&app, &zip_path)?;
        zip = Some(zip_path);
    }
    Ok(Bundled { app, zip })
}

pub fn info_plist(identity: &Identity, version: &BundleVersion) -> String {
    let strings = [
        ("CFBundleDevelopmentRegion", "en"),
        ("CFBundleDisplayName", identity.product_name),
        ("CFBundleExecutable", APP_EXECUTABLE),
        ("CFBundleIconFile", "icon"),
        ("CFBundleIdentifier", identity.bundle_id),
        ("CFBundleInfoDictionaryVersion", "6.0"),
        ("CFBundleName", identity.product_name),
        ("CFBundlePackageType", "APPL"),
        ("CFBundleShortVersionString", version.short.as_str()),
        ("CFBundleVersion", version.long.as_str()),
        (
            "LSApplicationCategoryType",
            "public.app-category.developer-tools",
        ),
        ("LSMinimumSystemVersion", "12.0"),
    ];
    let booleans = [
        ("NSHighResolutionCapable", true),
        ("NSRequiresAquaSystemAppearance", false),
    ];

    let mut plist = String::new();
    plist.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    plist.push_str("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ");
    plist.push_str("\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
    plist.push_str("<plist version=\"1.0\">\n<dict>\n");
    for (key, value) in strings {
        let escaped = escape_xml(value);
        plist.push_str(&format!(
            "\t<key>{key}</key>\n\t<string>{escaped}</string>\n"
        ));
    }
    for (key, value) in booleans {
        plist.push_str(&format!("\t<key>{key}</key>\n\t<{value}/>\n"));
    }
    plist.push_str("</dict>\n</plist>\n");
    plist
}

fn escape_xml(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn ensure_icon(repo_root: &Path, identity: &Identity) -> Result<PathBuf, String> {
    let icon = repo_root.join(identity.icon);
    if !icon.exists() {
        let mut script = Command::new("bash");
        script.arg("branding/build-icon.sh");
        script.current_dir(repo_root);
        repo::run(&mut script)?;
    }
    if !icon.exists() {
        return Err(format!("{} was not built", icon.display()));
    }
    Ok(icon)
}

fn sign(path: &Path) -> Result<(), String> {
    let mut codesign = Command::new("codesign");
    codesign.args(["--force", "--sign", "-"]);
    codesign.arg(path);
    repo::run(&mut codesign)
}

fn make_zip(app: &Path, zip: &Path) -> Result<(), String> {
    if zip.exists() {
        fs::remove_file(zip)
            .map_err(|error| format!("could not remove {}: {error}", zip.display()))?;
    }
    let mut ditto = Command::new("ditto");
    ditto.args(["-c", "-k", "--keepParent"]);
    ditto.arg(app);
    ditto.arg(zip);
    repo::run(&mut ditto)
}

fn create_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path)
        .map_err(|error| format!("could not create {}: {error}", path.display()))
}

fn remove_dir(path: &Path) -> Result<(), String> {
    fs::remove_dir_all(path)
        .map_err(|error| format!("could not remove {}: {error}", path.display()))
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    fs::write(path, contents)
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn copy_file(from: &Path, to: &Path) -> Result<(), String> {
    fs::copy(from, to).map_err(|error| {
        format!(
            "could not copy {} to {}: {error}",
            from.display(),
            to.display()
        )
    })?;
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    create_dir(to)?;
    let entries = fs::read_dir(from)
        .map_err(|error| format!("could not read {}: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("could not read {}: {error}", from.display()))?;
        let source = entry.path();
        let destination = to.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("could not inspect {}: {error}", source.display()))?;
        if file_type.is_dir() {
            copy_dir(&source, &destination)?;
        } else {
            copy_file(&source, &destination)?;
        }
    }
    Ok(())
}
