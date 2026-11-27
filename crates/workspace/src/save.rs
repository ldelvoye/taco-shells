use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use taco_agents::{Harness, harness_of};
use taco_protocol::{Arrangement, Panels, TabGroupId, TerminalId};

use crate::sessions::{AgentSession, AgentSessions};
use crate::storage::Storage;

/// The save file the Rust app writes: one row per terminal.
pub const SAVE_VERSION: u64 = 2;
/// How long the save waits after a change, so a burst of changes is written once.
pub const SAVE_DEBOUNCE_MS: u64 = 250;
const MAX_SAVE_WAIT_MS: u64 = 2_000;
const SAVE_RETRY_MS: u64 = 1_000;

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
    /// None for a row from a file without ids, such as a hand-edited one.
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

/// When the save is due and what it should hold, so a burst of changes is written once.
#[derive(Default)]
pub(crate) struct SaveScheduler {
    /// What the save should hold now.
    wanted_save: Option<Saved>,
    /// What the file was last written with.
    written_save: Option<Saved>,
    save_due: Option<u64>,
    /// When the oldest change not yet written was made.
    unsaved_since: Option<u64>,
}

/// The version a save file's text declares, if it is a JSON object with one.
pub fn version_of(text: &str) -> Option<u64> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    parsed.get("version")?.as_u64()
}

/// The save a file's text describes, or none when it cannot be one.
pub fn read_saved(text: &str) -> Option<Saved> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let file = parsed.as_object()?;
    let version = file.get("version")?.as_u64()?;
    if version != SAVE_VERSION {
        return None;
    }
    let read = read_rows(file)?;
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

pub(crate) fn saved_from(
    arrangement: &Arrangement,
    directories: &BTreeMap<TerminalId, String>,
    sessions: &AgentSessions,
    panels: &Panels,
) -> Saved {
    // a tab group's index in the file is its place in the list, so the records follow the rows
    let mut ordered: Vec<&TabGroupId> = Vec::new();
    for row in &arrangement.rows {
        if let Some(tab_group) = &row.tab_group
            && !ordered.contains(&tab_group)
        {
            ordered.push(tab_group);
        }
    }
    let rows = arrangement.rows.iter().map(|row| {
        let mut tab_group = None;
        if let Some(group) = &row.tab_group {
            tab_group = ordered.iter().position(|known| *known == group);
        }
        SavedRow {
            id: Some(row.terminal.clone()),
            directory: directories.get(&row.terminal).cloned(),
            agent: sessions.session_of(&row.terminal).cloned(),
            tab_group,
        }
    });
    let tab_groups = ordered.iter().filter_map(|id| {
        let mut records = arrangement.tab_groups.iter();
        let record = records.find(|record| record.id == **id)?;
        let saved = SavedTabGroup {
            name: record.name.clone(),
            collapsed: record.collapsed,
        };
        Some(saved)
    });
    Saved {
        rows: rows.collect(),
        selected: arrangement.selected,
        tab_groups: tab_groups.collect(),
        panels: Some(panels.clone()),
    }
}

impl SaveScheduler {
    pub(crate) fn due(&self) -> Option<u64> {
        self.save_due
    }

    // A steady stream of changes still saves at least every MAX_SAVE_WAIT_MS, so a crash never loses more.
    pub(crate) fn schedule(&mut self, saved: Option<Saved>, now_ms: u64) {
        let Some(saved) = saved else {
            return;
        };
        if self.wanted_save.as_ref() == Some(&saved) {
            return;
        }
        self.wanted_save = Some(saved);
        let first_change = *self.unsaved_since.get_or_insert(now_ms);
        let debounced = now_ms + SAVE_DEBOUNCE_MS;
        let latest = first_change + MAX_SAVE_WAIT_MS;
        self.save_due = Some(debounced.min(latest));
    }

    /// Replaces what the save should hold, at once and without a wait.
    pub(crate) fn want(&mut self, saved: Option<Saved>) {
        self.wanted_save = saved;
    }

    // After the freeze the save stays what it was when the quit began, so only the state from before it is written.
    pub(crate) fn write(&mut self, storage: &mut dyn Storage, now_ms: u64) {
        self.save_due = None;
        let Some(saved) = &self.wanted_save else {
            return;
        };
        if self.written_save.as_ref() == Some(saved) {
            self.unsaved_since = None;
            return;
        }
        let text = saved_text(saved);
        if !storage.write_save(&text) {
            self.save_due = Some(now_ms + SAVE_RETRY_MS);
            return;
        }
        self.written_save = Some(saved.clone());
        self.unsaved_since = None;
    }
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

    // A save as this build writes it, with an agent, a tab group and both panels. Renaming a field or a panel's
    // value would lose a user's terminals or layout without a word, so the file must read back whole and unchanged.
    const STORED_SAVE: &str = concat!(
        r#"{"version":2,"rows":["#,
        r#"{"id":"t1","directory":"/w","agent":{"harness":"claude","sessionId":"123e4567-e89b-12d3-a456-426614174000","cwd":"/w/tacos"},"tabGroup":0},"#,
        r#"{"id":"t2","directory":null,"agent":null,"tabGroup":null}"#,
        r#"],"selected":1,"tabGroups":[{"name":"Tacos","collapsed":true}],"panels":{"#,
        r#""Terminals":{"edge":"Left","open":true,"size":210.0},"#,
        r#""AgentActivity":{"edge":"Bottom","open":false,"size":340.5}}}"#,
    );

    #[test]
    fn a_save_this_build_wrote_reads_back_whole_and_writes_the_same_text() {
        let saved = read_saved(STORED_SAVE).unwrap();
        assert_eq!(saved_text(&saved), STORED_SAVE);
        let on_the_right = STORED_SAVE.replace("Bottom", "Right");
        let saved = read_saved(&on_the_right).unwrap();
        assert_eq!(saved_text(&saved), on_the_right);

        // the panels as builds before panels were stored by name wrote them
        let terminals_renamed = STORED_SAVE.replace(r#""Terminals":"#, r#""terminals":"#);
        let earlier_panels =
            terminals_renamed.replace(r#""AgentActivity":"#, r#""agent_activity":"#);
        let read = read_saved(&earlier_panels).unwrap();
        let current = read_saved(STORED_SAVE).unwrap();
        let rows_kept = Saved {
            panels: None,
            ..current
        };
        assert_eq!(
            read, rows_kept,
            "every row survives; only the layout resets"
        );
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
