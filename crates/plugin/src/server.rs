use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::tools::{NOTE, NOW, SHARE, SHARE_KINDS, TOOL_NAMES, tools};

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const UNKNOWN_METHOD: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

struct Failure {
    code: i64,
    message: String,
}

pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    serving: bool,
) -> std::io::Result<()> {
    let mut line = Vec::new();
    loop {
        line.clear();
        let bytes_read = input.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            break;
        }

        let reply = reply_to_bytes(&line, serving);
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn parse_error_reply() -> Value {
    let failure = Failure {
        code: PARSE_ERROR,
        message: String::from("parse error"),
    };
    error_reply(Value::Null, failure)
}

fn reply_to_bytes(line: &[u8], serving: bool) -> Option<Value> {
    let decoded = std::str::from_utf8(line);
    let text = match decoded {
        Ok(text) => text,
        Err(_) => return Some(parse_error_reply()),
    };
    if text.trim().is_empty() {
        return None;
    }
    reply_to(text, serving)
}

fn reply_to(line: &str, serving: bool) -> Option<Value> {
    let parsed = serde_json::from_str::<Value>(line);
    let message = match parsed {
        Ok(message) => message,
        Err(_) => return Some(parse_error_reply()),
    };

    let id = message.get("id")?.clone();
    let method_value = message.get("method")?;
    let Some(method) = method_value.as_str() else {
        let failure = Failure {
            code: INVALID_REQUEST,
            message: String::from("invalid request"),
        };
        return Some(error_reply(id, failure));
    };
    let params = match message.get("params") {
        Some(params) => params.clone(),
        None => Value::Null,
    };

    let outcome = answer(method, &params, serving);
    match outcome {
        Ok(result) => Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
        Err(failure) => Some(error_reply(id, failure)),
    }
}

fn error_reply(id: Value, failure: Failure) -> Value {
    let error = json!({"code": failure.code, "message": failure.message});
    json!({"jsonrpc": "2.0", "id": id, "error": error})
}

fn answer(method: &str, params: &Value, serving: bool) -> Result<Value, Failure> {
    match method {
        "initialize" => Ok(initialize_result(params)),
        "tools/list" => Ok(tools_list_result(serving)),
        "tools/call" => call_result(params, serving),
        "ping" => Ok(json!({})),
        _ => Err(Failure {
            code: UNKNOWN_METHOD,
            message: format!("unknown method {method}"),
        }),
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested_value = params.get("protocolVersion");
    let requested = requested_value.and_then(Value::as_str);
    let version = match requested {
        Some(version) if SUPPORTED_PROTOCOL_VERSIONS.contains(&version) => version,
        _ => DEFAULT_PROTOCOL_VERSION,
    };
    let server_info = json!({
        "name": "taco-shells-activity",
        "version": env!("CARGO_PKG_VERSION"),
    });
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": server_info,
    })
}

fn tools_list_result(serving: bool) -> Value {
    // Every session loads the plugin, so outside its own app's terminals it lists nothing.
    if !serving {
        return json!({"tools": []});
    }
    let listed = tools();
    json!({"tools": listed})
}

fn is_non_empty_string(value: Option<&Value>) -> bool {
    match value.and_then(Value::as_str) {
        Some(text) => !text.is_empty(),
        None => false,
    }
}

fn share_problem(arguments: Option<&Value>) -> Option<String> {
    let kind = arguments.and_then(|arguments| arguments.get("kind"));
    let kind_text = kind.and_then(Value::as_str);
    let is_kind_valid = match kind_text {
        Some(kind_text) => SHARE_KINDS.contains(&kind_text),
        None => false,
    };
    if !is_kind_valid {
        let kinds = SHARE_KINDS.join(", ");
        return Some(format!("share needs a kind: {kinds}"));
    }

    let title = arguments.and_then(|arguments| arguments.get("title"));
    if !is_non_empty_string(title) {
        return Some(String::from("share needs a title"));
    }

    let target = arguments.and_then(|arguments| arguments.get("target"));
    let body = arguments.and_then(|arguments| arguments.get("body"));
    for (field, value) in [("target", target), ("body", body)] {
        if let Some(value) = value
            && !value.is_string()
        {
            return Some(format!("share {field} must be a string"));
        }
    }

    let has_target = is_non_empty_string(target);
    let has_body = is_non_empty_string(body);
    if !has_target && !has_body {
        return Some(String::from("share needs a target or a body"));
    }
    None
}

fn argument_problem(name: &str, arguments: Option<&Value>) -> Option<String> {
    match name {
        NOTE | NOW => {
            let text = arguments.and_then(|arguments| arguments.get("text"));
            if is_non_empty_string(text) {
                return None;
            }
            Some(format!("{name} needs a text"))
        }
        SHARE => share_problem(arguments),
        _ => None,
    }
}

fn call_result(params: &Value, serving: bool) -> Result<Value, Failure> {
    let requested_value = params.get("name");
    let requested = requested_value.and_then(Value::as_str);
    let Some(name) = requested else {
        return Err(Failure {
            code: INVALID_PARAMS,
            message: String::from("missing tool name"),
        });
    };

    let is_known = TOOL_NAMES.contains(&name);
    if !(serving && is_known) {
        return Err(Failure {
            code: INVALID_PARAMS,
            message: format!("unknown tool {name}"),
        });
    }

    let problem = argument_problem(name, params.get("arguments"));
    if let Some(problem) = problem {
        return Ok(json!({
            "content": [{"type": "text", "text": problem}],
            "isError": true,
        }));
    }

    // The call reaches the app through the PostToolUse hook, so the server only answers.
    Ok(json!({"content": [{"type": "text", "text": "Noted."}]}))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: &[u8] = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        "this is not json\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"note","arguments":{"text":"x"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"remember"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":5,"method":"initialize","params":{"protocolVersion":"9999-01-01"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":6,"result":{}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"note","arguments":{}}}"#,
        "\n",
    )
    .as_bytes();

    fn run(input: &[u8], serving: bool) -> Vec<Value> {
        let mut output = Vec::new();
        serve(input, &mut output, serving).unwrap();
        let text = String::from_utf8(output).unwrap();
        let mut replies = Vec::new();
        for line in text.lines() {
            let reply = serde_json::from_str(line).unwrap();
            replies.push(reply);
        }
        replies
    }

    #[test]
    fn answers_a_session_when_serving() {
        let replies = run(INPUT, true);

        assert_eq!(replies.len(), 7);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(replies[1]["error"]["code"], -32700);
        assert_eq!(replies[1]["id"], Value::Null);
        let listed = replies[2]["result"]["tools"].as_array().unwrap();
        let mut names: Vec<&str> = Vec::new();
        for tool in listed {
            let name = tool["name"].as_str().unwrap();
            names.push(name);
        }
        assert_eq!(names, TOOL_NAMES);
        assert_eq!(replies[3]["result"]["content"][0]["text"], "Noted.");
        assert_eq!(replies[4]["error"]["code"], -32602);
        assert_eq!(replies[5]["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(replies[6]["id"], 7);
        assert_eq!(replies[6]["result"]["isError"], true);
    }

    #[test]
    fn a_share_needs_something_to_open() {
        let title_only = json!({"kind": "document", "title": "Plan"});
        let with_target = json!({"kind": "document", "title": "Plan", "target": "PLAN.md"});
        let with_body = json!({"kind": "other", "title": "Summary", "body": "Done."});

        let refused = share_problem(Some(&title_only));
        assert_eq!(refused.as_deref(), Some("share needs a target or a body"));
        assert_eq!(share_problem(Some(&with_target)), None);
        assert_eq!(share_problem(Some(&with_body)), None);
    }

    #[test]
    fn survives_a_line_that_is_not_utf8() {
        let input = [b"\xff\xfe\n".as_slice(), INPUT].concat();
        let replies = run(&input, true);

        assert_eq!(replies[0]["error"]["code"], -32700);
        assert_eq!(replies[0]["id"], Value::Null);
        assert_eq!(replies[1]["id"], 1);
    }

    #[test]
    fn lists_nothing_and_refuses_calls_when_not_serving() {
        let replies = run(INPUT, false);

        assert_eq!(replies.len(), 7);
        assert_eq!(replies[2]["result"], json!({"tools": []}));
        assert_eq!(replies[3]["error"]["code"], -32602);
    }
}
