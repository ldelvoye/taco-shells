use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use taco_plugin::{
    ANCESTOR_LIMIT, Post, Question, Surroundings, ancestor_pids, ask, permission_answer, post,
    posts_for, question_for, read_port, read_token, send_posts, serve, serves,
    session_start_output, stop_output,
};
use taco_protocol::routes::CONTEXT;
use taco_protocol::{ContextAnswer, PORT_FILE_VARIABLE, StopAnswer, deadlines};

fn run_mcp() -> std::io::Result<()> {
    let surroundings = Surroundings::from_environment();
    let serving = serves(&surroundings);
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(stdin.lock(), stdout.lock(), serving)
}

fn run_allow() -> std::io::Result<()> {
    let mut hook_input = String::new();
    std::io::stdin().read_to_string(&mut hook_input)?;

    let answer = permission_answer(&hook_input);
    if let Some(answer) = answer {
        let mut stdout = std::io::stdout();
        writeln!(stdout, "{answer}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn send_all(posts: &[Post], port: u16, token: Option<&str>) {
    let deadline = Instant::now() + deadlines::POSTS;
    send_posts(posts, |request| post(port, request, token, deadline));
}

fn ask_question(port: u16, question: &Question, token: Option<&str>) -> Option<serde_json::Value> {
    let deadline = Instant::now() + deadlines::QUESTION;
    ask(port, question.route, &question.body, token, deadline)
}

fn context_answer(reply: Option<serde_json::Value>) -> ContextAnswer {
    let Some(reply) = reply else {
        return ContextAnswer::default();
    };
    let parsed = serde_json::from_value::<ContextAnswer>(reply);
    let Ok(answer) = parsed else {
        return ContextAnswer::default();
    };
    answer
}

fn stop_answer(reply: Option<serde_json::Value>) -> StopAnswer {
    let Some(reply) = reply else {
        return StopAnswer::default();
    };
    let parsed = serde_json::from_value::<StopAnswer>(reply);
    let Ok(answer) = parsed else {
        return StopAnswer::default();
    };
    answer
}

fn print_output(output: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    // Claude Code reads stdout as the decision, so nothing else may ever be printed.
    writeln!(stdout, "{output}")?;
    stdout.flush()
}

fn run_hook() -> std::io::Result<()> {
    let mut raw_payload = String::new();
    std::io::stdin().read_to_string(&mut raw_payload)?;

    let surroundings = Surroundings::from_environment();
    if !serves(&surroundings) {
        return Ok(());
    }

    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw_payload) else {
        return Ok(());
    };

    let Some(port_file) = std::env::var_os(PORT_FILE_VARIABLE) else {
        return Ok(());
    };
    let path = PathBuf::from(port_file);
    let Some(port) = read_port(&path) else {
        return Ok(());
    };

    let pids = ancestor_pids(std::os::unix::process::parent_id(), ANCESTOR_LIMIT);
    let token = read_token(&path);
    let posts = posts_for(&payload, &pids);
    let token_text = token.as_deref();

    let Some(question) = question_for(&payload, &pids) else {
        send_all(&posts, port, token_text);
        return Ok(());
    };
    if question.route == CONTEXT {
        send_all(&posts, port, token_text);
        let reply = ask_question(port, &question, token_text);
        let answer = context_answer(reply);
        if let Some(output) = session_start_output(&answer) {
            print_output(&output)?;
        }
        return Ok(());
    }

    let reply = ask_question(port, &question, token_text);
    let answer = stop_answer(reply);
    if let Some(output) = stop_output(&answer) {
        // A nudged stop did not happen, so none of its posts are sent and the row stays working.
        print_output(&output)?;
        return Ok(());
    }
    send_all(&posts, port, token_text);
    Ok(())
}

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
    // A hook never fails the agent: every outcome exits 0, and its posts and question give up after their deadlines.
    if mode.as_deref() == Some("hook") {
        let _ = run_hook();
        return ExitCode::SUCCESS;
    }

    let outcome = match mode.as_deref() {
        Some("mcp") => run_mcp(),
        Some("allow") => run_allow(),
        _ => {
            eprintln!("usage: taco-shells-plugin mcp|allow|hook");
            return ExitCode::from(2);
        }
    };

    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("taco-shells-plugin: {problem}");
            ExitCode::FAILURE
        }
    }
}
