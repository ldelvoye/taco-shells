use std::collections::BTreeMap;

use serde_json::{Map, Value};

use crate::commands::{CommandId, is_retired};

const NAMED_KEYS: [&str; 26] = [
    "space",
    "enter",
    "tab",
    "escape",
    "backspace",
    "delete",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Modifier {
    Cmd,
    Ctrl,
    Alt,
    Shift,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub chord: Chord,
    pub command: CommandId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeymapResolution {
    pub bindings: Vec<Binding>,
    pub problems: Vec<String>,
}

fn modifier_for(alias: &str) -> Option<Modifier> {
    match alias {
        "cmd" | "command" | "meta" => Some(Modifier::Cmd),
        "ctrl" | "control" => Some(Modifier::Ctrl),
        "alt" | "option" | "opt" => Some(Modifier::Alt),
        "shift" => Some(Modifier::Shift),
        _ => None,
    }
}

fn modifier_slot(chord: &mut Chord, modifier: Modifier) -> &mut bool {
    match modifier {
        Modifier::Cmd => &mut chord.cmd,
        Modifier::Ctrl => &mut chord.ctrl,
        Modifier::Alt => &mut chord.alt,
        Modifier::Shift => &mut chord.shift,
    }
}

pub fn parse_chord(input: &str) -> Option<Chord> {
    let segments: Vec<&str> = input.split('+').collect();
    if segments.contains(&"") {
        return None;
    }

    let last_index = segments.len() - 1;
    let modifier_segments = &segments[..last_index];
    let base_segment = segments[last_index];

    let mut chord = Chord {
        cmd: false,
        ctrl: false,
        alt: false,
        shift: false,
        key: String::new(),
    };

    for segment in modifier_segments {
        let alias = segment.to_lowercase();
        let modifier = modifier_for(&alias)?;
        let slot = modifier_slot(&mut chord, modifier);
        if *slot {
            return None;
        }
        *slot = true;
    }

    let base_key = base_segment.to_lowercase();
    if modifier_for(&base_key).is_some() {
        return None;
    }

    let is_multi_char = base_key.chars().count() > 1;
    if is_multi_char && !NAMED_KEYS.contains(&base_key.as_str()) {
        return None;
    }

    chord.key = base_key;
    Some(chord)
}

pub fn format_chord(chord: &Chord) -> String {
    chord_text(chord, "alt")
}

/// A chord as the file spells it, with "opt" for the option key: "cmd+opt+left".
pub fn file_chord_text(chord: &Chord) -> String {
    chord_text(chord, "opt")
}

fn spelling_of(modifier: Modifier) -> &'static str {
    match modifier {
        Modifier::Cmd => "cmd",
        Modifier::Ctrl => "ctrl",
        Modifier::Alt => "opt",
        Modifier::Shift => "shift",
    }
}

/// The file's spelling of a modifier word: "command" and "meta" give "cmd", "option" and "alt" give "opt".
pub fn modifier_spelling(word: &str) -> Option<&'static str> {
    let modifier = modifier_for(word)?;
    let spelling = spelling_of(modifier);
    Some(spelling)
}

fn chord_text(chord: &Chord, option_word: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if chord.cmd {
        parts.push(spelling_of(Modifier::Cmd));
    }
    if chord.ctrl {
        parts.push(spelling_of(Modifier::Ctrl));
    }
    if chord.alt {
        parts.push(option_word);
    }
    if chord.shift {
        parts.push(spelling_of(Modifier::Shift));
    }
    parts.push(&chord.key);
    parts.join("+")
}

fn command_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn apply_keymap_layer(
    layer: &Map<String, Value>,
    label: &str,
    bindings: &mut BTreeMap<String, Binding>,
    problems: &mut Vec<String>,
) {
    for (chord_text, value) in layer {
        let Some(chord) = parse_chord(chord_text) else {
            problems.push(format!("{label}: \"{chord_text}\" is not a valid chord"));
            continue;
        };
        let canonical = format_chord(&chord);

        if value.is_null() {
            bindings.remove(&canonical);
            continue;
        }

        let known_command = match value.as_str() {
            Some(id) => CommandId::from_id(id),
            None => None,
        };
        if let Some(command) = known_command {
            bindings.insert(canonical, Binding { chord, command });
            continue;
        }

        let is_retired_id = match value.as_str() {
            Some(id) => is_retired(id),
            None => false,
        };
        if is_retired_id {
            bindings.remove(&canonical);
            continue;
        }

        let shown = command_text(value);
        problems.push(format!(
            "{label}: \"{chord_text}\": \"{shown}\" is not a known command id"
        ));
    }
}

pub fn resolve_keymap(
    defaults: &Map<String, Value>,
    user: Option<&Map<String, Value>>,
    label: &str,
) -> KeymapResolution {
    let mut bindings: BTreeMap<String, Binding> = BTreeMap::new();
    let mut problems = Vec::new();

    apply_keymap_layer(defaults, label, &mut bindings, &mut problems);
    if let Some(user_layer) = user {
        apply_keymap_layer(user_layer, label, &mut bindings, &mut problems);
    }

    let resolved: Vec<Binding> = bindings.into_values().collect();
    KeymapResolution {
        bindings: resolved,
        problems,
    }
}

pub fn command_for(bindings: &[Binding], chord: &Chord) -> Option<CommandId> {
    for binding in bindings {
        if binding.chord == *chord {
            return Some(binding.command);
        }
    }
    None
}

/// The command's chords, in the order the keymap holds them.
pub fn chords_for(bindings: &[Binding], command: CommandId) -> Vec<Chord> {
    let mut chords = Vec::new();
    for binding in bindings {
        if binding.command == command {
            chords.push(binding.chord.clone());
        }
    }
    chords
}

/// Whether a command's chords are not its defaults, in any order.
pub fn differs_from_defaults(chords: &[Chord], defaults: &[Chord]) -> bool {
    let same_count = chords.len() == defaults.len();
    let all_default = chords.iter().all(|chord| defaults.contains(chord));
    !(same_count && all_default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chord(text: &str) -> Chord {
        parse_chord(text).expect(text)
    }

    #[test]
    fn back_to_default_shows_only_when_the_chords_are_not_the_defaults() {
        let both = [chord("cmd+t"), chord("cmd+n")];
        let reversed = [chord("cmd+n"), chord("cmd+t")];
        let one = [chord("cmd+t")];
        assert!(!differs_from_defaults(&reversed, &both));
        assert!(differs_from_defaults(&one, &both));
        assert!(differs_from_defaults(&both, &one));
    }

    fn map_from(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    fn command_at(resolution: &KeymapResolution, chord_text: &str) -> Option<CommandId> {
        let chord = parse_chord(chord_text).expect(chord_text);
        command_for(&resolution.bindings, &chord)
    }

    #[test]
    fn spellings_canonicalise_and_malformed_chords_are_refused() {
        let spellings = ["Command+Option+K", "alt+cmd+k", "meta+opt+k"];
        for spelling in spellings {
            let chord = parse_chord(spelling).expect(spelling);
            assert_eq!(format_chord(&chord), "cmd+alt+k", "{spelling}");
        }

        let malformed = ["", "cmd+", "cmd+cmd+k", "shift", "cmd+foo", "+"];
        for text in malformed {
            assert_eq!(parse_chord(text), None, "{text:?}");
        }
    }

    #[test]
    fn user_layer_overrides_unbinds_and_retires_without_losing_other_bindings() {
        let defaults = map_from(json!({
            "cmd+k": "terminal.clear",
            "cmd+b": "sidebar.toggle",
            "cmd+\\": "terminal.split",
            "cmd+w": "terminal.close",
            "cmd+n": "terminal.new"
        }));
        let user = map_from(json!({
            "Command+K": "terminal.copy",
            "cmd+b": null,
            "cmd+w": "terminal.focusNextPane",
            "cmd+opt+1": "terminal.colorRed",
            "cmd+t": "terminal.teleport",
            "cmd+shift+7": 7,
            "cmd+cmd+x": "terminal.new"
        }));

        let resolution = resolve_keymap(&defaults, Some(&user), "keybindings.json");

        assert_eq!(
            command_at(&resolution, "cmd+k"),
            Some(CommandId::TerminalCopy)
        );
        assert_eq!(command_at(&resolution, "cmd+b"), None);
        assert_eq!(command_at(&resolution, "cmd+\\"), None);
        assert_eq!(command_at(&resolution, "cmd+w"), None);
        assert_eq!(command_at(&resolution, "cmd+opt+1"), None);
        assert_eq!(
            command_at(&resolution, "cmd+n"),
            Some(CommandId::TerminalNew)
        );
        let expected = vec![
            "keybindings.json: \"cmd+t\": \"terminal.teleport\" is not a known command id",
            "keybindings.json: \"cmd+shift+7\": \"7\" is not a known command id",
            "keybindings.json: \"cmd+cmd+x\" is not a valid chord",
        ];
        assert_eq!(resolution.problems, expected);
    }
}
