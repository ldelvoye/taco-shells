use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::tools::{TOOL_NAMES, tools};

const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const PARSE_ERROR: i64 = -32700;
const UNKNOWN_METHOD: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

struct Failure {
    code: i64,
    message: String,
}

pub fn serve(input: impl BufRead, mut output: impl Write, serving: bool) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let reply = reply_to(&line, serving);
        if let Some(reply) = reply {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

fn reply_to(line: &str, serving: bool) -> Option<Value> {
    let parsed = serde_json::from_str::<Value>(line);
    let message = match parsed {
        Ok(message) => message,
        Err(_) => {
            let failure = Failure {
                code: PARSE_ERROR,
                message: String::from("parse error"),
            };
            return Some(error_reply(Value::Null, failure));
        }
    };

    let id = message.get("id")?.clone();
    let requested_method = message.get("method").and_then(Value::as_str);
    let method = requested_method.unwrap_or_default();
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
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = match requested {
        Some(version) => version,
        None => DEFAULT_PROTOCOL_VERSION,
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
    if serving {
        json!({"tools": tools()})
    } else {
        json!({"tools": []})
    }
}

fn call_result(params: &Value, serving: bool) -> Result<Value, Failure> {
    let requested = params.get("name").and_then(Value::as_str);
    let name = requested.unwrap_or_default();

    let is_known = TOOL_NAMES.contains(&name);
    if serving && is_known {
        return Ok(json!({"content": [{"type": "text", "text": "Noted."}]}));
    }
    Err(Failure {
        code: INVALID_PARAMS,
        message: format!("unknown tool {name}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: &str = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        "this is not json\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"note","arguments":{"text":"x"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"delete"}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"remember"}}"#,
        "\n",
    );

    fn run(serving: bool) -> Vec<Value> {
        let mut output = Vec::new();
        serve(INPUT.as_bytes(), &mut output, serving).unwrap();
        let text = String::from_utf8(output).unwrap();
        text.lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn answers_a_session_when_serving() {
        let replies = run(true);

        assert_eq!(replies.len(), 6);
        assert_eq!(replies[0]["id"], 1);
        assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(replies[1]["error"]["code"], -32700);
        assert_eq!(replies[1]["id"], Value::Null);
        let listed = replies[2]["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = listed
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, TOOL_NAMES);
        assert_eq!(replies[3]["result"]["content"][0]["text"], "Noted.");
        assert_eq!(replies[4]["error"]["code"], -32602);
        assert_eq!(replies[5]["error"]["code"], -32602);
    }

    #[test]
    fn lists_nothing_and_refuses_calls_when_not_serving() {
        let replies = run(false);

        assert_eq!(replies.len(), 6);
        assert_eq!(replies[2]["result"], json!({"tools": []}));
        assert_eq!(replies[3]["error"]["code"], -32602);
    }
}
