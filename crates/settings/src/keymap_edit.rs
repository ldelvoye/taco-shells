use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::commands::CommandId;
use crate::config::DEFAULT_KEYBINDINGS_TEXT;
use crate::jsonc::{TopLevelEdit, edit_top_level, parse_jsonc};
use crate::keys::{Binding, Chord, command_for, parse_chord, resolve_keymap};

#[derive(Clone, Debug, PartialEq)]
pub enum KeymapChange {
    Add {
        command: CommandId,
        chord: Chord,
    },
    Remove {
        command: CommandId,
        chord: Chord,
    },
    Replace {
        command: CommandId,
        from: Chord,
        to: Chord,
    },
    Reset {
        command: CommandId,
    },
}

impl KeymapChange {
    pub fn command(&self) -> CommandId {
        match self {
            KeymapChange::Add { command, .. } => *command,
            KeymapChange::Remove { command, .. } => *command,
            KeymapChange::Replace { command, .. } => *command,
            KeymapChange::Reset { command } => *command,
        }
    }

    fn carried_chords(&self) -> Vec<&Chord> {
        match self {
            KeymapChange::Add { chord, .. } => vec![chord],
            KeymapChange::Remove { chord, .. } => vec![chord],
            KeymapChange::Replace { from, to, .. } => vec![from, to],
            KeymapChange::Reset { .. } => Vec::new(),
        }
    }

    fn held_chord(&self) -> Option<&Chord> {
        match self {
            KeymapChange::Remove { chord, .. } => Some(chord),
            KeymapChange::Replace { from, .. } => Some(from),
            KeymapChange::Add { .. } | KeymapChange::Reset { .. } => None,
        }
    }
}

fn file_chord_text(chord: &Chord) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if chord.cmd {
        parts.push("cmd");
    }
    if chord.ctrl {
        parts.push("ctrl");
    }
    if chord.alt {
        parts.push("opt");
    }
    if chord.shift {
        parts.push("shift");
    }
    parts.push(&chord.key);
    parts.join("+")
}

fn same_chord(line_key: &str, edit_key: &str) -> bool {
    let line_chord = parse_chord(line_key);
    let edit_chord = parse_chord(edit_key);
    match (line_chord, edit_chord) {
        (Some(line), Some(edit)) => line == edit,
        _ => false,
    }
}

fn read_user(text: Option<&str>, label: &str) -> Result<Option<Map<String, Value>>, String> {
    let Some(source) = text else {
        return Ok(None);
    };
    if source.trim().is_empty() {
        return Ok(None);
    }
    let parsed = parse_jsonc(source, label);
    if let Some(map) = parsed.value {
        return Ok(Some(map));
    }
    match parsed.problems.first() {
        Some(problem) => Err(problem.clone()),
        None => Err(format!("{label}: the file could not be read")),
    }
}

fn give_edit(command: CommandId, chord: &Chord, default_bindings: &[Binding]) -> TopLevelEdit {
    let shown = file_chord_text(chord);
    let default_command = command_for(default_bindings, chord);
    if default_command == Some(command) {
        return TopLevelEdit::Remove(shown);
    }
    TopLevelEdit::Set(shown, Value::String(command.id().to_string()))
}

fn take_edit(chord: &Chord, default_bindings: &[Binding]) -> TopLevelEdit {
    let shown = file_chord_text(chord);
    let default_command = command_for(default_bindings, chord);
    // Nulled when the defaults bind the chord, since removing the line would hand it back to another command.
    if default_command.is_some() {
        TopLevelEdit::Set(shown, Value::Null)
    } else {
        TopLevelEdit::Remove(shown)
    }
}

fn reset_edits(
    command: CommandId,
    user: Option<&Map<String, Value>>,
    default_bindings: &[Binding],
) -> Vec<TopLevelEdit> {
    let mut keys: Vec<String> = Vec::new();

    if let Some(user_layer) = user {
        for (line_key, value) in user_layer {
            let Some(chord) = parse_chord(line_key) else {
                continue;
            };
            if value.as_str() == Some(command.id()) {
                keys.push(file_chord_text(&chord));
            }
        }
    }

    for binding in default_bindings {
        if binding.command == command {
            keys.push(file_chord_text(&binding.chord));
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut edits: Vec<TopLevelEdit> = Vec::new();
    for key in keys {
        if seen.insert(key.clone()) {
            edits.push(TopLevelEdit::Remove(key));
        }
    }
    edits
}

fn chords_of(bindings: &[Binding], command: CommandId) -> HashSet<Chord> {
    let mut chords: HashSet<Chord> = HashSet::new();
    for binding in bindings {
        if binding.command == command {
            chords.insert(binding.chord.clone());
        }
    }
    chords
}

fn came_out_as_asked(
    change: &KeymapChange,
    bindings: &[Binding],
    default_bindings: &[Binding],
) -> bool {
    let command = change.command();
    match change {
        KeymapChange::Add { chord, .. } => command_for(bindings, chord) == Some(command),
        KeymapChange::Remove { chord, .. } => command_for(bindings, chord) != Some(command),
        KeymapChange::Replace { from, to, .. } => {
            let from_gone = command_for(bindings, from) != Some(command);
            let to_held = command_for(bindings, to) == Some(command);
            from_gone && to_held
        }
        KeymapChange::Reset { .. } => {
            let now = chords_of(bindings, command);
            let shipped = chords_of(default_bindings, command);
            now == shipped
        }
    }
}

pub fn edit_keymap(
    text: Option<&str>,
    change: &KeymapChange,
    label: &str,
) -> Result<String, String> {
    let command = change.command();

    for chord in change.carried_chords() {
        let shown = file_chord_text(chord);
        let reread = parse_chord(&shown);
        if reread.as_ref() != Some(chord) {
            return Err(format!("{label}: \"{shown}\" is not a valid chord"));
        }
    }

    let default_parsed = parse_jsonc(DEFAULT_KEYBINDINGS_TEXT, label);
    let defaults = match default_parsed.value {
        Some(map) => map,
        None => Map::new(),
    };
    let default_bindings = resolve_keymap(&defaults, None, label).bindings;

    let user = read_user(text, label)?;
    let current = resolve_keymap(&defaults, user.as_ref(), label);
    let problems_before = current.problems.len();

    if let Some(held) = change.held_chord() {
        let held_command = command_for(&current.bindings, held);
        if held_command != Some(command) {
            let shown = file_chord_text(held);
            let id = command.id();
            return Err(format!(
                "{label}: \"{shown}\" is no longer a shortcut for {id}"
            ));
        }
    }

    let edits: Vec<TopLevelEdit> = match change {
        KeymapChange::Add { chord, .. } => vec![give_edit(command, chord, &default_bindings)],
        KeymapChange::Remove { chord, .. } => vec![take_edit(chord, &default_bindings)],
        KeymapChange::Replace { from, to, .. } => vec![
            take_edit(from, &default_bindings),
            give_edit(command, to, &default_bindings),
        ],
        KeymapChange::Reset { .. } => reset_edits(command, user.as_ref(), &default_bindings),
    };

    let new_text = edit_top_level(text, &edits, label, &same_chord)?;

    let new_user = read_user(Some(&new_text), label)?;
    let after = resolve_keymap(&defaults, new_user.as_ref(), label);
    if after.problems.len() > problems_before {
        return Err(format!(
            "{label} could not be changed without breaking another line, so nothing was changed."
        ));
    }
    if !came_out_as_asked(change, &after.bindings, &default_bindings) {
        return Err(format!(
            "{label} did not come out as asked, so nothing was changed."
        ));
    }

    Ok(new_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LABEL: &str = "keybindings.json";
    const USER_BODY: &str = "  // my comment\n  \"cmd+d\": \"terminal.split\",\n  \"hyper+x\": \"terminal.new\",\n  // my clear key\n  \"cmd+j\": \"terminal.clear\"";

    fn file(body: &str) -> String {
        format!("{{\n{body}\n}}\n")
    }

    fn chord(text: &str) -> Chord {
        parse_chord(text).expect(text)
    }

    fn add(command: CommandId, text: &str) -> KeymapChange {
        KeymapChange::Add {
            command,
            chord: chord(text),
        }
    }

    fn remove(command: CommandId, text: &str) -> KeymapChange {
        KeymapChange::Remove {
            command,
            chord: chord(text),
        }
    }

    fn edited(text: &str, change: &KeymapChange) -> Result<String, String> {
        edit_keymap(Some(text), change, LABEL)
    }

    fn chords_after(text: &str, command: CommandId) -> HashSet<Chord> {
        let parsed = parse_jsonc(text, LABEL);
        let defaults = parse_jsonc(DEFAULT_KEYBINDINGS_TEXT, LABEL);
        let defaults = defaults.value.expect("shipped keybindings parse");
        let resolved = resolve_keymap(&defaults, parsed.value.as_ref(), LABEL);
        chords_of(&resolved.bindings, command)
    }

    #[test]
    fn changes_touch_only_their_own_lines() {
        let clear = CommandId::TerminalClear;
        let user = file(USER_BODY);

        let added = edited(&user, &add(clear, "cmd+u"));
        let with_u = file(&format!("{USER_BODY},\n  \"cmd+u\": \"terminal.clear\""));
        assert_eq!(added, Ok(with_u));

        let nulled = edited(&user, &remove(clear, "cmd+k"));
        let with_null = file(&format!("{USER_BODY},\n  \"cmd+k\": null"));
        assert_eq!(nulled, Ok(with_null));

        let dropped = edited(&user, &remove(clear, "cmd+j"));
        let without_j = file(
            "  // my comment\n  \"cmd+d\": \"terminal.split\",\n  \"hyper+x\": \"terminal.new\"\n  // my clear key",
        );
        assert_eq!(dropped, Ok(without_j));

        let replace = KeymapChange::Replace {
            command: clear,
            from: chord("cmd+k"),
            to: chord("cmd+u"),
        };
        let replaced = edited(&user, &replace);
        let both = file(&format!(
            "{USER_BODY},\n  \"cmd+k\": null,\n  \"cmd+u\": \"terminal.clear\""
        ));
        assert_eq!(replaced, Ok(both));

        let option = edit_keymap(None, &add(clear, "opt+up"), LABEL);
        assert_eq!(
            option,
            Ok("{\n  \"opt+up\": \"terminal.clear\"\n}\n".to_string())
        );
    }

    #[test]
    fn defaults_and_spellings_decide_between_nulling_and_removing() {
        let clear = CommandId::TerminalClear;

        let held_by_other = "{\n  \"cmd+b\": \"terminal.clear\"\n}\n";
        let nulled = edited(held_by_other, &remove(clear, "cmd+b"));
        assert_eq!(nulled, Ok("{\n  \"cmd+b\": null\n}\n".to_string()));

        let rebound = "{\n  \"cmd+k\": \"terminal.new\"\n}\n";
        let handed_back = edited(rebound, &add(clear, "cmd+k"));
        assert_eq!(handed_back, Ok("{}\n".to_string()));

        let spelled = "{\n  \"Cmd+U\": null\n}\n";
        let in_place = edited(spelled, &add(clear, "cmd+u"));
        assert_eq!(
            in_place,
            Ok("{\n  \"Cmd+U\": \"terminal.clear\"\n}\n".to_string())
        );

        let reset_file = "{\n  // keep one\n  \"cmd+k\": \"terminal.new\",\n  // keep two\n  \"cmd+j\": \"terminal.clear\"\n}\n";
        let reset = KeymapChange::Reset { command: clear };
        let text = edited(reset_file, &reset).expect("reset is written");
        assert_eq!(text, "{\n  // keep one\n  // keep two\n}\n");
        let expected: HashSet<Chord> = HashSet::from([chord("cmd+k")]);
        assert_eq!(chords_after(&text, clear), expected);
    }

    #[test]
    fn unsafe_changes_are_refused_with_the_reason() {
        let clear = CommandId::TerminalClear;
        let user = file(USER_BODY);

        let bad_chord = Chord {
            cmd: true,
            ctrl: false,
            alt: false,
            shift: false,
            key: "notakey".to_string(),
        };
        let invalid = KeymapChange::Add {
            command: clear,
            chord: bad_chord,
        };
        assert_eq!(
            edited(&user, &invalid),
            Err("keybindings.json: \"cmd+notakey\" is not a valid chord".to_string())
        );

        let stale = edited(&user, &remove(clear, "cmd+u"));
        assert_eq!(
            stale,
            Err(
                "keybindings.json: \"cmd+u\" is no longer a shortcut for terminal.clear"
                    .to_string()
            )
        );

        let twice = "{\n  \"cmd+u\": null,\n  \"Cmd+U\": null\n}\n";
        let duplicate = edited(twice, &add(clear, "cmd+u"));
        assert_eq!(
            duplicate,
            Err("keybindings.json has \"cmd+u\" twice".to_string())
        );

        let broken = edited("{ broken", &add(clear, "cmd+u"));
        let problem = broken.expect_err("an unreadable file is refused");
        assert!(problem.starts_with("keybindings.json: "), "{problem}");
    }
}
