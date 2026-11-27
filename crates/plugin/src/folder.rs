use std::path::PathBuf;

use serde_json::{Value, json};

use crate::identity::PLUGIN_NAME;

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
            "activity": {
                "command": "${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin",
                "args": ["mcp"],
                "alwaysLoad": true,
            }
        }
    });
    let hooks = json!({
        "hooks": {
            "PreToolUse": [{
                "matcher": format!("mcp__plugin_{PLUGIN_NAME}_activity__.*"),
                "hooks": [{
                    "type": "command",
                    "command": "\"${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin\" allow",
                }],
            }]
        }
    });

    vec![
        file(String::from(".claude-plugin/marketplace.json"), marketplace),
        file(
            format!("{PLUGIN_NAME}/.claude-plugin/plugin.json"),
            manifest,
        ),
        file(format!("{PLUGIN_NAME}/.mcp.json"), mcp),
        file(format!("{PLUGIN_NAME}/hooks/hooks.json"), hooks),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(files: &[(PathBuf, String)], suffix: &str) -> Value {
        let found = files.iter().find(|(path, _)| path.ends_with(suffix));
        let (_, content) = found.unwrap();
        serde_json::from_str(content).unwrap()
    }

    #[test]
    fn describes_the_plugin_folder() {
        let files = plugin_files("2026.11.26");

        assert_eq!(files.len(), 4);
        assert_eq!(parsed(&files, "plugin.json")["version"], "2026.11.26");

        let mcp = parsed(&files, ".mcp.json");
        assert_eq!(mcp["mcpServers"]["activity"]["alwaysLoad"], true);
        assert_eq!(mcp["mcpServers"]["activity"]["args"], json!(["mcp"]));

        let hooks = parsed(&files, "hooks.json");
        let entry = &hooks["hooks"]["PreToolUse"][0];
        let expected_matcher = format!("mcp__plugin_{PLUGIN_NAME}_activity__.*");
        assert_eq!(entry["matcher"], expected_matcher);
        let command = entry["hooks"][0]["command"].as_str().unwrap();
        assert!(command.contains("\"${CLAUDE_PLUGIN_ROOT}/bin/taco-shells-plugin\" allow"));
    }
}
