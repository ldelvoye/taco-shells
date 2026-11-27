use std::env;
use std::path::Path;
use std::process::Command;

use crate::bump::{self, FIRST_VERSION, Version};
use crate::repo;

const TARGET_VARIABLE: &str = "GITHUB_SHA";
const NO_DOWNLOAD_NOTE: &str = "This release is a point in main's history, not a build to install. It carries no download: \
a build is large, and `cargo xtask install` makes one and installs it as **Taco Shells Dev**, beside the stable app \
rather than over it.

The current stable version is whichever release GitHub marks as Latest.";

/// Cuts the push CI is building as a prerelease with no download; `promote` is what later makes one stable. A dry run
/// prints what it would cut and touches nothing.
pub fn run(dry_run: bool) -> Result<(), String> {
    let configured = env::var_os(TARGET_VARIABLE);
    let target = match configured {
        Some(target) => target.to_string_lossy().into_owned(),
        None if dry_run => "HEAD".to_string(),
        None => {
            return Err(format!(
                "{TARGET_VARIABLE} is not set; this is meant to run in CI, or with --dry-run"
            ));
        }
    };
    let repo_root = repo::root()?;

    let previous = last_tag(&repo_root);
    let changes = subjects_since(&repo_root, previous.as_deref())?;
    if changes.is_empty() {
        println!("nothing new since {previous:?}. Nothing to release.");
        return Ok(());
    }

    let version = match &previous {
        Some(tag) => {
            let Some(current) = Version::parse(tag) else {
                return Err(format!("the last tag {tag} is not a plain version"));
            };
            let messages = messages_since(&repo_root, Some(tag))?;
            bump::next_version(current, &messages)
        }
        None => FIRST_VERSION,
    };
    let tag = version.tag();

    // Running a workflow again must not fail on the tag it already made.
    if tag_exists(&repo_root, &tag) {
        println!("{tag} already exists. Nothing to do.");
        return Ok(());
    }

    let changed = change_list(&changes);
    let notes = format!("{changed}\n\n{NO_DOWNLOAD_NOTE}");
    let title = version.to_string();
    if dry_run {
        println!(
            "would release {tag} at {target} as a prerelease titled {title}, with these notes:\n\n{notes}"
        );
        return Ok(());
    }
    let mut gh = Command::new("gh");
    gh.args([
        "release",
        "create",
        &tag,
        "--prerelease",
        "--target",
        &target,
    ]);
    gh.args(["--title", &title, "--notes", &notes]);
    gh.current_dir(&repo_root);
    repo::run(&mut gh)?;

    println!("released {tag} at {target} (unstable)");
    Ok(())
}

/// The commit subjects since a tag, one per commit, as a markdown "Changes" section.
pub fn change_list(subjects: &[String]) -> String {
    if subjects.is_empty() {
        return "## Changes\n\nNothing since the last release.".to_string();
    }
    let bullets: Vec<String> = subjects
        .iter()
        .map(|subject| format!("- {subject}"))
        .collect();
    let joined = bullets.join("\n");
    format!("## Changes\n\n{joined}")
}

/// The most recent version tag reachable from `commit`, or none before the first one.
pub fn last_tag_before(repo_root: &Path, commit: &str) -> Option<String> {
    let mut git = Command::new("git");
    git.args(["describe", "--tags", "--abbrev=0", "--match", "v*", commit]);
    git.current_dir(repo_root);
    repo::capture(&mut git)
}

pub fn subjects_since(repo_root: &Path, tag: Option<&str>) -> Result<Vec<String>, String> {
    let log = git_log(repo_root, "--format=%s", tag)?;
    let lines = log.lines();
    let subjects = lines.filter(|subject| !subject.is_empty());
    let owned: Vec<String> = subjects.map(String::from).collect();
    Ok(owned)
}

fn last_tag(repo_root: &Path) -> Option<String> {
    last_tag_before(repo_root, "HEAD")
}

// Whole messages, not subjects: a conventional commit can declare a break in its body. NUL-separated, because a
// message may hold any other byte, blank lines included.
fn messages_since(repo_root: &Path, tag: Option<&str>) -> Result<Vec<String>, String> {
    let log = git_log(repo_root, "--format=%B%x00", tag)?;
    let messages = log.split('\0');
    let trimmed = messages.map(str::trim);
    let kept = trimmed.filter(|message| !message.is_empty());
    let owned: Vec<String> = kept.map(String::from).collect();
    Ok(owned)
}

fn git_log(repo_root: &Path, format: &str, tag: Option<&str>) -> Result<String, String> {
    let range = match tag {
        Some(tag) => format!("{tag}..HEAD"),
        None => "HEAD".to_string(),
    };
    let mut git = Command::new("git");
    git.args(["log", format, &range]);
    git.current_dir(repo_root);
    match repo::capture(&mut git) {
        Some(log) => Ok(log),
        None => Err(format!("git log {range} failed")),
    }
}

fn tag_exists(repo_root: &Path, tag: &str) -> bool {
    let reference = format!("refs/tags/{tag}");
    let mut git = Command::new("git");
    git.args(["rev-parse", "--verify", "--quiet", &reference]);
    git.current_dir(repo_root);
    repo::capture(&mut git).is_some()
}
