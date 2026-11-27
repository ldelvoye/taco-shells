use std::path::Path;

use jsonc_parser::cst::{CstInputValue, CstNode, CstObject, CstRootNode};
use jsonc_parser::parse_to_serde_value;
use serde_json::{Map, Value};
use taco_agents::{HarnessSetup, HookEntry};
use taco_settings::JSONC_PARSE_OPTIONS;

const EMPTY_FILE: &str = "{}";
const HOOKS_KEY: &str = "hooks";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookWiring {
    None,
    Managed,
    ByHand,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HookEdit {
    Changed(String),
    Unchanged,
    Refused(String),
}

struct HookRef {
    event: String,
    group_index: usize,
    hook_index: usize,
    command: String,
    hooks_in_group: usize,
    groups_in_event: usize,
}

pub(crate) fn unreadable_problem(label: &str) -> String {
    format!("{label} could not be read as JSON, so nothing was changed.")
}

pub(crate) fn uneditable_problem(label: &str) -> String {
    format!("{label} could not be edited, so nothing was changed.")
}

fn parse_value(text: &str) -> Option<Value> {
    let parsed = parse_to_serde_value(text, &JSONC_PARSE_OPTIONS);
    parsed.ok()?
}

fn read_hooks(text: &str, label: &str) -> Result<Option<Map<String, Value>>, String> {
    let value = parse_value(text);
    let Some(Value::Object(mut root_map)) = value else {
        return Err(unreadable_problem(label));
    };
    let hooks = root_map.remove(HOOKS_KEY);
    let Some(hooks) = hooks else {
        return Ok(None);
    };
    let Value::Object(hooks_map) = hooks else {
        return Err(format!(
            "{label}: \"hooks\" must be an object, so nothing was changed."
        ));
    };
    Ok(Some(hooks_map))
}

fn hook_refs_of(hooks: &Map<String, Value>) -> Vec<HookRef> {
    let mut refs = Vec::new();
    for (event, groups) in hooks {
        let Value::Array(groups) = groups else {
            continue;
        };
        let groups_in_event = groups.len();
        for (group_index, group) in groups.iter().enumerate() {
            let group_hooks = group.get(HOOKS_KEY);
            let Some(Value::Array(group_hooks)) = group_hooks else {
                continue;
            };
            let hooks_in_group = group_hooks.len();
            for (hook_index, hook) in group_hooks.iter().enumerate() {
                let command = hook.get("command");
                let Some(Value::String(command)) = command else {
                    continue;
                };
                refs.push(HookRef {
                    event: event.clone(),
                    group_index,
                    hook_index,
                    command: command.clone(),
                    hooks_in_group,
                    groups_in_event,
                });
            }
        }
    }
    refs
}

fn command_for(entry: &HookEntry, hooks_directory: &Path) -> String {
    let path = hooks_directory.join(entry.script);
    let text = path.to_string_lossy().into_owned();
    if text.contains(' ') {
        return format!("\"{text}\"");
    }
    text
}

fn is_managed_command(command: &str, hooks_directory: &Path) -> bool {
    let directory = hooks_directory.to_string_lossy();
    let prefix = format!("{directory}/");
    let quoted_prefix = format!("\"{prefix}");
    command.starts_with(&prefix) || command.starts_with(&quoted_prefix)
}

fn is_script_path(word: &str, scripts: &[&str]) -> bool {
    for script in scripts {
        let suffix = format!("/{script}");
        if word == *script || word.ends_with(&suffix) {
            return true;
        }
    }
    false
}

// Managed runs a file in the app's hooks directory; by hand runs one of the setup's scripts from anywhere else. These decide what removal deletes.
fn is_hand_command(command: &str, scripts: &[&str], hooks_directory: &Path) -> bool {
    if is_managed_command(command, hooks_directory) {
        return false;
    }
    let words = command.split(|c: char| c == '"' || c == '\'' || c.is_whitespace());
    for word in words {
        if is_script_path(word, scripts) {
            return true;
        }
    }
    false
}

fn scripts_of(setup: &HarnessSetup) -> Vec<&'static str> {
    let mut scripts = Vec::new();
    for entry in setup.entries {
        scripts.push(entry.script);
    }
    scripts
}

fn group_for(command: String, is_async: bool) -> CstInputValue {
    let mut hook = vec![
        ("type".to_string(), CstInputValue::from("command")),
        ("command".to_string(), CstInputValue::from(command)),
    ];
    if is_async {
        hook.push(("async".to_string(), CstInputValue::from(true)));
    }
    let hooks = CstInputValue::Array(vec![CstInputValue::Object(hook)]);
    CstInputValue::Object(vec![(HOOKS_KEY.to_string(), hooks)])
}

fn malformed_event_problem(
    hooks: Option<&Map<String, Value>>,
    entries: &[HookEntry],
    label: &str,
) -> Option<String> {
    let hooks = hooks?;
    for entry in entries {
        let groups = hooks.get(entry.event);
        if let Some(groups) = groups
            && !groups.is_array()
        {
            return Some(format!(
                "{label}: \"hooks.{}\" must be a list, so nothing was changed.",
                entry.event
            ));
        }
    }
    None
}

fn insert_groups(source: &str, missing: &[(&HookEntry, String)]) -> Option<String> {
    let root = CstRootNode::parse(source, &JSONC_PARSE_OPTIONS).ok()?;
    let object = root.object_value_or_set();
    let hooks = object.object_value_or_set(HOOKS_KEY);
    for (entry, command) in missing {
        let group = group_for(command.clone(), entry.is_async);
        let list = hooks.array_value(entry.event);
        match list {
            Some(list) => {
                list.append(group);
            }
            None => {
                hooks.append(entry.event, CstInputValue::Array(vec![group]));
            }
        }
    }
    Some(root.to_string())
}

// The narrowest removal that leaves no empty group, event list or "hooks" object behind.
pub(crate) fn properties_named(object: &CstObject, name: &str) -> usize {
    let properties = object.properties();
    let mut count = 0;
    for property in properties {
        let decoded = property.decoded_name();
        if decoded.as_deref() == Some(name) {
            count += 1;
        }
    }
    count
}

fn command_of(hook_node: &CstNode) -> Option<String> {
    let hook = hook_node.as_object()?;
    let command_prop = hook.get("command")?;
    let command_value = command_prop.value()?;
    let command_text = command_value.as_string_lit()?;
    command_text.decoded_value().ok()
}

// The indexes come from serde's reading and are applied to the CST's, which can differ (a duplicate key), so the hook is checked before anything goes.
fn remove_hook(text: &str, hook_ref: &HookRef, event_count: usize) -> Option<String> {
    let root = CstRootNode::parse(text, &JSONC_PARSE_OPTIONS).ok()?;
    let object = root.object_value()?;
    let hooks_keys = properties_named(&object, HOOKS_KEY);
    if hooks_keys != 1 {
        return None;
    }
    let hooks_prop = object.get(HOOKS_KEY)?;
    let hooks = hooks_prop.object_value()?;
    let events = hooks.properties();
    if events.len() != event_count {
        return None;
    }
    let event_keys = properties_named(&hooks, &hook_ref.event);
    if event_keys != 1 {
        return None;
    }
    let event_prop = hooks.get(&hook_ref.event)?;
    let groups = event_prop.array_value()?;
    let group_nodes = groups.elements();
    if group_nodes.len() != hook_ref.groups_in_event {
        return None;
    }
    let group_node = group_nodes.get(hook_ref.group_index)?;
    let group = group_node.as_object()?;
    let group_hooks = group.array_value(HOOKS_KEY)?;
    let hook_nodes = group_hooks.elements();
    if hook_nodes.len() != hook_ref.hooks_in_group {
        return None;
    }
    let hook_node = hook_nodes.get(hook_ref.hook_index)?;
    let command = command_of(hook_node)?;
    if command != hook_ref.command {
        return None;
    }

    let group_has_one_hook = hook_ref.hooks_in_group <= 1;
    let event_has_one_group = hook_ref.groups_in_event <= 1;
    let hooks_has_one_event = event_count <= 1;
    if group_has_one_hook && event_has_one_group && hooks_has_one_event {
        hooks_prop.remove();
    } else if group_has_one_hook && event_has_one_group {
        event_prop.remove();
    } else if group_has_one_hook {
        group_node.clone().remove();
    } else {
        hook_node.clone().remove();
    }
    Some(root.to_string())
}

fn remove_matching(text: &str, label: &str, test: &dyn Fn(&str) -> bool) -> HookEdit {
    let mut edited = text.to_string();
    let mut previous_matches: Option<usize> = None;

    // One hook per pass, read again each time: every removal shifts the indexes after it.
    loop {
        let read = read_hooks(&edited, label);
        let hooks = match read {
            Ok(hooks) => hooks,
            Err(problem) => return HookEdit::Refused(problem),
        };
        let Some(hooks) = hooks else {
            break;
        };
        let refs = hook_refs_of(&hooks);
        let matches: Vec<&HookRef> = refs.iter().filter(|r| test(&r.command)).collect();
        let Some(first) = matches.first() else {
            break;
        };
        // A pass that removed nothing would loop forever, so the file is refused instead.
        if let Some(previous) = previous_matches
            && matches.len() >= previous
        {
            return HookEdit::Refused(uneditable_problem(label));
        }
        previous_matches = Some(matches.len());

        let removed = remove_hook(&edited, first, hooks.len());
        let Some(next) = removed else {
            return HookEdit::Refused(uneditable_problem(label));
        };
        edited = next;
    }

    if edited == text {
        return HookEdit::Unchanged;
    }
    HookEdit::Changed(edited)
}

pub(crate) fn hook_wiring_of(
    text: Option<&str>,
    setup: &HarnessSetup,
    hooks_directory: &Path,
) -> HookWiring {
    let Some(text) = text else {
        return HookWiring::None;
    };
    let read = read_hooks(text, setup.settings_file);
    let Ok(Some(hooks)) = read else {
        return HookWiring::None;
    };

    let refs = hook_refs_of(&hooks);
    let scripts = scripts_of(setup);
    let has_managed = refs
        .iter()
        .any(|r| is_managed_command(&r.command, hooks_directory));
    if has_managed {
        return HookWiring::Managed;
    }
    let has_hand = refs
        .iter()
        .any(|r| is_hand_command(&r.command, &scripts, hooks_directory));
    if has_hand {
        return HookWiring::ByHand;
    }
    HookWiring::None
}

pub fn add_entries(
    text: Option<&str>,
    entries: &[HookEntry],
    hooks_directory: &Path,
    label: &str,
) -> HookEdit {
    let mut source = EMPTY_FILE;
    let mut is_new_file = true;
    if let Some(text) = text
        && !text.trim().is_empty()
    {
        source = text;
        is_new_file = false;
    }

    let read = read_hooks(source, label);
    let hooks = match read {
        Ok(hooks) => hooks,
        Err(problem) => return HookEdit::Refused(problem),
    };
    let event_problem = malformed_event_problem(hooks.as_ref(), entries, label);
    if let Some(problem) = event_problem {
        return HookEdit::Refused(problem);
    }

    let mut refs = Vec::new();
    if let Some(hooks) = &hooks {
        refs = hook_refs_of(hooks);
    }
    let mut missing = Vec::new();
    for entry in entries {
        let command = command_for(entry, hooks_directory);
        let is_present = refs
            .iter()
            .any(|r| r.event == entry.event && r.command == command);
        if !is_present {
            missing.push((entry, command));
        }
    }
    if missing.is_empty() {
        return HookEdit::Unchanged;
    }

    let inserted = insert_groups(source, &missing);
    let Some(mut edited) = inserted else {
        return HookEdit::Refused(uneditable_problem(label));
    };
    if is_new_file {
        edited.push('\n');
    }
    HookEdit::Changed(edited)
}

pub fn remove_managed_entries(text: &str, hooks_directory: &Path, label: &str) -> HookEdit {
    let test = |command: &str| is_managed_command(command, hooks_directory);
    remove_matching(text, label, &test)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const LABEL: &str = "settings.json";
    const USER_FILE: &str = r#"{
  // my comment
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "/some/user/script.sh" }]
      }
    ]
  }
}
"#;
    const USER_GROUP: &str = r#"      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "/some/user/script.sh" }]
      }"#;

    fn setup() -> &'static HarnessSetup {
        let harness = taco_agents::harness_of("claude").unwrap();
        harness.setup().unwrap()
    }

    fn hooks_directory() -> PathBuf {
        PathBuf::from("/Users/someone/.taco-shells/hooks")
    }

    fn value_of(text: &str) -> Value {
        parse_value(text).unwrap()
    }

    fn changed_text(edit: HookEdit) -> String {
        match edit {
            HookEdit::Changed(text) => text,
            other => panic!("expected a changed file, got {other:?}"),
        }
    }

    fn added(text: Option<&str>) -> String {
        let directory = hooks_directory();
        let edit = add_entries(text, setup().entries, &directory, LABEL);
        changed_text(edit)
    }

    fn attention_command() -> String {
        let directory = hooks_directory();
        let path = directory.join("taco-shells-attention.sh");
        path.to_string_lossy().into_owned()
    }

    fn commands_under(value: &Value, event: &str) -> Vec<String> {
        let mut commands = Vec::new();
        let event_value = &value["hooks"][event];
        let groups = event_value.as_array();
        for group in groups.into_iter().flatten() {
            let group_hooks = &group["hooks"];
            let hooks = group_hooks.as_array();
            for hook in hooks.into_iter().flatten() {
                let command = hook["command"].as_str().unwrap();
                commands.push(command.to_string());
            }
        }
        commands
    }

    #[test]
    fn adding_keeps_the_users_file() {
        let result = added(Some(USER_FILE));
        assert!(result.contains("  // my comment\n  \"model\": \"opus\",\n"));
        assert!(result.contains(USER_GROUP));

        let value = value_of(&result);
        let pre_tool_use = commands_under(&value, "PreToolUse");
        assert_eq!(pre_tool_use[0], "/some/user/script.sh");
        let directory = hooks_directory();
        for entry in setup().entries {
            let path = directory.join(entry.script);
            let command = path.to_string_lossy().into_owned();
            let event_commands = commands_under(&value, entry.event);
            assert!(event_commands.contains(&command));
        }

        let stop_value = &value["hooks"]["Stop"];
        let stop_groups = stop_value.as_array().unwrap();
        let async_flags: Vec<bool> = stop_groups
            .iter()
            .map(|group| group["hooks"][0].get("async").is_some())
            .collect();
        assert_eq!(async_flags, vec![false, true]);
        let second_hook = &stop_groups[1]["hooks"][0];
        assert_eq!(second_hook["async"], true);

        let again = add_entries(Some(&result), setup().entries, &directory, LABEL);
        assert_eq!(again, HookEdit::Unchanged);

        let attention = attention_command();
        let notification_only = format!(
            r#"{{"hooks": {{"Notification": [
                {{"hooks": [{{"type": "command", "command": "{attention}"}}]}}
            ]}}}}"#
        );
        let filled_in = added(Some(&notification_only));
        let filled_value = value_of(&filled_in);
        let notification = commands_under(&filled_value, "Notification");
        let stop = commands_under(&filled_value, "Stop");
        let notification_count = notification.iter().filter(|c| **c == attention).count();
        assert_eq!(notification_count, 1);
        assert!(stop.contains(&attention));
    }

    #[test]
    fn adding_matches_tabs_a_missing_file_and_a_space_in_the_path() {
        let tabbed = added(Some("{\n\t\"model\": \"opus\"\n}\n"));
        assert!(tabbed.contains("\n\t\t\"Notification\": ["));
        assert!(tabbed.lines().all(|line| !line.starts_with(' ')));

        let fresh = added(None);
        assert!(fresh.ends_with("}\n"));
        assert!(!fresh.ends_with("\n\n"));

        let spaced = PathBuf::from("/Users/some one/.taco-shells/hooks");
        let edit = add_entries(None, setup().entries, &spaced, LABEL);
        let spaced_text = changed_text(edit);
        let value = value_of(&spaced_text);
        let commands = commands_under(&value, "SessionStart");
        let expected = "\"/Users/some one/.taco-shells/hooks/taco-shells-claude-session.sh\"";
        assert_eq!(commands, vec![expected]);
    }

    #[test]
    fn removal_takes_only_what_it_owns_and_leaves_nothing_empty() {
        let result = added(Some(USER_FILE));
        let directory = hooks_directory();
        let removed = remove_managed_entries(&result, &directory, LABEL);
        let removed_text = changed_text(removed);
        let value = value_of(&removed_text);
        let expected_text = r#"{"model": "opus", "hooks": {"PreToolUse": [
                {"matcher": "Bash", "hooks": [{"type": "command", "command": "/some/user/script.sh"}]}
            ]}}"#;
        let expected: Value = serde_json::from_str(expected_text).unwrap();
        assert_eq!(value, expected);

        let only_ours = added(None);
        let emptied = remove_managed_entries(&only_ours, &directory, LABEL);
        let emptied_text = changed_text(emptied);
        let emptied_value = value_of(&emptied_text);
        assert_eq!(emptied_value, serde_json::json!({}));

        let attention = attention_command();
        let shared = format!(
            r#"{{"hooks": {{"Stop": [{{"hooks": [
                {{"type": "command", "command": "/some/user/script.sh"}},
                {{"type": "command", "command": "{attention}"}}
            ]}}]}}}}"#
        );
        let unshared = remove_managed_entries(&shared, &directory, LABEL);
        let unshared_text = changed_text(unshared);
        let unshared_value = value_of(&unshared_text);
        let user_hook = r#"{"type": "command", "command": "/some/user/script.sh"}"#;
        let expected_group: Value = serde_json::from_str(user_hook).unwrap();
        let stop_value = &unshared_value["hooks"]["Stop"];
        let stop_groups = stop_value.as_array().unwrap();
        assert_eq!(stop_groups.len(), 1);
        let group_hooks = stop_groups[0]["hooks"].as_array().unwrap();
        assert_eq!(group_hooks, &vec![expected_group]);
        assert!(unshared_text.contains(user_hook));

        let spaced = PathBuf::from("/Users/some one/.taco-shells/hooks");
        let spaced_added = add_entries(None, setup().entries, &spaced, LABEL);
        let spaced_text = changed_text(spaced_added);
        let wiring = hook_wiring_of(Some(&spaced_text), setup(), &spaced);
        assert_eq!(wiring, HookWiring::Managed);
        let spaced_removed = remove_managed_entries(&spaced_text, &spaced, LABEL);
        let spaced_result = changed_text(spaced_removed);
        let after = hook_wiring_of(Some(&spaced_result), setup(), &spaced);
        assert_eq!(after, HookWiring::None);
        assert!(!spaced_result.contains("taco-shells-"));
    }

    #[test]
    fn a_file_that_cannot_be_edited_safely_is_refused() {
        let entries = setup().entries;
        let directory = hooks_directory();
        let cases = ["not json", r#"{"hooks": []}"#, r#"{"hooks": {"Stop": {}}}"#];
        for text in cases {
            let edit = add_entries(Some(text), entries, &directory, LABEL);
            let HookEdit::Refused(problem) = edit else {
                panic!("expected a refusal for {text}, got {edit:?}");
            };
            assert!(problem.contains(LABEL), "{problem}");
        }
    }

    #[test]
    fn wiring_says_who_set_the_hooks_up() {
        let directory = hooks_directory();
        let missing_wiring = hook_wiring_of(None, setup(), &directory);
        assert_eq!(missing_wiring, HookWiring::None);

        let managed = added(None);
        let managed_wiring = hook_wiring_of(Some(&managed), setup(), &directory);
        assert_eq!(managed_wiring, HookWiring::Managed);

        let by_hand = r#"{"hooks": {"Stop": [
            {"hooks": [{"type": "command", "command": "~/bin/taco-shells-attention.sh"}]}
        ]}}"#;
        let both_edit = add_entries(Some(by_hand), setup().entries, &directory, LABEL);
        let both = changed_text(both_edit);
        let both_wiring = hook_wiring_of(Some(&both), setup(), &directory);
        assert_eq!(both_wiring, HookWiring::Managed);
        let by_hand_wiring = hook_wiring_of(Some(by_hand), setup(), &directory);
        assert_eq!(by_hand_wiring, HookWiring::ByHand);
    }

    #[test]
    fn a_removal_that_would_hit_another_hook_is_refused() {
        let directory = hooks_directory();
        let ours = directory.join("taco-shells-attention.sh");
        let ours_text = ours.display().to_string();
        let two_hooks_keys = format!(
            r#"{{
  "hooks": {{"Stop": [{{"hooks": [{{"type": "command", "command": "/some/user/script.sh"}}]}}]}},
  "hooks": {{"Stop": [{{"hooks": [{{"type": "command", "command": "{ours_text}"}}]}}]}}
}}"#
        );

        let removal = remove_managed_entries(&two_hooks_keys, &directory, LABEL);

        let HookEdit::Refused(problem) = removal else {
            panic!("expected a refusal, got {removal:?}");
        };
        let expected = uneditable_problem(LABEL);
        assert_eq!(problem, expected);
    }
}
