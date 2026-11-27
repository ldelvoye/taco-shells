use std::io::{Read, Write};
use std::process::ExitCode;

use taco_plugin::{
    PostError, Surroundings, allow_answer, ancestor_pids, port_file, post, posts_at_all, posts_for,
    read_port, serve, serves,
};
use taco_protocol::PORT_FILE_VARIABLE;

const CONFIG_DIRECTORY_VARIABLE: &str = "TACO_SHELLS_CONFIG_DIR";

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

    let answer = allow_answer(&hook_input);
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
    if !posts_at_all(&surroundings) {
        return Ok(());
    }

    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw_payload) else {
        return Ok(());
    };

    let port_file_variable = std::env::var_os(PORT_FILE_VARIABLE);
    let config_dir_variable = std::env::var_os(CONFIG_DIRECTORY_VARIABLE);
    let home = std::env::var_os("HOME");
    let Some(path) = port_file(port_file_variable, config_dir_variable, home) else {
        return Ok(());
    };
    let Some(port) = read_port(&path) else {
        return Ok(());
    };

    // Six ancestors reaches the pane's shell through the usual agent and wrapper layers; too few and the pane is never placed.
    let pids = ancestor_pids(std::os::unix::process::parent_id(), 6);
    let posts = posts_for(&payload, &raw_payload, &pids);
    for next in &posts {
        let sent = post(port, next);
        if sent == Err(PostError::Refused) {
            break;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
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
