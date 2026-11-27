use std::env;
use std::path::Path;
use std::process::Command;

use crate::bump::{self, FIRST_VERSION, Version};
use crate::repo;

const TARGET_VARIABLE: &str = "GITHUB_SHA";
const LISTED_CHANGES: usize = 500;
const NO_DOWNLOAD_NOTE: &str = "This release is a point in main's history, not a build to install. It carries no download: a build is large, and `cargo xtask install` makes one and installs it as **Taco Shells Dev**, beside the stable app rather than over it.\n\nThe current stable version is whichever release GitHub marks as Latest.";

/// Cuts HEAD as a prerelease with no download. A dry run prints what it would cut and touches nothing.
pub fn run(dry_run: bool) -> Result<(), String> {
    let repo_root = repo::root()?;
    let head = resolved_head(&repo_root)?;
    // The release is cut where CI checked out, and the version and notes are read from that same commit.
    let configured = env::var(TARGET_VARIABLE);
    match configured {
        Ok(target) if target == head => {}
        Ok(target) => {
            return Err(format!("{TARGET_VARIABLE} is {target} but HEAD is {head}"));
        }
        Err(_) if dry_run => {}
        Err(_) => {
            return Err(format!(
                "{TARGET_VARIABLE} is not set; this is meant to run in CI, or with --dry-run"
            ));
        }
    }

    // A shallow clone has neither the tags nor the history, so it would cut the wrong version.
    if is_shallow(&repo_root)? {
        return Err(
            "the checkout is shallow; fetch the whole history and its tags first".to_string(),
        );
    }
    let previous = last_tag(&repo_root)?;
    let messages = messages_since(&repo_root, previous.as_deref())?;
    if messages.is_empty() {
        match &previous {
            Some(tag) => println!("nothing new since {tag}. Nothing to release."),
            None => println!("no commits. Nothing to release."),
        }
        return Ok(());
    }

    let version = match &previous {
        Some(tag) => {
            let Some(current) = Version::parse(tag) else {
                return Err(format!(
                    "the last tag {tag} is not a plain version, as every tag on this line must be"
                ));
            };
            bump::next_version(current, &messages)
        }
        None => FIRST_VERSION,
    };
    let tag = version.tag();

    if tag_exists(&repo_root, &tag)? {
        return Err(format!(
            "{tag} already exists but is not reachable from HEAD"
        ));
    }

    let changed = change_list(&messages);
    let notes = format!("{changed}\n\n{NO_DOWNLOAD_NOTE}");
    let title = version.to_string();
    if dry_run {
        println!(
            "would release {tag} at {head} as a prerelease titled {title}, with these notes:\n\n{notes}"
        );
        return Ok(());
    }
    let mut gh = Command::new("gh");
    gh.args(["release", "create", &tag, "--prerelease", "--target", &head]);
    gh.args(["--title", &title, "--notes", &notes]);
    gh.current_dir(&repo_root);
    repo::run(&mut gh)?;

    println!("released {tag} at {head} (unstable)");
    Ok(())
}

// Capped so a long gap between tags stays well under GitHub's 125,000-character release body.
pub(crate) fn change_list(messages: &[String]) -> String {
    let subjects = messages.iter().filter_map(|message| message.lines().next());
    let listed = subjects.take(LISTED_CHANGES);
    let mut bullets: Vec<String> = listed.map(|subject| format!("- {subject}")).collect();
    if messages.len() > LISTED_CHANGES {
        let unlisted = messages.len() - LISTED_CHANGES;
        bullets.push(format!("- and {unlisted} older commits"));
    }
    let joined = bullets.join("\n");
    format!("## Changes\n\n{joined}")
}

fn is_shallow(repo_root: &Path) -> Result<bool, String> {
    let mut git = Command::new("git");
    git.args(["rev-parse", "--is-shallow-repository"]);
    git.current_dir(repo_root);
    match repo::capture(&mut git) {
        Some(answer) => Ok(answer == "true"),
        None => Err("git rev-parse --is-shallow-repository failed".to_string()),
    }
}

fn resolved_head(repo_root: &Path) -> Result<String, String> {
    let mut git = Command::new("git");
    git.args(["rev-parse", "HEAD"]);
    git.current_dir(repo_root);
    match repo::capture(&mut git) {
        Some(head) => Ok(head),
        None => Err("git rev-parse HEAD failed".to_string()),
    }
}

fn last_tag(repo_root: &Path) -> Result<Option<String>, String> {
    last_tag_at(repo_root, "HEAD")
}

// `git describe` fails both when there is no tag and when git itself fails, so a failure counts as "no tag yet"
// only when git lists no version tag at all.
/// The newest version tag at or behind `commit`.
pub(crate) fn last_tag_at(repo_root: &Path, commit: &str) -> Result<Option<String>, String> {
    let mut describe = Command::new("git");
    describe.args(["describe", "--tags", "--abbrev=0", "--match", "v*", commit]);
    describe.current_dir(repo_root);
    if let Some(tag) = repo::capture(&mut describe) {
        return Ok(Some(tag));
    }
    let listed = version_tags_named(repo_root, "v*")?;
    if listed.is_empty() {
        return Ok(None);
    }
    Err(format!(
        "git describe found no version tag behind {commit}, though the repo has some"
    ))
}

fn tag_exists(repo_root: &Path, tag: &str) -> Result<bool, String> {
    let listed = version_tags_named(repo_root, tag)?;
    Ok(!listed.is_empty())
}

fn version_tags_named(repo_root: &Path, pattern: &str) -> Result<String, String> {
    let mut git = Command::new("git");
    git.args(["tag", "--list", pattern]);
    git.current_dir(repo_root);
    match repo::capture(&mut git) {
        Some(listed) => Ok(listed),
        None => Err(format!("git tag --list {pattern} failed")),
    }
}

// Whole messages, not subjects: a conventional commit can declare a break in its body.
pub(crate) fn messages_since(repo_root: &Path, tag: Option<&str>) -> Result<Vec<String>, String> {
    let range = match tag {
        Some(tag) => format!("{tag}..HEAD"),
        None => "HEAD".to_string(),
    };
    let mut git = Command::new("git");
    git.args(["log", "--format=%B%x00", &range]);
    git.current_dir(repo_root);
    let Some(log) = repo::capture(&mut git) else {
        return Err(format!("git log {range} failed"));
    };
    let messages = log.split('\0');
    let trimmed = messages.map(str::trim);
    let kept = trimmed.filter(|message| !message.is_empty());
    let owned: Vec<String> = kept.map(String::from).collect();
    Ok(owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GITHUB_BODY_LIMIT: usize = 125_000;

    #[test]
    fn a_long_gap_between_tags_still_fits_a_release_body_and_says_what_it_left_out() {
        let long_subject = format!("feat: {}", "x".repeat(200));
        let messages = vec![long_subject; 3_000];
        let listed = change_list(&messages);
        let notes = format!("{listed}\n\n{NO_DOWNLOAD_NOTE}");
        assert!(
            notes.len() < GITHUB_BODY_LIMIT,
            "{} characters",
            notes.len()
        );
        assert!(listed.ends_with("and 2500 older commits"), "{listed:.200}");
    }
}
