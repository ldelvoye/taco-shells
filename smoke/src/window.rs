use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use taco_drive::{Appearance, Point, Rect};
use taco_protocol::{DrawnState, Edge, TerminalId};
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
const RIGHT_EDGE_INSET: f32 = 20.;
const SHELL_DEADLINE: Duration = Duration::from_secs(15);
const SHELL_POLL: Duration = Duration::from_millis(50);
// The command holds `$((6*7))`, so the line `smoke-42` can only be the shell's answer, never the typed echo.
const TYPED_COMMAND: &str = "echo smoke-$((6*7))";
const TYPED_ANSWER: &str = "smoke-42";
const NEXT_ROW: &str = "cmd-shift-]";
const PREVIOUS_ROW: &str = "cmd-shift-[";
// Nothing in the app binds ctrl-u, so the shell gets it and erases the line typed before it.
const UNBOUND_CHORD: &str = "ctrl-u";
const ERASED_TEXT: &str = "echo garbage";
const CHORD_COMMAND: &str = "echo chord-$((3+3))";
const CHORD_ANSWER: &str = "chord-6";
const SECOND_RUN_WINDOW: Duration = Duration::from_millis(300);
const OPEN_SETTINGS: &str = "cmd-,";
const CLOSE_TERMINAL: &str = "cmd-w";
const LEAKED_COMMAND: &str = "echo leaked-$((5+5))";
const LEAKED_ANSWER: &str = "leaked-10";
const RETURNED_COMMAND: &str = "echo returned-$((4*4))";
const RETURNED_ANSWER: &str = "returned-16";
const SCROLLBACK_PART: &str = "settings:scrollback";
const SETTINGS_FILE: &str = "settings.json";
const TYPED_SCROLLBACK: &str = "5000";
const WRITTEN_SCROLLBACK: &str = "\"scrollback\":5000";
const OPEN_SHORTCUTS: &str = "cmd-shift-,";
const ADD_NEW_TERMINAL_PART: &str = "shortcut-add:terminal.new";
const KEYBINDINGS_FILE: &str = "keybindings.json";
const PRESSED_SHORTCUT: &str = "cmd-j";
const WRITTEN_SHORTCUT: &str = "\"cmd+j\":\"terminal.new\"";
const PAGE_COLUMN_PART: &str = "page:column";
const SAME_PLACE: f32 = 0.5;
const UNWANTED_SCROLLBACK: &str = "7777";
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

pub fn a_dragged_panel_moves_to_the_edge_it_is_dropped_on(
    _: &Smoke,
    driver: &mut Driver,
) -> Result<(), String> {
    let before = drawn(driver)?;
    if before.panels.terminals.edge == Edge::Right {
        return Err("the Terminals panel already starts on the right".to_string());
    }
    let title = required(driver, "panel:terminals:title")?;
    let middle = required(driver, "middle")?;
    let right_of_window = match driver.bounds("panel:agent_activity")? {
        Some(activity) => activity.origin.x + activity.width,
        None => middle.origin.x + middle.width,
    };
    let from = center(&title);
    let to = Point {
        x: right_of_window - RIGHT_EDGE_INSET,
        y: middle.origin.y + middle.height / 2.,
    };
    driver.drag(from, to)?;

    let after = settled(driver, before.revision)?;
    if after.panels.terminals.edge != Edge::Right {
        return Err(format!(
            "the Terminals panel is on {:?} after a drop on the right edge",
            after.panels.terminals.edge
        ));
    }
    let panel = required(driver, "panel:terminals")?;
    let moved_middle = required(driver, "middle")?;
    if panel.origin.x <= moved_middle.origin.x {
        return Err("the Terminals panel is on the right but drawn left of the middle".to_string());
    }
    Ok(())
}

pub fn a_typed_command_is_answered_on_screen(_: &Smoke, driver: &mut Driver) -> Result<(), String> {
    let state = drawn(driver)?;
    let Some(arrangement) = &state.arrangement else {
        return Err("the window drew no rows".to_string());
    };
    let Some(row) = arrangement.rows.get(arrangement.selected) else {
        return Err("no row is selected".to_string());
    };
    let terminal = row.terminal.clone();

    shown_lines(driver, &terminal, |lines| {
        lines.iter().any(|line| !line.is_empty())
    })?;
    driver.text(TYPED_COMMAND)?;
    driver.keys(&["enter"])?;
    shown_lines(driver, &terminal, |lines| {
        lines.iter().any(|line| line == TYPED_ANSWER)
    })?;
    Ok(())
}

/// cmd+w closes the page and not the terminal behind it, what is typed on the page never reaches the shell, and once
/// Esc closes the page the terminal has the keyboard again.
pub fn the_settings_page_keeps_and_returns_the_keyboard(
    _: &Smoke,
    driver: &mut Driver,
) -> Result<(), String> {
    let before = drawn(driver)?;
    let rows = terminals_of(&before);
    let rows_before = rows.len();
    let selected = selected_index(&before);
    let Some(terminal) = selected.and_then(|index| rows.get(index).cloned()) else {
        return Err("no row is selected".to_string());
    };

    driver.keys(&[OPEN_SETTINGS, CLOSE_TERMINAL])?;
    thread::sleep(SECOND_RUN_WINDOW);
    let after_close = drawn(driver)?;
    let rows_after = terminals_of(&after_close).len();
    if rows_after != rows_before {
        return Err(format!(
            "cmd+w over the page left {rows_after} rows of {rows_before}"
        ));
    }

    driver.keys(&[OPEN_SETTINGS])?;
    driver.text(LEAKED_COMMAND)?;
    driver.keys(&["enter", "escape"])?;
    driver.text(RETURNED_COMMAND)?;
    driver.keys(&["enter"])?;
    shown_lines(driver, &terminal, |lines| {
        lines.iter().any(|line| line == RETURNED_ANSWER)
    })?;
    let lines = driver.screen_text(&terminal)?;
    let leaked = lines.is_some_and(|lines| lines.iter().any(|line| line == LEAKED_ANSWER));
    if leaked {
        return Err("text typed on the settings page reached the shell".to_string());
    }
    Ok(())
}

/// A number typed into the page's Scrollback field and entered is written to settings.json.
pub fn a_typed_scrollback_is_written_to_the_settings_file(
    smoke: &Smoke,
    driver: &mut Driver,
) -> Result<(), String> {
    driver.keys(&[OPEN_SETTINGS])?;
    let field = settled_bounds(driver, SCROLLBACK_PART)?;
    let inside = center(&field);
    driver.drag(inside, inside)?;
    driver.keys(&["cmd-a"])?;
    driver.text(TYPED_SCROLLBACK)?;
    driver.keys(&["enter"])?;
    let settings = smoke.config.join(SETTINGS_FILE);
    file_gains(&settings, WRITTEN_SCROLLBACK)?;
    // The first Esc is the field's, the second closes the page.
    driver.keys(&["escape", "escape"])?;
    Ok(())
}

/// Keys pressed on a shortcut row, after its +, are written to keybindings.json for that command.
pub fn a_shortcut_set_by_pressing_keys_is_written_to_the_keybindings_file(
    smoke: &Smoke,
    driver: &mut Driver,
) -> Result<(), String> {
    driver.keys(&[OPEN_SHORTCUTS])?;
    let add = settled_bounds(driver, ADD_NEW_TERMINAL_PART)?;
    let inside = center(&add);
    driver.drag(inside, inside)?;
    driver.keys(&[PRESSED_SHORTCUT])?;
    let keybindings = smoke.config.join(KEYBINDINGS_FILE);
    file_gains(&keybindings, WRITTEN_SHORTCUT)?;
    // Esc goes back to Settings, then closes it.
    driver.keys(&["escape", "escape"])?;
    Ok(())
}

/// A page fading out after it closes sits exactly where it was, and a click on it then changes nothing.
pub fn a_closing_page_stays_in_place_and_takes_no_clicks(
    smoke: &Smoke,
    driver: &mut Driver,
) -> Result<(), String> {
    driver.keys(&[OPEN_SETTINGS])?;
    let open_column = settled_bounds(driver, PAGE_COLUMN_PART)?;
    let field = settled_bounds(driver, SCROLLBACK_PART)?;
    driver.keys(&["escape"])?;
    let Some(closing_column) = driver.bounds(PAGE_COLUMN_PART)? else {
        return Err("the closed page was not drawn fading out".to_string());
    };
    let shifted = (closing_column.origin.x - open_column.origin.x).abs();
    if shifted > SAME_PLACE {
        return Err(format!(
            "the closing page moved {shifted} points: open at {:?}, closing at {:?}",
            open_column.origin, closing_column.origin
        ));
    }
    let settings = smoke.config.join(SETTINGS_FILE);
    let before = read_text(&settings)?;
    let inside = center(&field);
    driver.drag(inside, inside)?;
    driver.text(UNWANTED_SCROLLBACK)?;
    driver.keys(&["enter"])?;
    thread::sleep(SECOND_RUN_WINDOW);
    let after = read_text(&settings)?;
    if after != before {
        return Err(format!(
            "a click on the closing page changed settings.json: {after}"
        ));
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

// Compared with all whitespace taken out, so the writer's formatting does not matter.
fn file_gains(path: &Path, wanted: &str) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let compact: String = text.split_whitespace().collect();
        if compact.contains(wanted) {
            return Ok(());
        }
        if started.elapsed() >= SETTLE_DEADLINE {
            return Err(format!("{} never gained {wanted}: {text}", path.display()));
        }
        thread::sleep(SETTLE_POLL);
    }
}

/// A bound chord runs its command once and types nothing; an unbound one reaches the shell. If either bracket chord
/// leaked its character, or ctrl-u never arrived, the shell would not answer `chord-6` on a line of its own.
pub fn chords_reach_their_command_or_the_shell(
    _: &Smoke,
    driver: &mut Driver,
) -> Result<(), String> {
    let state = drawn(driver)?;
    let Some(arrangement) = &state.arrangement else {
        return Err("the window drew no rows".to_string());
    };
    let count = arrangement.rows.len();
    if count < 2 {
        return Err(format!("the list has {count} rows, two are needed"));
    }
    let start = arrangement.selected;
    let terminal = arrangement.rows[start].terminal.clone();
    let next = (start + 1) % count;

    driver.text(ERASED_TEXT)?;
    driver.keys(&[UNBOUND_CHORD])?;

    driver.keys(&[NEXT_ROW])?;
    selected_becomes(driver, next)?;
    thread::sleep(SECOND_RUN_WINDOW);
    let after = drawn(driver)?;
    let still = selected_index(&after);
    if still != Some(next) {
        return Err(format!(
            "{NEXT_ROW} moved the selection more than one row: {still:?}"
        ));
    }

    driver.keys(&[PREVIOUS_ROW])?;
    selected_becomes(driver, start)?;

    driver.text(CHORD_COMMAND)?;
    driver.keys(&["enter"])?;
    shown_lines(driver, &terminal, |lines| {
        lines.iter().any(|line| line == CHORD_ANSWER)
    })
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

fn settled_bounds(driver: &mut Driver, part: &str) -> Result<Rect, String> {
    let started = Instant::now();
    loop {
        if let Some(rect) = driver.bounds(part)? {
            return Ok(rect);
        }
        if started.elapsed() >= SETTLE_DEADLINE {
            return Err(format!("{part} was never drawn"));
        }
        thread::sleep(SETTLE_POLL);
    }
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

fn selected_index(state: &DrawnState) -> Option<usize> {
    let arrangement = state.arrangement.as_ref()?;
    Some(arrangement.selected)
}

fn selected_becomes(driver: &mut Driver, wanted: usize) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let state = drawn(driver)?;
        let selected = selected_index(&state);
        if selected == Some(wanted) {
            return Ok(());
        }
        if started.elapsed() >= SETTLE_DEADLINE {
            return Err(format!("the selection is {selected:?}, not row {wanted}"));
        }
        thread::sleep(SETTLE_POLL);
    }
}

fn shown_lines(
    driver: &mut Driver,
    terminal: &TerminalId,
    wanted: impl Fn(&[String]) -> bool,
) -> Result<(), String> {
    let started = Instant::now();
    let mut last = None;
    loop {
        let lines = driver.screen_text(terminal)?;
        if let Some(lines) = &lines
            && wanted(lines)
        {
            return Ok(());
        }
        if lines.is_some() {
            last = lines;
        }
        if started.elapsed() >= SHELL_DEADLINE {
            let seen = match last {
                Some(lines) => lines.join(" | "),
                None => "no view".to_string(),
            };
            return Err(format!(
                "terminal {} after {SHELL_DEADLINE:?}: {seen}",
                terminal.0
            ));
        }
        thread::sleep(SHELL_POLL);
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
