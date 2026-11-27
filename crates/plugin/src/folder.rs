use std::path::PathBuf;

use serde_json::{Value, json};

use crate::identity::{PLUGIN_NAME, SERVER_NAME, tool_name_prefix};

const DESCRIPTION: &str = "Agent Activity for Taco Shells";

fn file(path: String, content: Value) -> (PathBuf, String) {
    (PathBuf::from(path), format!("{content:#}\n"))
}

fn hook_command_group(is_async: bool) -> Value {
    let mut hook = json!({
        "type": "command",
        "command": "\"${CLAUDE_PLUGIN_ROOT}/libexec/taco-shells-plugin\" hook",
    });
    if is_async {
        hook["async"] = Value::Bool(true);
    }
    json!({"hooks": [hook]})
}

pub fn plugin_files(version: &str) -> Vec<(PathBuf, String)> {
    let manifest = json!({
        "name": PLUGIN_NAME,
        "version": version,
        "description": DESCRIPTION,
    });
    let mcp = json!({
        "mcpServers": {
            SERVER_NAME: {
                "command": "${CLAUDE_PLUGIN_ROOT}/libexec/taco-shells-plugin",
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
            "command": "\"${CLAUDE_PLUGIN_ROOT}/libexec/taco-shells-plugin\" allow",
        }],
    });
    // Sync where the app must hear the event before the agent goes on; async elsewhere so a hook never slows it.
    let hooks = json!({
        "hooks": {
            // The allow group first: it must answer before Claude Code asks the user.
            "PreToolUse": [allow_group, hook_command_group(true)],
            "SessionStart": [hook_command_group(false)],
            "SessionEnd": [hook_command_group(false)],
            "UserPromptSubmit": [hook_command_group(true)],
            "PostToolUse": [hook_command_group(false)],
            "PostToolUseFailure": [hook_command_group(false)],
            "StopFailure": [hook_command_group(true)],
            "Notification": [hook_command_group(false)],
            "Stop": [hook_command_group(false)],
        }
    });

    let manifest_path = String::from(".claude-plugin/plugin.json");
    let mcp_path = String::from(".mcp.json");
    let hooks_path = String::from("hooks/hooks.json");
    vec![
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

        let paths: Vec<&str> = files
            .iter()
            .map(|(path, _)| path.to_str().unwrap())
            .collect();
        let expected_paths = [
            ".claude-plugin/plugin.json",
            ".mcp.json",
            "hooks/hooks.json",
        ];
        assert_eq!(paths, expected_paths);

        let mcp = parsed(&files, ".mcp.json");
        assert_eq!(mcp["mcpServers"][SERVER_NAME]["alwaysLoad"], true);
        assert_eq!(mcp["mcpServers"][SERVER_NAME]["args"], json!(["mcp"]));
        let mcp_command = mcp["mcpServers"][SERVER_NAME]["command"].as_str().unwrap();
        assert!(mcp_command.contains("/libexec/"));

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
        assert!(!allow_command.contains("/bin/"));

        let expected = [
            ("SessionStart", false),
            ("SessionEnd", false),
            ("UserPromptSubmit", true),
            ("PreToolUse", true),
            ("PostToolUse", false),
            ("PostToolUseFailure", false),
            ("StopFailure", true),
            ("Notification", false),
            ("Stop", false),
        ];
        for (event, is_async) in expected {
            let groups = hooks["hooks"][event].as_array().unwrap();
            let last_group = groups.last().unwrap();
            let hook = &last_group["hooks"][0];
            let async_flag = hook.get("async").map(|flag| flag == &json!(true));
            if is_async {
                assert_eq!(async_flag, Some(true), "{event}");
            } else {
                assert_eq!(async_flag, None, "{event}");
            }
        }
    }
}
