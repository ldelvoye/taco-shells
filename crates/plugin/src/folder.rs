use std::path::PathBuf;

use serde_json::{Value, json};

use crate::identity::{PLUGIN_NAME, SERVER_NAME, tool_name_prefix};

const DESCRIPTION: &str = "Agent Activity for Taco Shells";

fn file(path: String, content: Value) -> (PathBuf, String) {
    (PathBuf::from(path), format!("{content:#}\n"))
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
    let hooks = json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": matcher,
                "hooks": [{
                    "type": "command",
                    "command": "\"${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin\" allow",
                }],
            }]
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
    }
}
