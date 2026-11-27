use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use taco_plugin::{
    ANCESTOR_LIMIT, Surroundings, ancestor_pids, permission_answer, post, posts_for, read_port,
    read_token, send_posts, serve, serves,
};
use taco_protocol::PORT_FILE_VARIABLE;

const HOOK_DEADLINE: Duration = Duration::from_millis(250);

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
    let deadline = Instant::now() + HOOK_DEADLINE;
    send_posts(&posts, |request| post(port, request, token_text, deadline));
    Ok(())
}

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
    // A hook never fails the agent: every outcome exits 0, and its posts give up after the hook's deadline.
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
