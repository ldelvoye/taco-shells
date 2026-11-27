use std::path::PathBuf;

use serde_json::{Value, json};

use crate::identity::{PLUGIN_NAME, SERVER_NAME, tool_name_prefix};

const DESCRIPTION: &str = "Agent Activity for Taco Shells";

fn file(path: String, content: Value) -> (PathBuf, String) {
    (PathBuf::from(path), format!("{content:#}\n"))
}

fn event_group(is_async: bool) -> Value {
    let mut hook = json!({
        "type": "command",
        "command": "\"${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin\" hook",
    });
    if is_async {
        hook["async"] = Value::Bool(true);
    }
    json!({"hooks": [hook]})
}

pub fn plugin_files(stamp: &str) -> Vec<(PathBuf, String)> {
    let marketplace = json!({
        "name": PLUGIN_NAME,
        "owner": {"name": "Taco Shells"},
        "plugins": [{
            "name": PLUGIN_NAME,
            "source": format!("./{PLUGIN_NAME}"),
            "description": DESCRIPTION,
        }],
    });
    let manifest = json!({
        "name": PLUGIN_NAME,
        "version": stamp,
        "description": DESCRIPTION,
    });
    let mcp = json!({
        "mcpServers": {
            SERVER_NAME: {
                "command": "${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin",
                "args": ["mcp"],
                // Without it the tools wait behind tool search and an agent rarely loads them.
                "alwaysLoad": true,
            }
        }
    });
    let matcher = format!("{}.*", tool_name_prefix());
    let allow_group = json!({
        "matcher": matcher,
        "hooks": [{
            "type": "command",
            "command": "\"${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin\" allow",
        }],
    });
    let hooks = json!({
        "hooks": {
            "PreToolUse": [allow_group, event_group(true)],
            "SessionStart": [event_group(false)],
            "SessionEnd": [event_group(false)],
            "UserPromptSubmit": [event_group(true)],
            "PostToolUse": [event_group(true)],
            "StopFailure": [event_group(true)],
            "Notification": [event_group(false)],
            "Stop": [event_group(false)],
        }
    });

    let marketplace_path = String::from(".claude-plugin/marketplace.json");
    let manifest_path = format!("{PLUGIN_NAME}/.claude-plugin/plugin.json");
    let mcp_path = format!("{PLUGIN_NAME}/.mcp.json");
    let hooks_path = format!("{PLUGIN_NAME}/hooks/hooks.json");
    vec![
        file(marketplace_path, marketplace),
        file(manifest_path, manifest),
        file(mcp_path, mcp),
        file(hooks_path, hooks),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::TOOL_NAMES;

    fn parsed(files: &[(PathBuf, String)], suffix: &str) -> Value {
        let found = files.iter().find(|(path, _)| path.ends_with(suffix));
        let (_, content) = found.unwrap();
        serde_json::from_str(content).unwrap()
    }

    #[test]
    fn describes_the_plugin_folder() {
        let files = plugin_files("2026.11.26");

        let mcp = parsed(&files, ".mcp.json");
        assert_eq!(mcp["mcpServers"][SERVER_NAME]["alwaysLoad"], true);
        assert_eq!(mcp["mcpServers"][SERVER_NAME]["args"], json!(["mcp"]));

        let hooks = parsed(&files, "hooks.json");
        let matcher = hooks["hooks"]["PreToolUse"][0]["matcher"].as_str().unwrap();
        let matcher_prefix = matcher.strip_suffix(".*").unwrap();
        for name in TOOL_NAMES {
            let approved = format!("{}{name}", tool_name_prefix());
            assert!(approved.starts_with(matcher_prefix));
        }

        let pre_tool_groups = hooks["hooks"]["PreToolUse"].as_array().unwrap();
        let allow_command = pre_tool_groups[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(allow_command.ends_with("allow"));
        assert_eq!(pre_tool_groups.len(), 2);

        let expected = [
            ("SessionStart", false),
            ("SessionEnd", false),
            ("UserPromptSubmit", true),
            ("PreToolUse", true),
            ("PostToolUse", true),
            ("StopFailure", true),
            ("Notification", false),
            ("Stop", false),
        ];
        for (event, is_async) in expected {
            let groups = hooks["hooks"][event].as_array().unwrap();
            let last_group = groups.last().unwrap();
            let hook = &last_group["hooks"][0];
            assert!(hook["command"].as_str().unwrap().ends_with("\" hook"));
            let async_flag = hook.get("async").map(|flag| flag == &json!(true));
            if is_async {
                assert_eq!(async_flag, Some(true), "{event}");
            } else {
                assert_eq!(async_flag, None, "{event}");
            }
        }
    }
}
