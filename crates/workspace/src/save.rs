use serde_json::{Map, Value, json};
use taco_agents::{Harness, harness_of, legacy_harness};
use taco_protocol::{Panels, TerminalId};

use crate::sessions::AgentSession;

/// The save file the Rust app writes: one row per terminal.
pub const SAVE_VERSION: u64 = 2;
/// The save file the Electron app writes: groups of split panes.
pub const ELECTRON_VERSION: u64 = 1;

/// A save file read and repaired: what a restore rebuilds.
#[derive(Clone, Debug, PartialEq)]
pub struct Saved {
    pub rows: Vec<SavedRow>,
    pub selected: usize,
    pub tab_groups: Vec<SavedTabGroup>,
    pub panels: Option<Panels>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedRow {
    /// None for a row from a file without ids, such as the Electron app's.
    pub id: Option<TerminalId>,
    pub directory: Option<String>,
    pub agent: Option<AgentSession>,
    /// An index into `Saved::tab_groups`.
    pub tab_group: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedTabGroup {
    pub name: String,
    pub collapsed: bool,
}

/// The version a save file's text declares, if it is a JSON object with one.
pub fn version_of(text: &str) -> Option<u64> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed.get("version")?.as_u64()
}

/// The save a file's text describes, from either app, or none when it cannot be one.
pub fn read_saved(text: &str) -> Option<Saved> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let file = parsed.as_object()?;
    let version = file.get("version")?.as_u64()?;
    let read = match version {
        ELECTRON_VERSION => read_electron(file)?,
        SAVE_VERSION => read_rows(file)?,
        _ => return None,
    };
    Some(repaired(read))
}

/// The text of a version 2 save file.
pub fn saved_text(saved: &Saved) -> String {
    let rows = saved.rows.iter().map(|row| {
        let agent = row.agent.as_ref().map(|agent| {
            json!({ "harness": agent.harness.0, "sessionId": agent.session_id, "cwd": agent.cwd })
        });
        let id = row.id.as_ref().map(|id| id.0.clone());
        json!({ "id": id, "directory": row.directory, "agent": agent, "tabGroup": row.tab_group })
    });
    let rows: Vec<Value> = rows.collect();
    let tab_groups = saved
        .tab_groups
        .iter()
        .map(|record| json!({ "name": record.name, "collapsed": record.collapsed }));
    let tab_groups: Vec<Value> = tab_groups.collect();
    let file = json!({
        "version": SAVE_VERSION,
        "rows": rows,
        "selected": saved.selected,
        "tabGroups": tab_groups,
        "panels": saved.panels,
    });
    file.to_string()
}

fn string_in(record: &Map<String, Value>, key: &str) -> Option<String> {
    let value = record.get(key)?.as_str()?;
    Some(value.to_string())
}

fn index_in(record: &Map<String, Value>, key: &str) -> Option<usize> {
    let value = record.get(key)?.as_u64()?;
    usize::try_from(value).ok()
}

fn bool_in(record: &Map<String, Value>, key: &str) -> Option<bool> {
    record.get(key)?.as_bool()
}

fn panels_in(file: &Map<String, Value>) -> Option<Panels> {
    let value = file.get("panels")?;
    serde_json::from_value(value.clone()).ok()
}

/// A session one agent accepts: an id it would type into a shell, and a directory.
fn agent_session_for(harness: &Harness, value: &Value) -> Option<AgentSession> {
    let record = value.as_object()?;
    let session_id = string_in(record, "sessionId")?;
    let cwd = string_in(record, "cwd")?;
    if !harness.is_session_id(&session_id) || cwd.is_empty() {
        return None;
    }
    let session = AgentSession {
        harness: harness.id(),
        session_id,
        cwd,
    };
    Some(session)
}

fn agent_session_from(value: &Value) -> Option<AgentSession> {
    let record = value.as_object()?;
    let harness_name = string_in(record, "harness")?;
    let harness = harness_of(&harness_name)?;
    agent_session_for(harness, value)
}

fn tab_groups_in(file: &Map<String, Value>) -> Vec<SavedTabGroup> {
    let Some(Value::Array(records)) = file.get("tabGroups") else {
        return Vec::new();
    };
    let read = records.iter().map(|value| {
        let empty = Map::new();
        let record = match value.as_object() {
            Some(record) => record,
            None => &empty,
        };
        let mut name = String::new();
        if let Some(found) = string_in(record, "name") {
            name = found;
        }
        let collapsed = bool_in(record, "collapsed") == Some(true);
        SavedTabGroup { name, collapsed }
    });
    read.collect()
}

fn read_rows(file: &Map<String, Value>) -> Option<Saved> {
    let Value::Array(values) = file.get("rows")? else {
        return None;
    };
    let records = values.iter().filter_map(Value::as_object);
    let rows = records.map(|record| {
        let id = match string_in(record, "id") {
            Some(id) if !id.is_empty() => Some(TerminalId(id)),
            _ => None,
        };
        let agent = match record.get("agent") {
            Some(value) => agent_session_from(value),
            None => None,
        };
        SavedRow {
            id,
            directory: string_in(record, "directory"),
            agent,
            tab_group: index_in(record, "tabGroup"),
        }
    });
    let rows: Vec<SavedRow> = rows.collect();
    let saved = Saved {
        selected: clamped(index_in(file, "selected"), rows.len()),
        rows,
        tab_groups: tab_groups_in(file),
        panels: panels_in(file),
    };
    Some(saved)
}

fn electron_pane(record: &Map<String, Value>, tab_group: Option<usize>) -> SavedRow {
    let agent = match (record.get("agent"), record.get("claude")) {
        (Some(value), _) => agent_session_from(value),
        // a file saved before agents had ids holds a Claude session under "claude"
        (None, Some(legacy)) => agent_session_for(legacy_harness(), legacy),
        (None, None) => None,
    };
    SavedRow {
        id: None,
        directory: string_in(record, "cwd"),
        agent,
        tab_group,
    }
}

/// An Electron save as rows, one per split pane in order; colors are dropped.
fn read_electron(file: &Map<String, Value>) -> Option<Saved> {
    let Value::Array(groups) = file.get("groups")? else {
        return None;
    };
    let mut kept: Vec<(Vec<SavedRow>, usize)> = Vec::new();
    for group in groups.iter().filter_map(Value::as_object) {
        let Some(Value::Array(panes)) = group.get("panes") else {
            continue;
        };
        let tab_group = index_in(group, "tabGroup");
        let records = panes.iter().filter_map(Value::as_object);
        let rows: Vec<SavedRow> = records.map(|pane| electron_pane(pane, tab_group)).collect();
        if rows.is_empty() {
            continue;
        }
        let active = clamped(index_in(group, "activePane"), rows.len());
        kept.push((rows, active));
    }

    let active_group = clamped(index_in(file, "activeGroup"), kept.len());
    let mut selected = 0;
    let mut rows = Vec::new();
    for (index, (group_rows, active_pane)) in kept.into_iter().enumerate() {
        if index == active_group {
            selected = rows.len() + active_pane;
        }
        rows.extend(group_rows);
    }
    let saved = Saved {
        rows,
        selected,
        tab_groups: tab_groups_in(file),
        panels: None,
    };
    Some(saved)
}

fn clamped(index: Option<usize>, length: usize) -> usize {
    let last = length.saturating_sub(1);
    match index {
        Some(index) => index.min(last),
        None => 0,
    }
}

/// A hand-edited file made sane: rows with an unknown or scattered tab group load ungrouped, and unused tab groups are dropped.
fn repaired(mut saved: Saved) -> Saved {
    let known = saved.tab_groups.len();
    let mut seen = Vec::new();
    let mut previous = None;
    for row in &mut saved.rows {
        if let Some(index) = row.tab_group {
            // a tab group's rows sit together, so only its first run keeps it
            let straggler = seen.contains(&index) && previous != Some(index);
            if index >= known || straggler {
                row.tab_group = None;
            } else if !seen.contains(&index) {
                seen.push(index);
            }
        }
        previous = row.tab_group;
    }

    let mut renumbered = vec![None; known];
    let mut kept = Vec::new();
    for (index, record) in saved.tab_groups.iter().enumerate() {
        if seen.contains(&index) {
            renumbered[index] = Some(kept.len());
            kept.push(record.clone());
        }
    }
    for row in &mut saved.rows {
        if let Some(index) = row.tab_group {
            row.tab_group = renumbered[index];
        }
    }
    saved.tab_groups = kept;
    saved
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELECTRON_FILE: &str = include_str!("../tests/fixtures/electron-workspace.json");

    fn shape(saved: &Saved) -> Vec<(Option<String>, Option<usize>, Option<String>)> {
        let rows = saved.rows.iter().map(|row| {
            let session = row.agent.as_ref().map(|agent| agent.session_id.clone());
            (row.directory.clone(), row.tab_group, session)
        });
        rows.collect()
    }

    #[test]
    fn an_electron_save_becomes_one_row_per_pane_in_order_with_its_colors_dropped() {
        let saved = read_saved(ELECTRON_FILE).unwrap();
        let tacos = Some("/Users/me/code/tacos".to_string());
        let first = Some("123e4567-e89b-12d3-a456-426614174000".to_string());
        let legacy = Some("223e4567-e89b-12d3-a456-426614174000".to_string());

        assert_eq!(
            shape(&saved),
            vec![
                (tacos.clone(), Some(0), first),
                (Some("/Users/me/code/tacos/docs".into()), Some(0), None),
                (Some("/Users/me".into()), Some(0), legacy),
                (None, None, None),
            ]
        );
        assert_eq!(saved.selected, 1, "the active pane of the active group");
        assert_eq!(
            saved.tab_groups,
            vec![SavedTabGroup {
                name: "Tacos".into(),
                collapsed: false
            }]
        );
        assert_eq!(saved.rows[2].agent.as_ref().unwrap().cwd, "/Users/me/notes");

        let later_group = r#"{"version":1,"activeGroup":1,"groups":[{"panes":[{},{}]},{"panes":[{},{}],"activePane":1}]}"#;
        assert_eq!(read_saved(later_group).unwrap().selected, 3);
    }

    #[test]
    fn a_hand_edited_file_loads_repaired_and_a_file_that_cannot_be_one_loads_nothing() {
        let edited = r#"{"version":2,"selected":9,"tabGroups":[{"name":"a"},{"name":"unused"},{"name":"b"}],
            "rows":[{"tabGroup":0},{"tabGroup":2},{"tabGroup":0},{"tabGroup":7},"not a row",
            {"agent":{"harness":"claude","sessionId":"$(rm -rf /)","cwd":"/x"}},
            {"agent":{"harness":"gone","sessionId":"123e4567-e89b-12d3-a456-426614174000","cwd":"/x"}},
            {"id":"","agent":{"harness":"claude","sessionId":"123e4567-e89b-12d3-a456-426614174000","cwd":""}}]}"#;
        let saved = read_saved(edited).unwrap();
        let groups: Vec<Option<usize>> = saved.rows.iter().map(|row| row.tab_group).collect();
        assert_eq!(groups, vec![Some(0), Some(1), None, None, None, None, None]);
        let names: Vec<&str> = saved
            .tab_groups
            .iter()
            .map(|record| record.name.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(saved.selected, 6);
        assert!(saved.rows.iter().all(|row| row.agent.is_none()));
        assert_eq!(saved.rows[6].id, None, "an empty id is minted afresh");

        assert_eq!(read_saved("{"), None);
        assert_eq!(read_saved(r#"{"version":3,"rows":[]}"#), None);
        assert_eq!(read_saved(r#"{"version":2}"#), None);
    }
}
