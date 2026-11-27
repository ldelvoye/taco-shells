use std::io::{Read, Write};
use std::process::ExitCode;

use taco_plugin::{Surroundings, allow_answer, serve, serves};

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

fn main() -> ExitCode {
    let mode = std::env::args().nth(1);
    let outcome = match mode.as_deref() {
        Some("mcp") => run_mcp(),
        Some("allow") => run_allow(),
        _ => {
            eprintln!("usage: taco-shells-plugin mcp|allow");
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
