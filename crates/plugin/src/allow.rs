use serde_json::{Value, json};

use crate::identity::PLUGIN_NAME;
use crate::tools::TOOL_NAMES;

pub fn allow_answer(hook_input: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(hook_input).ok()?;
    let tool_name = parsed.get("tool_name")?.as_str()?;

    let is_approved = TOOL_NAMES.iter().any(|name| {
        let approved = format!("mcp__plugin_{PLUGIN_NAME}_activity__{name}");
        approved == tool_name
    });
    if !is_approved {
        return None;
    }

    let answer = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "Taco Shells activity tool",
        }
    });
    Some(answer.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input_for(tool_name: &str) -> String {
        json!({"tool_name": tool_name}).to_string()
    }

    #[test]
    fn allows_only_this_channels_activity_tools() {
        let other_name = if PLUGIN_NAME == "taco-shells" {
            "taco-shells-dev"
        } else {
            "taco-shells"
        };
        let own_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__note");
        let other_tool = format!("mcp__plugin_{other_name}_activity__note");

        let allowed = allow_answer(&input_for(&own_tool)).unwrap();
        let decision: Value = serde_json::from_str(&allowed).unwrap();
        assert_eq!(
            decision["hookSpecificOutput"]["permissionDecision"],
            "allow"
        );
        let unlisted_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__open_terminal");
        assert_eq!(allow_answer(&input_for(&unlisted_tool)), None);
        assert_eq!(allow_answer(&input_for("Bash")), None);
        assert_eq!(allow_answer(&input_for(&other_tool)), None);
        assert_eq!(allow_answer("not json"), None);
    }
}
