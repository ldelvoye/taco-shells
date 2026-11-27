// Character caps on what a share carries; a target that does not fit opens nothing.
pub(super) const MAX_TITLE_CHARACTERS: usize = 200;
pub(super) const MAX_TARGET_CHARACTERS: usize = 1_000;
pub(super) const MAX_BODY_CHARACTERS: usize = 20_000;
pub(super) const NOTE_KIND: &str = "Note";
pub(super) const DOCUMENT_KIND: &str = "Document";
pub(super) const IMAGE_KIND: &str = "Image";
pub(super) const PULL_REQUEST_KIND: &str = "Pull request";
pub(super) const ARTIFACT_KIND: &str = "Artifact";
pub(super) const LINK_PREFIX: &str = "https://";
const PULL_REQUEST_PREFIX: &str = "https://github.com/";

/// A pull request's plain link, its number and its "owner/name" repository.
pub(super) struct PullRequestLink {
    pub(super) url: String,
    pub(super) number: u64,
    pub(super) repository: String,
}

// A pull request's plain link, "https://github.com/o/r/pull/155" from ".../pull/155#discussion", so one pull request
// shared from two of its pages is the same thing.
pub(super) fn pull_request_of(url: &str) -> Option<PullRequestLink> {
    let rest = url.strip_prefix(PULL_REQUEST_PREFIX)?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repository = parts.next()?;
    let pull = parts.next()?;
    if owner.is_empty() || repository.is_empty() || pull != "pull" {
        return None;
    }
    let number_part = parts.next()?;
    let characters = number_part.chars();
    let leading_digits = characters.take_while(|character| character.is_ascii_digit());
    let digits: String = leading_digits.collect();
    let after = &number_part[digits.len()..];
    let ends_the_number = after.is_empty() || after.starts_with(['#', '?']);
    if !ends_the_number {
        return None;
    }
    let number: u64 = digits.parse().ok()?;
    let plain = format!("{PULL_REQUEST_PREFIX}{owner}/{repository}/pull/{number}");
    Some(PullRequestLink {
        url: plain,
        number,
        repository: format!("{owner}/{repository}"),
    })
}
