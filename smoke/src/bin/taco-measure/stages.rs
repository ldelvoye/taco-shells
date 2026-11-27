use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use taco_protocol::TerminalId;
use taco_smoke::driver::Driver;
use taco_smoke::launch;

use crate::flags::Flags;

const READY_DEADLINE: Duration = Duration::from_secs(15);
const STEP_DEADLINE: Duration = Duration::from_secs(5);
const FILL_DEADLINE: Duration = Duration::from_secs(120);
const NO_PAUSE: Duration = Duration::ZERO;
const POLL_PAUSE: Duration = Duration::from_millis(200);
const TYPING_SPACING: Duration = Duration::from_millis(100);
const QUIET_PAUSE: Duration = Duration::from_millis(500);
const FLOOD_WARMUP: Duration = Duration::from_secs(1);
const NEW_TERMINAL: &str = "cmd-t";
const NEXT_ROW: &str = "cmd-shift-]";
const PREVIOUS_ROW: &str = "cmd-shift-[";
const INTERRUPT: &str = "\u{3}";
const FLOOD_WIDTH: usize = 80;
const FILL_LINES: u32 = 110_000;
const SWITCH_PAIRS: usize = 10;
const KILOBYTES_PER_MEGABYTE: f64 = 1024.0;

const READY: &str = "ready";
const MEMORY_ONE: &str = "memory, one terminal";
const OPEN: &str = "open terminals";
const TYPING_QUIET: &str = "typing, nothing printing";
const TYPING_FLOODED: &str = "typing while printing";
const FILL: &str = "fill scrollback";
const SWITCH_ALL: &str = "switch, every entry in turn";
const SWITCH_PAIR: &str = "switch between two entries";
const MEMORY_END: &str = "memory at the end";

pub struct MemoryPoint {
    pub label: String,
    pub app: f64,
    pub runtime: f64,
}

impl MemoryPoint {
    pub fn total(&self) -> f64 {
        self.app + self.runtime
    }
}

pub struct Measured {
    pub memory: Vec<MemoryPoint>,
    pub typing_quiet: Vec<f64>,
    pub typing_flooded: Vec<f64>,
    pub switch_all: Vec<f64>,
    pub switch_pair: Vec<f64>,
}

struct Rows {
    terminals: Vec<TerminalId>,
    selected: usize,
}

pub fn run(
    driver: &mut Driver,
    app_pid: u32,
    flags: &Flags,
    work: &Path,
) -> Result<Measured, String> {
    println!("stage: {READY}");
    wait_until_ready(driver).map_err(|error| failure(READY, error))?;

    println!("stage: {MEMORY_ONE}");
    let one = memory_point("one terminal", app_pid).map_err(|error| failure(MEMORY_ONE, error))?;

    println!("stage: {OPEN}");
    open_terminals(driver, flags.terminals).map_err(|error| failure(OPEN, error))?;
    let label = format!("{} terminals", flags.terminals);
    let opened = memory_point(&label, app_pid).map_err(|error| failure(OPEN, error))?;

    println!("stage: {TYPING_QUIET}");
    let typing_quiet =
        type_quietly(driver, flags.samples).map_err(|error| failure(TYPING_QUIET, error))?;

    println!("stage: {TYPING_FLOODED}");
    let typing_flooded =
        type_while_printing(driver, flags).map_err(|error| failure(TYPING_FLOODED, error))?;

    println!("stage: {FILL}");
    fill_scrollback(driver, flags.full, work).map_err(|error| failure(FILL, error))?;
    let label = format!("{} terminals with a full scrollback", flags.full);
    let filled = memory_point(&label, app_pid).map_err(|error| failure(FILL, error))?;

    println!("stage: {SWITCH_ALL}");
    let switch_all = switch_every_entry(driver).map_err(|error| failure(SWITCH_ALL, error))?;

    println!("stage: {SWITCH_PAIR}");
    let switch_pair = switch_between_two(driver).map_err(|error| failure(SWITCH_PAIR, error))?;

    println!("stage: {MEMORY_END}");
    let end = memory_point("at the end", app_pid).map_err(|error| failure(MEMORY_END, error))?;

    Ok(Measured {
        memory: vec![one, opened, filled, end],
        typing_quiet,
        typing_flooded,
        switch_all,
        switch_pair,
    })
}

fn failure(stage: &str, error: String) -> String {
    format!("stage \"{stage}\" failed: {error}")
}

fn wait_for(
    what: &str,
    deadline: Duration,
    pause: Duration,
    mut check: impl FnMut() -> Result<bool, String>,
) -> Result<(), String> {
    let start = Instant::now();
    loop {
        if check()? {
            return Ok(());
        }
        if start.elapsed() >= deadline {
            return Err(format!("timed out after {deadline:?} waiting for {what}"));
        }
        if !pause.is_zero() {
            thread::sleep(pause);
        }
    }
}

fn rows(driver: &mut Driver) -> Result<Rows, String> {
    let state = driver.drawn_state()?;
    let Some(arrangement) = state.arrangement else {
        return Ok(Rows {
            terminals: Vec::new(),
            selected: 0,
        });
    };
    let listed = arrangement.rows.iter();
    let terminals: Vec<TerminalId> = listed.map(|row| row.terminal.clone()).collect();
    Ok(Rows {
        terminals,
        selected: arrangement.selected,
    })
}

fn wait_until_ready(driver: &mut Driver) -> Result<(), String> {
    wait_for("a row in the arrangement", READY_DEADLINE, NO_PAUSE, || {
        let current = rows(driver)?;
        Ok(!current.terminals.is_empty())
    })
}

fn resident_megabytes(pid: u32) -> Result<f64, String> {
    let pid_text = pid.to_string();
    let mut ps = Command::new("ps");
    ps.args(["-o", "rss=", "-p", &pid_text]);
    let output = ps
        .output()
        .map_err(|error| format!("could not run ps for {pid}: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    let kilobytes: f64 = trimmed
        .parse()
        .map_err(|_| format!("ps gave no memory size for {pid}: {stdout:?}"))?;
    Ok(kilobytes / KILOBYTES_PER_MEGABYTE)
}

fn memory_point(label: &str, app_pid: u32) -> Result<MemoryPoint, String> {
    let Some(runtime_pid) = launch::runtime_pid(app_pid) else {
        return Err(format!("no runtime process under the app (pid {app_pid})"));
    };
    let app = resident_megabytes(app_pid)?;
    let runtime = resident_megabytes(runtime_pid)?;
    Ok(MemoryPoint {
        label: label.to_string(),
        app,
        runtime,
    })
}

fn open_terminals(driver: &mut Driver, wanted: usize) -> Result<(), String> {
    loop {
        let before = rows(driver)?.terminals.len();
        if before >= wanted {
            return Ok(());
        }
        driver.keys(&[NEW_TERMINAL])?;
        wait_for("a new row", STEP_DEADLINE, NO_PAUSE, || {
            let current = rows(driver)?;
            Ok(current.terminals.len() > before)
        })?;
    }
}

// Ends once the terminal's view answers: only eight are kept, and one brought back attaches after its snapshot
// arrives, so text typed before that would be lost.
fn select(driver: &mut Driver, target: usize) -> Result<(), String> {
    loop {
        let current = rows(driver)?;
        if current.selected == target {
            let terminal = current.terminals[target].clone();
            return wait_for("the terminal's view", STEP_DEADLINE, NO_PAUSE, || {
                let screen = driver.screen_text(&terminal)?;
                Ok(screen.is_some())
            });
        }
        let key = key_toward(target, current.selected);
        let from = current.selected;
        driver.keys(&[key])?;
        wait_for("the selection to move", STEP_DEADLINE, NO_PAUSE, || {
            let after = rows(driver)?;
            Ok(after.selected != from)
        })?;
    }
}

fn key_toward(target: usize, selected: usize) -> &'static str {
    if target > selected {
        return NEXT_ROW;
    }
    PREVIOUS_ROW
}

fn processes_running(command_line: &str) -> Result<usize, String> {
    let mut pgrep = Command::new("pgrep");
    pgrep.args(["-f", command_line]);
    let output = pgrep
        .output()
        .map_err(|error| format!("could not run pgrep: {error}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().count())
}

fn newest(driver: &mut Driver) -> Result<(usize, TerminalId), String> {
    let current = rows(driver)?;
    let Some(terminal) = current.terminals.last() else {
        return Err("the arrangement has no rows".to_string());
    };
    let index = current.terminals.len() - 1;
    Ok((index, terminal.clone()))
}

fn x_count(driver: &mut Driver, terminal: &TerminalId) -> Result<usize, String> {
    let Some(lines) = driver.screen_text(terminal)? else {
        return Err(format!("terminal {} has no view", terminal.0));
    };
    let mut count = 0;
    for line in &lines {
        count += line.matches('x').count();
    }
    Ok(count)
}

fn typing_sample(driver: &mut Driver, terminal: &TerminalId) -> Result<f64, String> {
    let before = x_count(driver, terminal)?;
    let start = Instant::now();
    driver.text("x")?;
    wait_for("the typed x on screen", STEP_DEADLINE, NO_PAUSE, || {
        let now = x_count(driver, terminal)?;
        Ok(now > before)
    })?;
    let elapsed = start.elapsed();
    Ok(elapsed.as_secs_f64() * 1000.0)
}

fn typing_samples(
    driver: &mut Driver,
    terminal: &TerminalId,
    mut done: impl FnMut(usize) -> bool,
) -> Result<Vec<f64>, String> {
    let mut samples = Vec::new();
    while !done(samples.len()) {
        let sample = typing_sample(driver, terminal)?;
        samples.push(sample);
        thread::sleep(TYPING_SPACING);
    }
    Ok(samples)
}

fn type_quietly(driver: &mut Driver, samples: usize) -> Result<Vec<f64>, String> {
    let (index, terminal) = newest(driver)?;
    select(driver, index)?;
    driver.text("cat\n")?;
    thread::sleep(QUIET_PAUSE);
    typing_samples(driver, &terminal, |taken| taken >= samples)
}

fn type_while_printing(driver: &mut Driver, flags: &Flags) -> Result<Vec<f64>, String> {
    let filler = "a".repeat(FLOOD_WIDTH);
    let flood = format!("yes {filler}\n");
    for index in 1..=flags.flooding {
        select(driver, index)?;
        driver.text(&flood)?;
    }

    let (newest_index, terminal) = newest(driver)?;
    select(driver, newest_index)?;
    thread::sleep(FLOOD_WARMUP);
    let printing = processes_running(&format!("yes {filler}"))?;
    if printing < flags.flooding {
        return Err(format!(
            "{printing} of the {} printing terminals are running yes",
            flags.flooding
        ));
    }

    let window = Duration::from_secs(flags.flood_seconds);
    let end = Instant::now() + window;
    let samples = typing_samples(driver, &terminal, |_| Instant::now() >= end)?;

    for index in 1..=flags.flooding {
        select(driver, index)?;
        driver.text(INTERRUPT)?;
    }
    Ok(samples)
}

fn fill_scrollback(driver: &mut Driver, full: usize, work: &Path) -> Result<(), String> {
    for index in 1..=full {
        select(driver, index)?;
        let marker = work.join(format!("filled-{index}"));
        let command = format!("seq 1 {FILL_LINES} && touch '{}'\n", marker.display());
        driver.text(&command)?;
        wait_for("the scrollback to fill", FILL_DEADLINE, POLL_PAUSE, || {
            Ok(marker.exists())
        })?;
    }
    Ok(())
}

fn switch_step(
    driver: &mut Driver,
    terminals: &[TerminalId],
    key: &str,
    expected: usize,
) -> Result<f64, String> {
    let start = Instant::now();
    driver.keys(&[key])?;
    wait_for("the selection to arrive", STEP_DEADLINE, NO_PAUSE, || {
        let current = rows(driver)?;
        Ok(current.selected == expected)
    })?;
    let terminal = &terminals[expected];
    wait_for("the terminal's screen", STEP_DEADLINE, NO_PAUSE, || {
        let screen = driver.screen_text(terminal)?;
        Ok(screen.is_some())
    })?;
    let elapsed = start.elapsed();
    Ok(elapsed.as_secs_f64() * 1000.0)
}

fn switch_every_entry(driver: &mut Driver) -> Result<Vec<f64>, String> {
    let start = rows(driver)?;
    let count = start.terminals.len();
    let mut current = start.selected;
    let mut samples = Vec::new();

    for _ in 0..count {
        let expected = (current + 1) % count;
        let sample = switch_step(driver, &start.terminals, NEXT_ROW, expected)?;
        samples.push(sample);
        current = expected;
    }
    for _ in 0..count {
        let expected = (current + count - 1) % count;
        let sample = switch_step(driver, &start.terminals, PREVIOUS_ROW, expected)?;
        samples.push(sample);
        current = expected;
    }
    Ok(samples)
}

fn switch_between_two(driver: &mut Driver) -> Result<Vec<f64>, String> {
    let start = rows(driver)?;
    let newest_index = start.terminals.len() - 1;
    let before_index = newest_index - 1;
    select(driver, newest_index)?;

    let mut samples = Vec::new();
    for _ in 0..SWITCH_PAIRS {
        let back = switch_step(driver, &start.terminals, PREVIOUS_ROW, before_index)?;
        samples.push(back);
        let forward = switch_step(driver, &start.terminals, NEXT_ROW, newest_index)?;
        samples.push(forward);
    }
    Ok(samples)
}
