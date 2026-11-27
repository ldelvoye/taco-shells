use serde_json::{Value, json};

use taco_agents::activity_tool_of;

const SUBAGENT_REASON: &str =
    "The activity panel is the main agent's; a subagent reports back to it instead.";

pub fn permission_answer(hook_input: &str) -> Option<String> {
    let parsed = serde_json::from_str::<Value>(hook_input).ok()?;
    let tool_value = parsed.get("tool_name")?;
    let tool_name = tool_value.as_str()?;

    let is_approved = activity_tool_of(tool_name).is_some();
    if !is_approved {
        return None;
    }

    let agent_id = parsed.get("agent_id").and_then(Value::as_str);
    let is_subagent = agent_id.is_some_and(|id| !id.is_empty());
    if is_subagent {
        let deny = json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": SUBAGENT_REASON,
            }
        });
        return Some(deny.to_string());
    }

    let allow = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "permissionDecisionReason": "Taco Shells activity tool",
        }
    });
    Some(allow.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::PLUGIN_NAME;

    fn input_for(tool_name: &str) -> String {
        json!({"tool_name": tool_name}).to_string()
    }

    #[test]
    fn answers_only_for_this_channels_activity_tools() {
        let other_name = match PLUGIN_NAME {
            "taco-shells" => "taco-shells-dev",
            _ => "taco-shells",
        };
        let own_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__note");
        let other_tool = format!("mcp__plugin_{other_name}_activity__note");

        let allowed = permission_answer(&input_for(&own_tool)).unwrap();
        let decision: Value = serde_json::from_str(&allowed).unwrap();
        assert_eq!(
            decision["hookSpecificOutput"]["permissionDecision"],
            "allow"
        );
        let unlisted_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__open_terminal");
        assert_eq!(permission_answer(&input_for(&unlisted_tool)), None);
        assert_eq!(permission_answer(&input_for("Bash")), None);
        assert_eq!(permission_answer(&input_for(&other_tool)), None);
        assert_eq!(permission_answer("not json"), None);
    }

    #[test]
    fn a_subagent_is_denied() {
        let own_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__share");
        let from_subagent = json!({"tool_name": own_tool, "agent_id": "a1"}).to_string();

        let answer = permission_answer(&from_subagent).unwrap();
        let decision: Value = serde_json::from_str(&answer).unwrap();
        let expected = json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": SUBAGENT_REASON,
            }
        });
        assert_eq!(decision, expected);

        let from_other_tool = json!({"tool_name": "Bash", "agent_id": "a1"}).to_string();
        assert_eq!(permission_answer(&from_other_tool), None);
    }

    #[test]
    fn a_null_or_empty_agent_id_is_the_main_agent() {
        let own_tool = format!("mcp__plugin_{PLUGIN_NAME}_activity__share");
        for agent_id in [json!(null), json!("")] {
            let input = json!({"tool_name": own_tool, "agent_id": agent_id}).to_string();
            let answer = permission_answer(&input).unwrap();
            let decision: Value = serde_json::from_str(&answer).unwrap();
            assert_eq!(
                decision["hookSpecificOutput"]["permissionDecision"],
                "allow"
            );
        }
    }
}
