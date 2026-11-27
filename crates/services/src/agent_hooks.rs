use std::path::Path;

use jsonc_parser::cst::{CstNode, CstRootNode};
use jsonc_parser::parse_to_serde_value;
use serde_json::{Map, Value};
use taco_settings::JSONC_PARSE_OPTIONS;

use crate::settings_edit::{HookEdit, properties_named, uneditable_problem, unreadable_problem};

const HOOKS_KEY: &str = "hooks";
const SETTINGS_LABEL: &str = "settings.json";

struct HookRef {
    event: String,
    group_index: usize,
    hook_index: usize,
    command: String,
    hooks_in_group: usize,
    groups_in_event: usize,
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

fn is_managed_command(command: &str, hooks_directory: &Path) -> bool {
    let directory = hooks_directory.to_string_lossy();
    let prefix = format!("{directory}/");
    let quoted_prefix = format!("\"{prefix}");
    command.starts_with(&prefix) || command.starts_with(&quoted_prefix)
}

fn command_of(hook_node: &CstNode) -> Option<String> {
    let hook = hook_node.as_object()?;
    let command_prop = hook.get("command")?;
    let command_value = command_prop.value()?;
    let command_text = command_value.as_string_lit()?;
    command_text.decoded_value().ok()
}

// The narrowest removal that leaves no empty group, event list or "hooks" object behind.
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

pub(crate) fn has_managed_entries(text: Option<&str>, hooks_directory: &Path) -> bool {
    let Some(text) = text else {
        return false;
    };
    let read = read_hooks(text, SETTINGS_LABEL);
    let Ok(Some(hooks)) = read else {
        return false;
    };
    let refs = hook_refs_of(&hooks);
    refs.iter()
        .any(|r| is_managed_command(&r.command, hooks_directory))
}

pub fn remove_managed_entries(text: &str, hooks_directory: &Path, label: &str) -> HookEdit {
    let test = |command: &str| is_managed_command(command, hooks_directory);
    remove_matching(text, label, &test)
}

#[cfg(test)]
pub(crate) mod electron_fixture {
    use std::path::Path;

    use serde_json::{Map, Value, json};

    const ATTENTION_SCRIPT: &str = "taco-shells-attention.sh";
    const SESSION_SCRIPT: &str = "taco-shells-claude-session.sh";
    const ELECTRON_HOOKS: [(&str, &str, bool); 10] = [
        ("Notification", ATTENTION_SCRIPT, false),
        ("Notification", SESSION_SCRIPT, true),
        ("Stop", ATTENTION_SCRIPT, false),
        ("Stop", SESSION_SCRIPT, true),
        ("SessionStart", SESSION_SCRIPT, false),
        ("SessionEnd", SESSION_SCRIPT, false),
        ("UserPromptSubmit", SESSION_SCRIPT, true),
        ("PreToolUse", SESSION_SCRIPT, true),
        ("PostToolUse", SESSION_SCRIPT, true),
        ("StopFailure", SESSION_SCRIPT, true),
    ];

    fn command_line(hooks_directory: &Path, script: &str) -> String {
        let path = hooks_directory.join(script);
        let text = path.to_string_lossy().into_owned();
        if text.contains(' ') {
            return format!("\"{text}\"");
        }
        text
    }

    fn group_of(command: String, is_async: bool) -> Value {
        let mut hook = Map::new();
        hook.insert("type".to_string(), json!("command"));
        hook.insert("command".to_string(), json!(command));
        if is_async {
            hook.insert("async".to_string(), json!(true));
        }
        json!({ "hooks": [hook] })
    }

    pub(crate) fn electron_hooks(hooks_directory: &Path, user_hooks: &str) -> String {
        let parsed: Value = serde_json::from_str(user_hooks).unwrap();
        let Value::Object(mut hooks) = parsed else {
            panic!("the user's hooks must be an object");
        };
        for (event, script, is_async) in ELECTRON_HOOKS {
            let command = command_line(hooks_directory, script);
            let group = group_of(command, is_async);
            match hooks.get_mut(event) {
                Some(Value::Array(groups)) => groups.push(group),
                Some(_) => panic!("the user's {event} must be a list"),
                None => {
                    hooks.insert(event.to_string(), Value::Array(vec![group]));
                }
            }
        }
        serde_json::to_string(&hooks).unwrap()
    }

    pub(crate) fn electron_settings(hooks_directory: &Path, user_hooks: &str) -> String {
        let hooks_text = electron_hooks(hooks_directory, user_hooks);
        let hooks: Value = serde_json::from_str(&hooks_text).unwrap();
        let settings = json!({ "hooks": hooks });
        let mut text = serde_json::to_string_pretty(&settings).unwrap();
        text.push('\n');
        text
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::electron_fixture::electron_settings;
    use super::*;

    const LABEL: &str = "settings.json";
    const USER_KEYS: &str = "{\n  // my comment\n  \"model\": \"opus\",\n";
    const USER_HOOKS: &str = r#"{
  "Stop": [{"hooks": [{"type": "command", "command": "/some/user/script.sh"}]}],
  "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "/some/user/check.sh"}]}]
}"#;

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

    fn attention_command() -> String {
        let directory = hooks_directory();
        let path = directory.join("taco-shells-attention.sh");
        path.to_string_lossy().into_owned()
    }

    fn with_user_keys(settings: &str) -> String {
        settings.replacen("{\n", USER_KEYS, 1)
    }

    #[test]
    fn an_electron_written_file_is_detected_and_taken_out_leaving_the_users_own() {
        let missing_file = has_managed_entries(None, &hooks_directory());
        assert!(!missing_file);
        let plain = hooks_directory();
        let spaced = PathBuf::from("/Users/some one/.taco-shells/hooks");
        let user_hooks: Value = serde_json::from_str(USER_HOOKS).unwrap();
        let own_settings = json!({ "hooks": user_hooks });
        let own_pretty = serde_json::to_string_pretty(&own_settings).unwrap();
        let own_text = format!("{own_pretty}\n");
        let users_own = with_user_keys(&own_text);

        for directory in [plain, spaced] {
            let written = electron_settings(&directory, USER_HOOKS);
            let electron = with_user_keys(&written);
            assert!(
                has_managed_entries(Some(&electron), &directory),
                "{directory:?}"
            );

            let removal = remove_managed_entries(&electron, &directory, LABEL);

            let removed = changed_text(removal);
            assert_eq!(removed, users_own, "{directory:?}");
            assert!(!has_managed_entries(Some(&removed), &directory));
        }
    }

    #[test]
    fn removal_takes_only_what_it_owns_and_leaves_nothing_empty() {
        let directory = hooks_directory();
        let only_ours = electron_settings(&directory, "{}");
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

        for text in ["not json", r#"{"hooks": []}"#] {
            let edit = remove_managed_entries(text, &directory, LABEL);
            let HookEdit::Refused(problem) = edit else {
                panic!("expected a refusal for {text}, got {edit:?}");
            };
            assert!(problem.contains(LABEL), "{problem}");
        }
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
