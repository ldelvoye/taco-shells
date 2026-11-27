use serde::{Deserialize, Serialize};
use taco_protocol::{Author, Board, SharedTarget, SharedThing};

/// How many things Shared keeps; the oldest goes as a new one comes.
pub const KEPT_THINGS: usize = 100;

/// The agent's line on what it is doing now, kept with the session that wrote it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NowLine {
    pub session_id: String,
    pub text: String,
}

/// A thing in Shared and its text, which only the state file keeps; the window asks for the text when it opens one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeptThing {
    pub thing: SharedThing,
    pub body: Option<String>,
}

/// What one share adds to Shared.
pub struct Sharing {
    pub by: Author,
    pub kind: String,
    pub title: String,
    pub target: Option<SharedTarget>,
    pub body: Option<String>,
    pub at_ms: u64,
}

/// What a terminal's state file holds, beside its log. Fields missing from the file read as empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TerminalBoard {
    pub now: Option<NowLine>,
    /// Newest first.
    #[serde(default)]
    pub shared: Vec<KeptThing>,
    #[serde(default)]
    pub next_thing: u64,
}

impl TerminalBoard {
    /// The board a state file holds; an unreadable file is an empty board.
    pub fn from_text(text: Option<String>) -> TerminalBoard {
        let Some(text) = text else {
            return TerminalBoard::default();
        };
        let parsed: Result<TerminalBoard, _> = serde_json::from_str(&text);
        let Ok(board) = parsed else {
            return TerminalBoard::default();
        };
        board
    }

    pub fn text(&self) -> Option<String> {
        serde_json::to_string(self).ok()
    }

    /// The board the panel shows while `session` is the terminal's agent session: a now line from another session is hidden.
    pub fn shown(&self, session: Option<&str>) -> Board {
        let mut now = None;
        if let Some(line) = &self.now
            && session == Some(line.session_id.as_str())
        {
            now = Some(line.text.clone());
        }
        let things = self.shared.iter().map(|kept| kept.thing.clone());
        Board {
            now,
            shared: things.collect(),
        }
    }

    /// Puts a shared thing at the top of Shared, replacing an earlier one with the same link, file or pull request,
    /// so sharing it again moves it up. Texts are never merged, since two can share a title and differ.
    pub fn share(&mut self, sharing: Sharing) -> SharedThing {
        let id = self.next_thing;
        self.next_thing += 1;
        let target = match sharing.target {
            Some(target) => target,
            None => text_target(sharing.body.as_deref()),
        };
        if !matches!(target, SharedTarget::Text { .. }) {
            self.shared
                .retain(|kept| !same_place(&kept.thing.target, &target));
        }
        let thing = SharedThing {
            id,
            at_ms: sharing.at_ms,
            by: sharing.by,
            kind: sharing.kind,
            title: sharing.title,
            target,
        };
        let kept = KeptThing {
            thing: thing.clone(),
            body: sharing.body,
        };
        self.shared.insert(0, kept);
        self.shared.truncate(KEPT_THINGS);
        thing
    }

    /// The text a shared thing opens to, or none when it is gone or has none.
    pub fn body_of(&self, thing: u64) -> Option<String> {
        let mut shared = self.shared.iter();
        let kept = shared.find(|kept| kept.thing.id == thing)?;
        kept.body.clone()
    }
}

fn text_target(body: Option<&str>) -> SharedTarget {
    let mut characters = 0;
    if let Some(body) = body {
        characters = body.chars().count() as u64;
    }
    SharedTarget::Text { characters }
}

// The same link, file or pull request. A pull request is known by its link: a number alone can be any repository's.
fn same_place(kept: &SharedTarget, new: &SharedTarget) -> bool {
    match (kept, new) {
        (SharedTarget::Link { url: kept }, SharedTarget::Link { url: new }) => kept == new,
        (SharedTarget::File { path: kept }, SharedTarget::File { path: new }) => kept == new,
        (
            SharedTarget::PullRequest {
                url: Some(kept), ..
            },
            SharedTarget::PullRequest { url: Some(new), .. },
        ) => kept == new,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_sharing(title: String) -> Sharing {
        Sharing {
            by: Author::Agent,
            kind: "Note".into(),
            title,
            target: None,
            body: Some("é".repeat(3)),
            at_ms: 0,
        }
    }

    #[test]
    fn shared_keeps_its_newest_things_and_a_file_without_them_reads_as_none() {
        let earlier = TerminalBoard::from_text(Some(r#"{"now":null}"#.into()));
        assert!(earlier.shared.is_empty(), "a state file from before Shared");

        let mut board = TerminalBoard::default();
        for index in 0..=KEPT_THINGS {
            board.share(text_sharing(index.to_string()));
        }
        let shown = board.shown(None);
        assert_eq!(shown.shared.len(), KEPT_THINGS);
        assert_eq!(
            shown.shared[0].title,
            KEPT_THINGS.to_string(),
            "newest first"
        );
        assert_eq!(
            shown.shared[0].target,
            SharedTarget::Text { characters: 3 },
            "a text is listed by its length in characters"
        );
        assert_eq!(board.body_of(0), None, "the oldest has gone");
        assert_eq!(board.body_of(1).as_deref(), Some("ééé"));
    }

    #[test]
    fn a_pull_request_is_the_same_thing_only_by_its_link() {
        let pull_request = |url: Option<&str>, number: u64| SharedTarget::PullRequest {
            url: url.map(str::to_string),
            number,
            state: taco_protocol::PrState::Open,
            checked_ms: None,
        };
        let titles = |board: &TerminalBoard| {
            let things = board.shared.iter().map(|kept| kept.thing.title.clone());
            things.collect::<Vec<_>>()
        };
        let mut board = TerminalBoard::default();
        let mut share = |title: &str, target: SharedTarget| {
            let mut sharing = text_sharing(title.to_string());
            sharing.target = Some(target);
            board.share(sharing);
        };
        share(
            "ours",
            pull_request(Some("https://github.com/o/r/pull/7"), 7),
        );
        share(
            "theirs",
            pull_request(Some("https://github.com/x/y/pull/7"), 7),
        );
        share(
            "ours again",
            pull_request(Some("https://github.com/o/r/pull/7"), 7),
        );
        share("merged", pull_request(None, 7));
        assert_eq!(
            titles(&board),
            ["merged", "ours again", "theirs"],
            "the same link is one pull request; the same number in another repository, or with no link, is not"
        );
    }
}
