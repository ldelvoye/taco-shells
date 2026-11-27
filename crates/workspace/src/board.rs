use serde::{Deserialize, Serialize};
use taco_agents::PullRequestSeen;
use taco_protocol::{Author, Board, PrState, SharedTarget, SharedThing};

/// How many things Shared keeps; the oldest goes as a new one comes.
pub const KEPT_THINGS: usize = 100;

/// What seeing a pull request did to Shared.
#[derive(Debug, PartialEq)]
pub enum Seen {
    Added(SharedThing),
    Updated,
    Nothing,
}

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
        let mut target = match sharing.target {
            Some(target) => target,
            None => text_target(sharing.body.as_deref()),
        };
        if !matches!(target, SharedTarget::Text { .. }) {
            let mut kept_things = self.shared.iter();
            let replaced = kept_things.find(|kept| same_place(&kept.thing.target, &target));
            if let Some(replaced) = replaced {
                keep_seen_state(&replaced.thing.target, &mut target);
            }
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

    /// A pull request a gh command touched, seen at `now_ms`: the one kept for it takes the state it was seen in,
    /// and one the app has not kept is added when the command gave its link.
    pub fn pull_request_seen(&mut self, seen: PullRequestSeen, now_ms: u64) -> Seen {
        if let Some(index) = self.pull_request_index(seen.url.as_deref(), seen.number) {
            // an edit says nothing of the state, so there is nothing to change
            let Some(state) = seen.state else {
                return Seen::Nothing;
            };
            let SharedTarget::PullRequest {
                state: kept_state,
                checked_ms,
                ..
            } = &mut self.shared[index].thing.target
            else {
                return Seen::Nothing;
            };
            *kept_state = state;
            *checked_ms = Some(now_ms);
            return Seen::Updated;
        }
        // a merge or close names only a number, which on its own could be any repository's
        let Some(url) = seen.url else {
            return Seen::Nothing;
        };
        // an edit's state is a guess until something sees it
        let mut state = PrState::Open;
        let mut checked_ms = None;
        if let Some(seen_state) = seen.state {
            state = seen_state;
            checked_ms = Some(now_ms);
        }
        let sharing = Sharing {
            by: Author::App,
            kind: seen.kind,
            title: seen.title,
            target: Some(SharedTarget::PullRequest {
                url: Some(url),
                number: seen.number,
                state,
                checked_ms,
            }),
            body: None,
            at_ms: now_ms,
        };
        let thing = self.share(sharing);
        Seen::Added(thing)
    }

    // The pull request kept with this link. With no link, the one kept with this number, only when no other kept
    // pull request has that number: two would be two repositories', and the command does not say which.
    fn pull_request_index(&self, url: Option<&str>, number: u64) -> Option<usize> {
        let is_pull_request = |index: &usize| {
            let target = &self.shared[*index].thing.target;
            matches!(target, SharedTarget::PullRequest { .. })
        };
        let indices = 0..self.shared.len();
        let mut pull_requests = indices.filter(is_pull_request);
        if let Some(url) = url {
            return pull_requests.find(|index| {
                let target = &self.shared[*index].thing.target;
                matches!(target, SharedTarget::PullRequest { url: Some(kept), .. } if kept == url)
            });
        }
        let with_the_number: Vec<usize> = pull_requests
            .filter(|index| {
                let target = &self.shared[*index].thing.target;
                matches!(target, SharedTarget::PullRequest { number: kept, .. } if *kept == number)
            })
            .collect();
        match with_the_number.as_slice() {
            [only] => Some(*only),
            _ => None,
        }
    }
}

fn text_target(body: Option<&str>) -> SharedTarget {
    let mut characters = 0;
    if let Some(body) = body {
        characters = body.chars().count() as u64;
    }
    SharedTarget::Text { characters }
}

// A share never sees a pull request's state, so a state the app saw outlasts sharing the pull request again.
fn keep_seen_state(replaced: &SharedTarget, target: &mut SharedTarget) {
    let SharedTarget::PullRequest {
        state: seen_state,
        checked_ms: Some(seen_ms),
        ..
    } = replaced
    else {
        return;
    };
    let SharedTarget::PullRequest {
        state, checked_ms, ..
    } = target
    else {
        return;
    };
    if checked_ms.is_some() {
        return;
    }
    *state = *seen_state;
    *checked_ms = Some(*seen_ms);
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

    #[test]
    fn a_seen_pull_request_takes_its_state_by_link_or_by_a_number_only_one_kept_has() {
        let seen = |url: Option<&str>, number: u64, state: Option<PrState>| PullRequestSeen {
            url: url.map(str::to_string),
            number,
            state,
            kind: "Pull request".into(),
            title: format!("#{number}"),
        };
        let states = |board: &TerminalBoard| {
            let things = board
                .shared
                .iter()
                .filter_map(|kept| match &kept.thing.target {
                    SharedTarget::PullRequest {
                        url,
                        state,
                        checked_ms,
                        ..
                    } => Some((url.clone(), *state, *checked_ms)),
                    _ => None,
                });
            things.collect::<Vec<_>>()
        };
        let ours = "https://github.com/o/r/pull/7";
        let theirs = "https://github.com/x/y/pull/7";
        let mut board = TerminalBoard::default();
        let added = board.pull_request_seen(seen(Some(ours), 7, Some(PrState::Open)), 10);
        assert!(matches!(added, Seen::Added(_)));
        let merged_alone = board.pull_request_seen(seen(None, 7, Some(PrState::Merged)), 20);
        assert_eq!(
            merged_alone,
            Seen::Updated,
            "one kept #7, so the number is enough"
        );

        board.pull_request_seen(seen(Some(theirs), 7, Some(PrState::Draft)), 30);
        let ambiguous = board.pull_request_seen(seen(None, 7, Some(PrState::Closed)), 40);
        assert_eq!(
            ambiguous,
            Seen::Nothing,
            "two kept #7s, so a bare number changes neither"
        );
        let by_link = board.pull_request_seen(seen(Some(theirs), 7, Some(PrState::Open)), 50);
        assert_eq!(by_link, Seen::Updated);
        let edited = board.pull_request_seen(seen(Some(ours), 7, None), 60);
        assert_eq!(edited, Seen::Nothing, "an edit changes no state");
        assert_eq!(
            states(&board),
            vec![
                (Some(theirs.to_string()), PrState::Open, Some(50)),
                (Some(ours.to_string()), PrState::Merged, Some(20)),
            ]
        );
        let unknown = "https://github.com/o/r/pull/8";
        board.pull_request_seen(seen(Some(unknown), 8, None), 70);
        let (_, state, checked_ms) = states(&board)[0].clone();
        assert_eq!(
            (state, checked_ms),
            (PrState::Open, None),
            "a new one seen only in an edit has no state anyone saw"
        );

        // the app sees a draft opened, then the agent shares the same pull request
        let drafted = "https://github.com/o/r/pull/9";
        board.pull_request_seen(seen(Some(drafted), 9, Some(PrState::Draft)), 80);
        let shared_as = |state: PrState, checked_ms: Option<u64>| {
            let mut sharing = text_sharing("Fix the logo (draft)".into());
            sharing.target = Some(SharedTarget::PullRequest {
                url: Some(drafted.into()),
                number: 9,
                state,
                checked_ms,
            });
            sharing
        };
        let shared = board.share(shared_as(PrState::Open, None));
        assert_eq!(shared.title, "Fix the logo (draft)", "the agent's words");
        assert_eq!(
            states(&board)[0],
            (Some(drafted.to_string()), PrState::Draft, Some(80)),
            "the state the app saw outlasts the share"
        );
        board.share(shared_as(PrState::Merged, Some(90)));
        assert_eq!(
            states(&board)[0],
            (Some(drafted.to_string()), PrState::Merged, Some(90)),
            "a state seen later wins"
        );
    }
}
