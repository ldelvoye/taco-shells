use std::fmt;

const BREAKING_FOOTERS: [&str; 2] = ["BREAKING CHANGE:", "BREAKING-CHANGE:"];
const FEATURE_TYPE: &str = "feat";

/// The version a repository with no tags yet starts from.
pub const FIRST_VERSION: Version = Version {
    major: 0,
    minor: 1,
    patch: 0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

// Smallest first: `bump_for` takes the largest by this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bump {
    Patch,
    Minor,
    Major,
}

/// A conventional commit subject's type and whether it carries the `!` of a breaking change.
struct Subject<'a> {
    kind: &'a str,
    breaking: bool,
}

impl Version {
    /// `1.2.3` from `1.2.3` or `v1.2.3`; anything else is not a version.
    pub fn parse(text: &str) -> Option<Version> {
        let without_v = match text.strip_prefix('v') {
            Some(rest) => rest,
            None => text,
        };
        let parts: Vec<&str> = without_v.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else {
            return None;
        };
        let major = number(major)?;
        let minor = number(minor)?;
        let patch = number(patch)?;
        Some(Version {
            major,
            minor,
            patch,
        })
    }

    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The bump a set of full commit messages calls for: the largest any one of them asks.
pub fn bump_for(messages: &[String]) -> Bump {
    let mut bump = Bump::Patch;
    for message in messages {
        let asked = bump_of(message);
        if asked > bump {
            bump = asked;
        }
    }
    bump
}

/// Below 1.0.0 a breaking change moves the minor: reaching 1.0.0 is set by hand, never by a commit.
pub fn apply_bump(version: Version, bump: Bump) -> Version {
    match bump {
        Bump::Major if version.major == 0 => Version {
            major: 0,
            minor: version.minor + 1,
            patch: 0,
        },
        Bump::Major => Version {
            major: version.major + 1,
            minor: 0,
            patch: 0,
        },
        Bump::Minor => Version {
            major: version.major,
            minor: version.minor + 1,
            patch: 0,
        },
        Bump::Patch => Version {
            major: version.major,
            minor: version.minor,
            patch: version.patch + 1,
        },
    }
}

pub fn next_version(current: Version, messages: &[String]) -> Version {
    let bump = bump_for(messages);
    apply_bump(current, bump)
}

fn number(text: &str) -> Option<u64> {
    let all_digits = !text.is_empty() && text.chars().all(|digit| digit.is_ascii_digit());
    if !all_digits {
        return None;
    }
    text.parse().ok()
}

fn bump_of(message: &str) -> Bump {
    let first_line = message.lines().next();
    let subject = first_line.and_then(subject_of);
    if has_breaking_footer(message) {
        return Bump::Major;
    }
    let Some(subject) = subject else {
        return Bump::Patch;
    };
    if subject.breaking {
        return Bump::Major;
    }
    if subject.kind == FEATURE_TYPE {
        return Bump::Minor;
    }
    Bump::Patch
}

fn has_breaking_footer(message: &str) -> bool {
    let body = message.lines().skip(1);
    for line in body {
        let is_footer = BREAKING_FOOTERS
            .iter()
            .any(|footer| line.starts_with(footer));
        if is_footer {
            return true;
        }
    }
    false
}

// `type(scope)!: summary`, with the scope and the `!` both optional and the type in lowercase letters.
fn subject_of(line: &str) -> Option<Subject<'_>> {
    let first_other = line.find(|character: char| !character.is_ascii_lowercase());
    let kind_end = match first_other {
        Some(index) => index,
        None => line.len(),
    };
    if kind_end == 0 {
        return None;
    }
    let kind = &line[..kind_end];
    let mut rest = &line[kind_end..];
    if let Some(after_open) = rest.strip_prefix('(') {
        let close = after_open.find(')')?;
        rest = &after_open[close + 1..];
    }
    let breaking = rest.starts_with('!');
    if breaking {
        rest = &rest[1..];
    }
    if !rest.starts_with(':') {
        return None;
    }
    Some(Subject { kind, breaking })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn messages(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|text| text.to_string()).collect()
    }

    #[test]
    fn the_commits_pick_the_bump() {
        let cases = [
            (vec!["feat: split terminals into panes"], Bump::Minor),
            (
                vec!["fix(pty): let shells exit with the process"],
                Bump::Patch,
            ),
            (vec!["chore: bump dependencies"], Bump::Patch),
            (
                vec!["feat(config)!: drop the old keybinding format"],
                Bump::Major,
            ),
            (
                vec!["feat: rework config\n\nBREAKING CHANGE: settings.json moved"],
                Bump::Major,
            ),
            (
                vec![
                    "fix: a cleanup round",
                    "feat: the feature itself",
                    "docs: readme",
                ],
                Bump::Minor,
            ),
            (
                vec!["fix: a tweak\n\nBREAKING-CHANGE: the config format moved"],
                Bump::Major,
            ),
            (
                vec!["feat: a new thing\n\nWith a body whose last line is no subject."],
                Bump::Minor,
            ),
            (
                vec![
                    "docs: say what a BREAKING CHANGE is\n\nMentioning BREAKING CHANGE: mid-line is prose.",
                ],
                Bump::Patch,
            ),
            (vec!["a subject with no type: feat"], Bump::Patch),
        ];
        for (texts, expected) in cases {
            let bump = bump_for(&messages(&texts));
            assert_eq!(bump, expected, "for {texts:?}");
        }
    }

    #[test]
    fn only_a_plain_v_tag_is_a_version() {
        let parsed = Version::parse("v0.29.3");
        assert_eq!(parsed, Some(version("0.29.3")));
        assert_eq!(version("0.30.0").tag(), "v0.30.0");
        for text in ["v1.2", "v1.2.3.4", "v1..3", "v1.2.3-rc1", "1.x.3"] {
            assert_eq!(Version::parse(text), None, "for {text}");
        }
    }

    #[test]
    fn a_bump_resets_what_is_below_it_and_keeps_a_break_below_one_on_the_minor() {
        let cases = [
            ("1.2.3", Bump::Minor, "1.3.0"),
            ("1.2.3", Bump::Major, "2.0.0"),
            ("0.2.3", Bump::Major, "0.3.0"),
        ];
        for (from, bump, to) in cases {
            let bumped = apply_bump(version(from), bump);
            assert_eq!(bumped, version(to), "{from} by {bump:?}");
        }
    }
}
