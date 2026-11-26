use std::path::Path;
use std::process::Command;

use crate::repo;

const FALLBACK_SHORT: &str = "0.0.0";
const FALLBACK_LONG: &str = "unknown";

pub struct BundleVersion {
    pub short: String,
    pub long: String,
}

pub fn current(repo_root: &Path) -> BundleVersion {
    let mut git = Command::new("git");
    git.args(["describe", "--tags", "--match", "v*", "--always", "--dirty"]);
    git.current_dir(repo_root);
    let described = repo::capture(&mut git);
    bundle_version(described.as_deref())
}

pub fn bundle_version(described: Option<&str>) -> BundleVersion {
    let Some(description) = described else {
        return BundleVersion {
            short: FALLBACK_SHORT.to_string(),
            long: FALLBACK_LONG.to_string(),
        };
    };
    let long = match description.strip_prefix('v') {
        Some(without_v) => without_v,
        None => description,
    };
    BundleVersion {
        short: tag_version(description),
        long: long.to_string(),
    }
}

fn tag_version(description: &str) -> String {
    let Some(without_v) = description.strip_prefix('v') else {
        return FALLBACK_SHORT.to_string();
    };
    let tag_end = match without_v.find('-') {
        Some(index) => index,
        None => without_v.len(),
    };
    let tag = &without_v[..tag_end];
    let numbers: Vec<&str> = tag.split('.').collect();
    let all_numeric = numbers
        .iter()
        .all(|number| !number.is_empty() && number.chars().all(|digit| digit.is_ascii_digit()));
    if numbers.len() == 3 && all_numeric {
        return tag.to_string();
    }
    FALLBACK_SHORT.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_version_is_always_a_plain_short_version() {
        let cases = [
            (None, "0.0.0", "unknown"),
            (Some("v1.2.3"), "1.2.3", "1.2.3"),
            (Some("v1.2.3-dirty"), "1.2.3", "1.2.3-dirty"),
            (Some("v0.26.4-14-gabc1234"), "0.26.4", "0.26.4-14-gabc1234"),
            (Some("abc1234"), "0.0.0", "abc1234"),
            (Some("v1.2"), "0.0.0", "1.2"),
            (Some("v1..3"), "0.0.0", "1..3"),
        ];
        for (described, short, long) in cases {
            let version = bundle_version(described);
            assert_eq!(version.short, short, "short for {described:?}");
            assert_eq!(version.long, long, "long for {described:?}");
        }
    }
}
