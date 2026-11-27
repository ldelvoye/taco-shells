use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use taco_identity::{
    APP_PROGRAM, INFO_PLIST_PATH, Identity, PLUGIN_PROGRAM, PROGRAMS_PATH, RESOURCES_PATH,
    RUNTIME_PROGRAM, UPDATE_HELPER_PATH, release_zip_name,
};

use crate::build::{self, Build, Channel, Profile};
use crate::identity::{icon_of, identity_of};
use crate::repo;
use crate::version::{self, BundleVersion};

const PROGRAMS: [&str; 3] = [APP_PROGRAM, RUNTIME_PROGRAM, PLUGIN_PROGRAM];
const NESTED_PROGRAMS: [&str; 2] = [RUNTIME_PROGRAM, PLUGIN_PROGRAM];

pub struct Options {
    pub profile: Profile,
    pub channel: Channel,
    pub zip: bool,
    pub icon: bool,
    pub drive: bool,
    pub config_dir: Option<PathBuf>,
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
    let identity = identity_of(options.channel);

    let build = Build {
        profile: options.profile,
        channel: options.channel,
        drive: options.drive,
        config_dir: options.config_dir.as_deref(),
    };
    let binaries = build::binaries(&build)?;

    let target = repo::target_dir()?;
    // Spotlight skips a folder ending in .noindex, so the build is not listed as a second copy of the app.
    let output = target
        .join("bundle.noindex")
        .join(options.profile.directory());
    let app = output.join(format!("{}.app", identity.application_name));
    if app.exists() {
        remove_dir(&app)?;
    }

    let programs = app.join(PROGRAMS_PATH);
    let resources = app.join(RESOURCES_PATH);
    create_dir(&programs)?;
    create_dir(&resources)?;

    for program in PROGRAMS {
        copy_file(&binaries.join(program), &programs.join(program))?;
    }
    if options.icon {
        let icon = ensure_icon(&repo_root, options.channel)?;
        copy_file(&icon, &resources.join("icon.icns"))?;
    }
    let helper = repo_root.join("resources").join("update-helper.sh");
    copy_file(&helper, &app.join(UPDATE_HELPER_PATH))?;

    let version = version::current(&repo_root);
    let plist = info_plist(identity, &version);
    write_file(&app.join(INFO_PLIST_PATH), &plist)?;

    // The runtime and the plugin program are signed before the bundle, whose signature seals everything inside it.
    for program in NESTED_PROGRAMS {
        sign(&programs.join(program))?;
    }
    sign(&app)?;

    let mut zip = None;
    if options.zip {
        let zip_name = release_zip_name(&version.short);
        let zip_path = output.join(zip_name);
        make_zip(&app, &zip_path)?;
        zip = Some(zip_path);
    }
    Ok(Bundled { app, zip })
}

pub fn info_plist(identity: &Identity, version: &BundleVersion) -> String {
    let strings = [
        ("CFBundleDevelopmentRegion", "en"),
        ("CFBundleDisplayName", identity.application_name),
        ("CFBundleExecutable", APP_PROGRAM),
        ("CFBundleIconFile", "icon"),
        ("CFBundleIdentifier", identity.bundle_id),
        ("CFBundleInfoDictionaryVersion", "6.0"),
        ("CFBundleName", identity.application_name),
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

fn ensure_icon(repo_root: &Path, channel: Channel) -> Result<PathBuf, String> {
    let icon = repo_root.join(icon_of(channel));
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
