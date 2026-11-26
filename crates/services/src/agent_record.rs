use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value, json};

pub const RECORD_FILE: &str = "agents.json";

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AgentRecord {
    pub declined: bool,
    pub written: Vec<String>,
}

fn read_record_file(config_directory: &Path) -> Map<String, Value> {
    let path = config_directory.join(RECORD_FILE);
    let Ok(text) = fs::read_to_string(path) else {
        return Map::new();
    };
    let parsed = serde_json::from_str::<Value>(&text);
    match parsed {
        Ok(Value::Object(file)) => file,
        _ => Map::new(),
    }
}

fn record_of(entry: &Value) -> Option<AgentRecord> {
    let declined_value = entry.get("declined")?;
    let declined = declined_value.as_bool()?;
    let written_value = entry.get("written")?;
    let items = written_value.as_array()?;
    let mut written = Vec::new();
    for item in items {
        let text = item.as_str()?;
        written.push(text.to_string());
    }
    Some(AgentRecord { declined, written })
}

pub fn read_agent_record(config_directory: &Path, harness_id: &str) -> AgentRecord {
    let file = read_record_file(config_directory);
    let Some(entry) = file.get(harness_id) else {
        return AgentRecord::default();
    };
    let record = record_of(entry);
    let Some(record) = record else {
        return AgentRecord::default();
    };
    record
}

pub fn write_agent_record(
    config_directory: &Path,
    harness_id: &str,
    record: &AgentRecord,
) -> io::Result<()> {
    let mut file = read_record_file(config_directory);
    let entry = json!({ "declined": record.declined, "written": record.written });
    file.insert(harness_id.to_string(), entry);

    fs::create_dir_all(config_directory)?;
    let path = config_directory.join(RECORD_FILE);
    let text = serde_json::to_string_pretty(&file)?;
    fs::write(path, format!("{text}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_agents_are_kept_apart_and_a_broken_file_reads_as_nothing() {
        let temp = tempfile::TempDir::new().unwrap();
        let directory = temp.path().join("not-yet-created");
        let claude = AgentRecord {
            declined: false,
            written: vec!["Notification:taco-shells-attention.sh".to_string()],
        };
        let other = AgentRecord {
            declined: true,
            written: Vec::new(),
        };

        write_agent_record(&directory, "claude", &claude).unwrap();
        write_agent_record(&directory, "other", &other).unwrap();
        let read_claude = read_agent_record(&directory, "claude");
        let read_other = read_agent_record(&directory, "other");
        let read_unknown = read_agent_record(&directory, "unknown");
        assert_eq!(read_claude, claude);
        assert_eq!(read_other, other);
        assert_eq!(read_unknown, AgentRecord::default());

        let path = directory.join(RECORD_FILE);
        fs::write(&path, "not json").unwrap();
        let from_broken_file = read_agent_record(&directory, "claude");
        assert_eq!(from_broken_file, AgentRecord::default());

        fs::write(&path, r#"{"claude": {"declined": "yes", "written": []}}"#).unwrap();
        let from_bad_entry = read_agent_record(&directory, "claude");
        assert_eq!(from_bad_entry, AgentRecord::default());
    }
}
