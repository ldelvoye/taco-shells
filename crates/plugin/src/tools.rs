use serde_json::{Value, json};

pub const TOOL_NAMES: [&str; 3] = ["note", "now", "share"];

fn text_tool(name: &str, description: &str) -> Value {
    let schema = json!({
        "type": "object",
        "properties": {"text": {"type": "string"}},
        "required": ["text"],
    });
    json!({"name": name, "description": description, "inputSchema": schema})
}

fn share_tool() -> Value {
    let description = "Put something in the user's Shared list: a document, an image, a pull request or an artifact you made yourself, or an artifact you read and judge important. Never sources or reference links.";
    let schema = json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "enum": ["document", "image", "pull_request", "artifact", "other"],
            },
            "title": {"type": "string", "description": "A few words."},
            "target": {"type": "string", "description": "A path or a URL."},
            "body": {
                "type": "string",
                "description": "The text itself, when there is no file or URL.",
            },
        },
        "required": ["kind", "title"],
    });
    json!({"name": "share", "description": description, "inputSchema": schema})
}

pub fn tools() -> Vec<Value> {
    let note = text_tool(
        "note",
        "Add an entry to the user's activity log for this terminal: one or two plain sentences on what you just did, decided or found, written for the user.",
    );
    let now = text_tool(
        "now",
        "Set the one line the user sees for what you are on right now. Replaces the previous line. Under ten words.",
    );
    let share = share_tool();
    vec![note, now, share]
}
