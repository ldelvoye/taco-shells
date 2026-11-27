//! Finding a command on the shortcuts page by its name or by any spelling of its shortcut.

use taco_settings::{Chord, file_chord_text, modifier_spelling};

/// Whether the normalized query finds a command by its name or by a chord in the file's spelling.
pub fn is_found(wanted: &str, name: &str, chords: &[Chord]) -> bool {
    if wanted.is_empty() {
        return true;
    }
    let lower_name = name.to_lowercase();
    if lower_name.contains(wanted) {
        return true;
    }
    for chord in chords {
        let spelled = file_chord_text(chord);
        if spelled.contains(wanted) {
            return true;
        }
    }
    false
}

pub fn normalized_query(query: &str) -> String {
    let lower = query.trim().to_lowercase();
    let mut parts = Vec::new();
    for part in lower.split('+') {
        let renamed = modifier_spelling(part);
        match renamed {
            Some(spelled) => parts.push(spelled.to_string()),
            None => parts.push(part.to_string()),
        }
    }
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use taco_settings::parse_chord;

    use super::*;

    fn chord(text: &str) -> Chord {
        parse_chord(text).expect(text)
    }

    fn finds(query: &str, name: &str, chords: &[Chord]) -> bool {
        let wanted = normalized_query(query);
        is_found(&wanted, name, chords)
    }

    #[test]
    fn search_finds_a_command_by_name_or_by_any_spelling_of_its_shortcut() {
        let clear = [chord("cmd+k")];
        let word_left = [chord("opt+left")];
        assert!(finds("", "Clear", &clear));
        assert!(finds("  CLE ", "Clear", &clear));
        assert!(finds("cmd+k", "Clear", &clear));
        assert!(finds("command+k", "Clear", &clear));
        assert!(finds("alt+left", "Back a word", &word_left));
        assert!(finds("option+left", "Back a word", &word_left));
        assert!(!finds("cmd+j", "Clear", &clear));
    }
}
