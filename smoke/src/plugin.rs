use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use taco_protocol::APP_VARIABLE;

use crate::Smoke;

const PLUGIN_EXECUTABLE: &str = "taco-shells-plugin";
const DEV_NAME: &str = "taco-shells-dev";
const STABLE_NAME: &str = "taco-shells";
const ACTIVITY_TOOLS: [&str; 3] = ["note", "now", "share"];
const INITIALIZE_ID: u64 = 1;
const TOOLS_LIST_ID: u64 = 2;
const ANSWER_DEADLINE: Duration = Duration::from_secs(5);
const EXIT_POLL: Duration = Duration::from_millis(5);

pub fn lists_its_three_tools_inside_the_dev_app(smoke: &Smoke) -> Result<(), String> {
    let replies = mcp_replies(smoke, Some(DEV_NAME))?;
    let initialize = reply_with_id(&replies, INITIALIZE_ID)?;
    let version = initialize["result"]["protocolVersion"].as_str();
    if version.is_none() {
        return Err(format!("initialize answered {initialize}"));
    }

    let names = tool_names(&replies)?;
    let mut expected: Vec<String> = ACTIVITY_TOOLS.map(String::from).to_vec();
    expected.sort();
    if names != expected {
        return Err(format!("it lists {names:?}"));
    }
    Ok(())
}

pub fn lists_no_tools_outside_the_dev_app(smoke: &Smoke) -> Result<(), String> {
    for app in [None, Some(STABLE_NAME)] {
        let replies = mcp_replies(smoke, app)?;
        let names = tool_names(&replies)?;
        if !names.is_empty() {
            return Err(format!("with {APP_VARIABLE} as {app:?} it lists {names:?}"));
        }
    }
    Ok(())
}

pub fn approves_its_own_tool_and_no_other(smoke: &Smoke) -> Result<(), String> {
    let prefix = format!("mcp__plugin_{DEV_NAME}_activity__");
    let own_tool = format!("{prefix}note");
    let unlisted_tool = format!("{prefix}open_terminal");

    let own_answer = hook_answer(smoke, &own_tool)?;
    let parsed = serde_json::from_str::<Value>(&own_answer).map_err(|error| {
        format!("its answer for {own_tool} is not JSON ({error}): {own_answer}")
    })?;
    let decision = &parsed["hookSpecificOutput"]["permissionDecision"];
    if decision != "allow" {
        return Err(format!("its answer for {own_tool} is {own_answer}"));
    }

    let unlisted_answer = hook_answer(smoke, &unlisted_tool)?;
    if !unlisted_answer.trim().is_empty() {
        return Err(format!("it answered {unlisted_answer} for {unlisted_tool}"));
    }
    Ok(())
}

fn mcp_replies(smoke: &Smoke, app: Option<&str>) -> Result<Vec<Value>, String> {
    let initialize = json!({
        "jsonrpc": "2.0",
        "id": INITIALIZE_ID,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "taco-smoke", "version": "0"},
        },
    });
    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let tools_list = json!({"jsonrpc": "2.0", "id": TOOLS_LIST_ID, "method": "tools/list"});
    let input = format!("{initialize}\n{initialized}\n{tools_list}\n");

    let output = run(smoke, "mcp", app, &input)?;
    let mut replies = Vec::new();
    for line in output.lines() {
        let reply = serde_json::from_str::<Value>(line)
            .map_err(|error| format!("it wrote a line that is not JSON ({error}): {line}"))?;
        replies.push(reply);
    }
    Ok(replies)
}

fn reply_with_id(replies: &[Value], id: u64) -> Result<&Value, String> {
    let found = replies.iter().find(|reply| reply["id"] == id);
    match found {
        Some(reply) => Ok(reply),
        None => Err(format!("it sent no reply to request {id}")),
    }
}

fn tool_names(replies: &[Value]) -> Result<Vec<String>, String> {
    let reply = reply_with_id(replies, TOOLS_LIST_ID)?;
    let Some(tools) = reply["result"]["tools"].as_array() else {
        return Err(format!("tools/list answered {reply}"));
    };

    let mut names = Vec::new();
    for tool in tools {
        let Some(name) = tool["name"].as_str() else {
            return Err(format!("a listed tool has no name: {tool}"));
        };
        names.push(name.to_string());
    }
    names.sort();
    Ok(names)
}

fn hook_answer(smoke: &Smoke, tool_name: &str) -> Result<String, String> {
    let input = json!({"tool_name": tool_name});
    run(smoke, "allow", None, &input.to_string())
}

fn run(smoke: &Smoke, mode: &str, app: Option<&str>, input: &str) -> Result<String, String> {
    let program = smoke
        .app
        .join("Contents")
        .join("MacOS")
        .join(PLUGIN_EXECUTABLE);
    let mut command = Command::new(&program);
    command.arg(mode);
    // Smoke runs inside agents' sessions, whose Claude Code variables would otherwise change the answer.
    command.env_clear();
    if let Some(app) = app {
        command.env(APP_VARIABLE, app);
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start {}: {error}", program.display()))?;

    let Some(mut stdin) = child.stdin.take() else {
        return Err("its stdin was not piped".to_string());
    };
    let Some(mut stdout) = child.stdout.take() else {
        return Err("its stdout was not piped".to_string());
    };
    let written = stdin.write_all(input.as_bytes());
    // The program ends only when its stdin closes.
    drop(stdin);
    if let Err(error) = written {
        abandon(&mut child);
        return Err(format!("could not write to {mode}: {error}"));
    }

    let deadline = Instant::now() + ANSWER_DEADLINE;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut text = String::new();
        let read = stdout.read_to_string(&mut text);
        let _ = sender.send(read.map(|_| text));
    });
    let received = receiver.recv_timeout(ANSWER_DEADLINE);
    let text = match received {
        Ok(Ok(text)) => text,
        Ok(Err(error)) => {
            abandon(&mut child);
            return Err(format!("could not read what {mode} wrote: {error}"));
        }
        Err(_) => {
            abandon(&mut child);
            return Err(format!(
                "{mode} wrote nothing to the end within {} s",
                ANSWER_DEADLINE.as_secs()
            ));
        }
    };

    let status = loop {
        let exited = child
            .try_wait()
            .map_err(|error| format!("could not wait for {mode}: {error}"))?;
        if let Some(status) = exited {
            break status;
        }
        if Instant::now() >= deadline {
            abandon(&mut child);
            return Err(format!(
                "{mode} did not exit within {} s",
                ANSWER_DEADLINE.as_secs()
            ));
        }
        thread::sleep(EXIT_POLL);
    };
    if !status.success() {
        return Err(format!("{mode} ended with {status}"));
    }
    Ok(text)
}

fn abandon(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
