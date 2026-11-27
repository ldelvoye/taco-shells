//! The words a stretch's steps are told in: one line per step, searches told as one, and the fold's label.

use taco_protocol::Step;

use super::peer_name;
use crate::activity_log::{Stretch, StretchStep};

/// A step as the fold draws it: its words, and the first line of its error when it failed.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct StepLine {
    pub(super) text: String,
    pub(super) failed: Option<String>,
}

fn step_text(step: &StretchStep) -> String {
    match step {
        StretchStep::Step(Step::Did { tool, summary, .. }) => match tool.as_str() {
            "Bash" => format!("Ran {summary}"),
            "Read" => format!("Read {summary}"),
            "Grep" | "Glob" => format!("Searched for {summary}"),
            _ => format!("{tool}: {summary}"),
        },
        StretchStep::Step(Step::Changed { tool, path }) => match tool.as_str() {
            "Write" => format!("Wrote {path}"),
            _ => format!("Edited {path}"),
        },
        StretchStep::SentMessage(peer) => format!("Sent a message to {}", peer_name(peer)),
    }
}

fn failure_of(step: &StretchStep) -> Option<&str> {
    let StretchStep::Step(Step::Did {
        failed: Some(error),
        ..
    }) = step
    else {
        return None;
    };
    Some(error)
}

fn failure_count(steps: &[StretchStep]) -> usize {
    let failures = steps.iter().filter(|step| failure_of(step).is_some());
    failures.count()
}

// A failed search keeps a line of its own, so its error is not folded into "Searched 3 times".
fn is_search(step: &StretchStep) -> bool {
    let StretchStep::Step(Step::Did {
        tool, failed: None, ..
    }) = step
    else {
        return false;
    };
    tool == "Grep" || tool == "Glob"
}

pub(super) fn step_line(step: &StretchStep) -> StepLine {
    let failed = failure_of(step).map(str::to_string);
    StepLine {
        text: step_text(step),
        failed,
    }
}

/// One line per step, with a run of searches told as one.
pub(super) fn step_lines(steps: &[StretchStep]) -> Vec<StepLine> {
    let mut lines = Vec::new();
    let mut searches = Vec::new();
    for step in steps {
        if is_search(step) {
            searches.push(step);
            continue;
        }
        tell_searches(&mut searches, &mut lines);
        lines.push(step_line(step));
    }
    tell_searches(&mut searches, &mut lines);
    lines
}

fn tell_searches(searches: &mut Vec<&StretchStep>, lines: &mut Vec<StepLine>) {
    match searches.as_slice() {
        [] => {}
        [only] => lines.push(step_line(only)),
        several => {
            let text = format!("Searched {} times", several.len());
            lines.push(StepLine { text, failed: None });
        }
    }
    searches.clear();
}

pub(super) fn fold_label(steps: &Stretch, is_open: bool) -> String {
    let count = steps.steps.len();
    if is_open && count == 1 {
        return "Hide the step".to_string();
    }
    if is_open {
        return "Hide the steps".to_string();
    }
    if count == 0 {
        return "Show the earlier steps".to_string();
    }
    let mut label = format!("Show the {count} steps");
    if steps.earlier_not_loaded {
        label = format!("Show the {count}+ steps");
    } else if count == 1 {
        label = "Show the step".to_string();
    }
    let failures = failure_count(&steps.steps);
    if failures > 0 {
        label = format!("{label}, {failures} failed");
    }
    label
}

#[cfg(test)]
mod tests {
    use taco_protocol::Peer;

    use super::*;

    fn did(tool: &str, summary: &str) -> StretchStep {
        let tool = tool.to_string();
        let summary = summary.to_string();
        StretchStep::Step(Step::Did {
            tool,
            summary,
            failed: None,
        })
    }

    fn changed(tool: &str, path: &str) -> StretchStep {
        let tool = tool.to_string();
        let path = path.to_string();
        StretchStep::Step(Step::Changed { tool, path })
    }

    fn failed(tool: &str, summary: &str, error: &str) -> StretchStep {
        let tool = tool.to_string();
        let summary = summary.to_string();
        let failed = Some(error.to_string());
        StretchStep::Step(Step::Did {
            tool,
            summary,
            failed,
        })
    }

    #[test]
    fn steps_tell_a_run_of_searches_as_one_and_folds_count_what_is_held() {
        let code_83 = Peer {
            name: Some("code-83".to_string()),
            address: None,
        };
        let steps = vec![
            did("Grep", "SIGHUP"),
            did("Glob", "*.rs"),
            did("Read", "pty.rs"),
            failed("Bash", "cargo test", "Exit code 101: 2 tests failed"),
            changed("Write", "PLAN.md"),
            changed("Edit", "pty.rs"),
            StretchStep::SentMessage(code_83),
            did("Glob", "tests/*.rs"),
            failed("Grep", "kill(", "regex parse error"),
            did("Grep", "fn shutdown"),
        ];
        let expected = [
            ("Searched 2 times", None),
            ("Read pty.rs", None),
            ("Ran cargo test", Some("Exit code 101: 2 tests failed")),
            ("Wrote PLAN.md", None),
            ("Edited pty.rs", None),
            ("Sent a message to code-83", None),
            ("Searched for tests/*.rs", None),
            ("Searched for kill(", Some("regex parse error")),
            ("Searched for fn shutdown", None),
        ];
        let lines = step_lines(&steps);
        let told: Vec<(&str, Option<&str>)> = lines
            .iter()
            .map(|line| (line.text.as_str(), line.failed.as_deref()))
            .collect();
        assert_eq!(told, expected);

        let stretch = |count: usize, earlier_not_loaded: bool| Stretch {
            number: 1,
            steps: steps[..count].to_vec(),
            earlier_not_loaded,
        };
        assert_eq!(fold_label(&stretch(1, false), false), "Show the step");
        assert_eq!(fold_label(&stretch(1, false), true), "Hide the step");
        assert_eq!(
            fold_label(&stretch(4, false), false),
            "Show the 4 steps, 1 failed"
        );
        assert_eq!(
            fold_label(&stretch(7, true), false),
            "Show the 7+ steps, 1 failed"
        );
        assert_eq!(
            fold_label(&stretch(0, true), false),
            "Show the earlier steps"
        );
    }
}
