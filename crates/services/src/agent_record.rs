use std::fs;
use std::io;
use std::path::Path;

use serde_json::{Map, Value, json};

pub const RECORD_FILE: &str = "agents.json";
pub const UNREADABLE_RECORD_FILE: &str = "agents.unreadable.json";

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct AgentRecord {
    pub declined: bool,
    pub written: Vec<String>,
}

enum RecordFile {
    Missing,
    Read(Map<String, Value>),
    Unreadable(String),
}

fn load_record_file(config_directory: &Path) -> RecordFile {
    let path = config_directory.join(RECORD_FILE);
    let Ok(text) = fs::read_to_string(path) else {
        return RecordFile::Missing;
    };
    let parsed = serde_json::from_str::<Value>(&text);
    match parsed {
        Ok(Value::Object(file)) => RecordFile::Read(file),
        _ => RecordFile::Unreadable(text),
    }
}

fn read_record_file(config_directory: &Path) -> Map<String, Value> {
    let loaded = load_record_file(config_directory);
    match loaded {
        RecordFile::Read(file) => file,
        RecordFile::Missing | RecordFile::Unreadable(_) => Map::new(),
    }
}

fn replace_record_file(config_directory: &Path, text: &str) -> io::Result<()> {
    let path = config_directory.join(RECORD_FILE);
    let process = std::process::id();
    let temporary_name = format!(".{RECORD_FILE}.{process}.taco-tmp");
    let temporary = config_directory.join(temporary_name);
    let written = fs::write(&temporary, text);
    let renamed = written.and_then(|()| fs::rename(&temporary, &path));
    if renamed.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    renamed
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
    fs::create_dir_all(config_directory)?;
    let loaded = load_record_file(config_directory);
    let mut file = match loaded {
        RecordFile::Read(file) => file,
        RecordFile::Missing => Map::new(),
        RecordFile::Unreadable(text) => {
            // Kept beside it before it is replaced, so the other agents' records are not lost without a trace.
            let aside = config_directory.join(UNREADABLE_RECORD_FILE);
            fs::write(aside, text)?;
            Map::new()
        }
    };
    let entry = json!({ "declined": record.declined, "written": record.written });
    file.insert(harness_id.to_string(), entry);

    let text = serde_json::to_string_pretty(&file)?;
    let with_newline = format!("{text}\n");
    replace_record_file(config_directory, &with_newline)
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

        let aside = directory.join(UNREADABLE_RECORD_FILE);
        assert!(!aside.exists());
        write_agent_record(&directory, "claude", &claude).unwrap();
        let kept = fs::read_to_string(&aside).unwrap();
        assert_eq!(kept, "not json");
        let after_rewrite = read_agent_record(&directory, "claude");
        assert_eq!(after_rewrite, claude);
        fs::remove_file(&aside).unwrap();
        write_agent_record(&directory, "other", &other).unwrap();
        assert!(!aside.exists());
        let names: Vec<String> = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        let leftovers: Vec<&String> = names
            .iter()
            .filter(|name| name.ends_with("taco-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        fs::write(&path, r#"{"claude": {"declined": "yes", "written": []}}"#).unwrap();
        let from_bad_entry = read_agent_record(&directory, "claude");
        assert_eq!(from_bad_entry, AgentRecord::default());
    }
}
