use std::path::Path;

use serde_json::Value;
use taco_protocol::{PrState, SharedTarget};

use super::shared::{
    ARTIFACT_KIND, DOCUMENT_KIND, IMAGE_KIND, LINK_PREFIX, MAX_TARGET_CHARACTERS,
    MAX_TITLE_CHARACTERS, PULL_REQUEST_KIND, pull_request_of, repository_of,
};
use super::text::{text_at, whole_text_at, written, written_at};
use crate::harness::{Found, PullRequestSeen};

const DOCUMENT_EXTENSIONS: &[&str] = &["md", "markdown", "rst", "adoc", "pdf"];
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp"];
// Folders whose files are the agent's or a tool's own, never things for the user.
const NOT_SHARED_FOLDERS: &[&str] = &["/.claude/", "/.git/", "/node_modules/"];
const DRAFT_FLAG: &str = "--draft";

// What the app shares by itself from a tool the agent ran: a pull request, from the gitOperation Claude Code reads
// out of a gh command, a document or image the agent wrote, or an artifact it published.
pub(super) fn found_by_app(parsed: &Value) -> Option<Found> {
    let tool = text_at(parsed, "/tool_name")?;
    match tool.as_str() {
        "Bash" => found_pull_request(parsed),
        "Write" => found_file(parsed),
        "Artifact" => found_artifact(parsed),
        _ => None,
    }
}

fn found_pull_request(parsed: &Value) -> Option<Found> {
    let operation = parsed.pointer("/tool_response/gitOperation/pr")?;
    let number = operation.pointer("/number")?.as_u64()?;
    let action = operation.pointer("/action")?.as_str()?;
    let mut url = None;
    let given = operation.pointer("/url");
    if let Some(Value::String(given)) = given
        && let Some((plain, _)) = pull_request_of(given)
    {
        url = Some(plain);
    }
    // the response never says "draft", so the flag on the whole command does, wherever it comes in a long one
    let command = parsed.pointer("/tool_input/command");
    let mut as_draft = false;
    if let Some(Value::String(command)) = command {
        let mut words = command.split_whitespace();
        as_draft = words.any(|word| word == DRAFT_FLAG);
    }
    let state = match action {
        "created" if as_draft => Some(PrState::Draft),
        "created" | "ready" => Some(PrState::Open),
        "merged" => Some(PrState::Merged),
        "closed" => Some(PrState::Closed),
        "edited" => None,
        _ => return None,
    };
    let mut title = format!("#{number}");
    if let Some(url) = &url
        && let Some(repository) = repository_of(url)
    {
        title = format!("#{number} {repository}");
    }
    Some(Found::PullRequest(PullRequestSeen {
        url,
        number,
        state,
        kind: PULL_REQUEST_KIND.to_string(),
        title,
    }))
}

// Only a new file is shared; rewriting one that exists is not news.
fn found_file(parsed: &Value) -> Option<Found> {
    let write = text_at(parsed, "/tool_response/type")?;
    if write != "create" {
        return None;
    }
    let path = whole_text_at(parsed, "/tool_response/filePath", MAX_TARGET_CHARACTERS)?;
    let file = Path::new(&path);
    let extension = file.extension()?.to_str()?;
    let lowered = extension.to_lowercase();
    let is_document = DOCUMENT_EXTENSIONS.contains(&lowered.as_str());
    let is_image = IMAGE_EXTENSIONS.contains(&lowered.as_str());
    let kind = match (is_document, is_image) {
        (true, _) => DOCUMENT_KIND,
        (false, true) => IMAGE_KIND,
        (false, false) => return None,
    };
    let scratchpad = text_at(parsed, "/scratchpad_dir");
    let in_scratchpad = scratchpad.is_some_and(|folder| path.starts_with(&folder));
    let in_a_tool_folder = NOT_SHARED_FOLDERS
        .iter()
        .any(|folder| path.contains(folder));
    let is_absolute = file.is_absolute();
    if in_scratchpad || in_a_tool_folder || !is_absolute {
        return None;
    }
    let name = file.file_name()?.to_str()?;
    let title = written(name, MAX_TITLE_CHARACTERS)?;
    Some(Found::Thing {
        kind: kind.to_string(),
        title,
        target: SharedTarget::File { path },
    })
}

// A page the agent published; the Artifact tool publishes when it is given no action, and an asset is no page.
fn found_artifact(parsed: &Value) -> Option<Found> {
    let action = text_at(parsed, "/tool_input/action");
    let publishes = action.is_none() || action.as_deref() == Some("publish");
    let asset = parsed.pointer("/tool_input/asset");
    if !publishes || asset == Some(&Value::Bool(true)) {
        return None;
    }
    let url = whole_text_at(parsed, "/tool_response/url", MAX_TARGET_CHARACTERS)?;
    if !url.starts_with(LINK_PREFIX) {
        return None;
    }
    let titled = written_at(parsed, "/tool_response/title", MAX_TITLE_CHARACTERS);
    let title = match titled {
        Some(title) => title,
        None => ARTIFACT_KIND.to_string(),
    };
    Some(Found::Thing {
        kind: ARTIFACT_KIND.to_string(),
        title,
        target: SharedTarget::Link { url },
    })
}

#[cfg(test)]
mod tests {
    use super::super::entries::read_entries;
    use super::*;
    use crate::harness::EntryRead;

    #[test]
    fn the_app_finds_pull_requests_documents_images_and_artifacts_in_the_agents_tools() {
        let found = |payload: serde_json::Value| {
            let mut reads = read_entries("PostToolUse", &payload.to_string());
            let last = reads.pop();
            match last {
                Some(EntryRead::Found(found)) => Some(found),
                _ => None,
            }
        };
        let gh = |command: &str, operation: serde_json::Value| {
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": command },
                "tool_response": { "stdout": "", "gitOperation": { "pr": operation } },
            })
        };
        let seen = |url: Option<&str>, number: u64, state: Option<PrState>| {
            let mut title = format!("#{number}");
            if url.is_some() {
                title = format!("#{number} o/r");
            }
            Found::PullRequest(PullRequestSeen {
                url: url.map(str::to_string),
                number,
                state,
                kind: PULL_REQUEST_KIND.to_string(),
                title,
            })
        };
        let created = serde_json::json!({ "number": 79, "url": "https://github.com/o/r/pull/79", "action": "created" });
        assert_eq!(
            found(gh("gh pr create --title x", created.clone())),
            Some(seen(
                Some("https://github.com/o/r/pull/79"),
                79,
                Some(PrState::Open)
            ))
        );
        assert_eq!(
            found(gh("gh pr create --draft --title x", created.clone())),
            Some(seen(
                Some("https://github.com/o/r/pull/79"),
                79,
                Some(PrState::Draft)
            ))
        );
        let long_body = format!("gh pr create --body '{}' --draft", "b".repeat(2_000));
        let Some(Found::PullRequest(after_a_long_body)) = found(gh(&long_body, created.clone()))
        else {
            panic!("a created pull request is found");
        };
        assert_eq!(
            after_a_long_body.state,
            Some(PrState::Draft),
            "the flag counts wherever it comes"
        );
        let Some(Found::PullRequest(not_a_draft)) =
            found(gh("gh pr create --draft=false --title x", created))
        else {
            panic!("a created pull request is found");
        };
        assert_eq!(
            not_a_draft.state,
            Some(PrState::Open),
            "only the flag itself makes a draft"
        );
        let merged = serde_json::json!({ "number": 79, "action": "merged" });
        assert_eq!(
            found(gh("gh pr merge 79", merged)),
            Some(seen(None, 79, Some(PrState::Merged)))
        );
        let closed = serde_json::json!({ "number": 79, "action": "closed" });
        assert_eq!(
            found(gh("gh pr close 79", closed)),
            Some(seen(None, 79, Some(PrState::Closed)))
        );
        let edited = serde_json::json!({ "number": 79, "url": "https://github.com/o/r/pull/79", "action": "edited" });
        assert_eq!(
            found(gh("gh pr edit 79", edited)),
            Some(seen(Some("https://github.com/o/r/pull/79"), 79, None)),
            "an edit says nothing of the state"
        );
        let commented = serde_json::json!({ "number": 79, "action": "commented" });
        assert_eq!(found(gh("gh pr comment 79", commented)), None);
        let bash = read_entries(
            "PostToolUse",
            &gh(
                "gh pr merge 79",
                serde_json::json!({ "number": 79, "action": "merged" }),
            )
            .to_string(),
        );
        assert!(
            matches!(bash[0], EntryRead::Step { .. }),
            "the command is still a step"
        );

        let wrote = |path: &str, write: &str| {
            serde_json::json!({
                "tool_name": "Write",
                "tool_input": { "file_path": path },
                "tool_response": { "type": write, "filePath": path },
                "scratchpad_dir": "/tmp/scratch",
            })
        };
        let file = |kind: &str, title: &str, path: &str| Found::Thing {
            kind: kind.into(),
            title: title.into(),
            target: SharedTarget::File { path: path.into() },
        };
        assert_eq!(
            found(wrote("/work/PLAN.md", "create")),
            Some(file("Document", "PLAN.md", "/work/PLAN.md"))
        );
        assert_eq!(
            found(wrote("/work/logo.PNG", "create")),
            Some(file("Image", "logo.PNG", "/work/logo.PNG"))
        );
        for not_shared in [
            wrote("/work/PLAN.md", "update"),
            wrote("/work/main.rs", "create"),
            wrote("/work/notes.txt", "create"),
            wrote("/tmp/scratch/page.md", "create"),
            wrote("/Users/x/.claude/projects/memory.md", "create"),
            wrote("/work/.git/COMMIT_EDITMSG.md", "create"),
            wrote("/work/node_modules/a/README.md", "create"),
            wrote("PLAN.md", "create"),
        ] {
            assert_eq!(found(not_shared.clone()), None, "{not_shared}");
        }

        let artifact = |input: serde_json::Value, response: serde_json::Value| serde_json::json!({ "tool_name": "Artifact", "tool_input": input, "tool_response": response });
        let published = serde_json::json!({ "url": "https://claude.ai/artifact/abc", "title": "Window choices", "updated": false });
        let page = Found::Thing {
            kind: "Artifact".into(),
            title: "Window choices".into(),
            target: SharedTarget::Link {
                url: "https://claude.ai/artifact/abc".into(),
            },
        };
        assert_eq!(
            found(artifact(
                serde_json::json!({ "file_path": "/x/index.html" }),
                published.clone()
            )),
            Some(page)
        );
        assert_eq!(
            found(artifact(
                serde_json::json!({ "action": "read" }),
                published.clone()
            )),
            None
        );
        assert_eq!(
            found(artifact(serde_json::json!({ "asset": true }), published)),
            None
        );
        let not_https = serde_json::json!({ "url": "http://claude.ai/artifact/abc", "title": "T" });
        assert_eq!(found(artifact(serde_json::json!({}), not_https)), None);
        let untitled = serde_json::json!({ "url": "https://claude.ai/artifact/abc" });
        let Some(Found::Thing { title, .. }) = found(artifact(serde_json::json!({}), untitled))
        else {
            panic!("an untitled page is still shared");
        };
        assert_eq!(title, ARTIFACT_KIND);
    }
}
