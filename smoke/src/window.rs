use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use taco_drive::{Appearance, Point, Rect};
use taco_protocol::{DrawnState, TerminalId};
use taco_smoke::driver::Driver;

use crate::Smoke;

const SETTLE_DEADLINE: Duration = Duration::from_secs(5);
const SETTLE_POLL: Duration = Duration::from_millis(20);
const FIXED_PARTS: [&str; 6] = [
    "middle",
    "strip:terminals",
    "strip:agent_activity",
    "strip:settings",
    "panel:terminals",
    "panel:terminals:title",
];
const UNKNOWN_PART: &str = "row:no-such-terminal";
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
const PNG_HEADER_LENGTH: usize = 24;

pub fn draws_rows(_: &Smoke, driver: &mut Driver) -> Result<(), String> {
    let rows = row_terminals(driver)?;
    if rows.is_empty() {
        return Err("the window drew no rows".to_string());
    }
    Ok(())
}

pub fn every_marked_part_has_bounds(_: &Smoke, driver: &mut Driver) -> Result<(), String> {
    let rows = row_terminals(driver)?;
    let row_parts = rows.iter().map(|terminal| format!("row:{}", terminal.0));
    let fixed_parts = FIXED_PARTS.iter().map(|part| part.to_string());
    let parts: Vec<String> = fixed_parts.chain(row_parts).collect();
    for part in &parts {
        let Some(rect) = driver.bounds(part)? else {
            return Err(format!("{part} has no bounds"));
        };
        if rect.width <= 0. || rect.height <= 0. {
            return Err(format!("{part} is drawn with no size: {rect:?}"));
        }
    }
    let unknown = driver.bounds(UNKNOWN_PART)?;
    if unknown.is_some() {
        return Err(format!("{UNKNOWN_PART}, which is not drawn, has bounds"));
    }
    Ok(())
}

pub fn a_dragged_row_moves(_: &Smoke, driver: &mut Driver) -> Result<(), String> {
    let before = drawn(driver)?;
    let rows = terminals_of(&before);
    let Some(target) = rows.get(2) else {
        return Err(format!(
            "the fixture has {} rows, three are needed",
            rows.len()
        ));
    };
    let dragged = &rows[0];
    let from = center(&required(driver, &format!("row:{}", dragged.0))?);
    let target_rect = required(driver, &format!("row:{}", target.0))?;
    let to = Point {
        x: from.x,
        y: target_rect.origin.y + target_rect.height * 0.75,
    };
    driver.drag(from, to)?;

    let after = settled(driver, before.revision)?;
    let order = terminals_of(&after);
    let first = &order[0];
    if first == dragged {
        return Err(format!(
            "{} is still the first row after the drag",
            dragged.0
        ));
    }
    Ok(())
}

pub fn draws_in_light_and_dark(smoke: &Smoke, driver: &mut Driver) -> Result<(), String> {
    let Some(work) = smoke.config.parent() else {
        return Err(format!("{} has no parent folder", smoke.config.display()));
    };
    for (appearance, name) in [
        (Appearance::Light, "look-light.png"),
        (Appearance::Dark, "look-dark.png"),
    ] {
        let path = work.join(name);
        driver.render(&path, appearance)?;
        let (width, height) = png_size(&path)?;
        if width == 0 || height == 0 {
            return Err(format!("{} is empty", path.display()));
        }
        println!("      {} ({width}x{height})", path.display());
    }
    Ok(())
}

fn drawn(driver: &mut Driver) -> Result<DrawnState, String> {
    driver.drawn_state()
}

fn terminals_of(state: &DrawnState) -> Vec<TerminalId> {
    let Some(arrangement) = &state.arrangement else {
        return Vec::new();
    };
    let rows = arrangement.rows.iter();
    rows.map(|row| row.terminal.clone()).collect()
}

fn row_terminals(driver: &mut Driver) -> Result<Vec<TerminalId>, String> {
    let state = drawn(driver)?;
    Ok(terminals_of(&state))
}

fn required(driver: &mut Driver, part: &str) -> Result<Rect, String> {
    match driver.bounds(part)? {
        Some(rect) => Ok(rect),
        None => Err(format!("{part} has no bounds")),
    }
}

fn center(rect: &Rect) -> Point {
    Point {
        x: rect.origin.x + rect.width / 2.,
        y: rect.origin.y + rect.height / 2.,
    }
}

fn settled(driver: &mut Driver, revision: u64) -> Result<DrawnState, String> {
    let started = Instant::now();
    loop {
        let state = drawn(driver)?;
        if state.revision > revision {
            return Ok(state);
        }
        if started.elapsed() >= SETTLE_DEADLINE {
            return Err(format!(
                "nothing changed within {SETTLE_DEADLINE:?} of the drag"
            ));
        }
        thread::sleep(SETTLE_POLL);
    }
}

fn png_size(path: &Path) -> Result<(u32, u32), String> {
    let mut file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let mut header = [0u8; PNG_HEADER_LENGTH];
    file.read_exact(&mut header)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if header[..8] != PNG_SIGNATURE {
        return Err(format!("{} is not a PNG", path.display()));
    }
    let width_bytes = [header[16], header[17], header[18], header[19]];
    let height_bytes = [header[20], header[21], header[22], header[23]];
    Ok((
        u32::from_be_bytes(width_bytes),
        u32::from_be_bytes(height_bytes),
    ))
}
