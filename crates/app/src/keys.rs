//! Key-downs turned into the chords `keybindings.json` names.

use gpui::Keystroke;
use taco_settings::Chord;

// macOS reports cmd+shift+] as cmd+} with shift off; the bindings name the key under the finger, with shift.
const SHIFTED_KEYS: [(&str, &str); 21] = [
    ("}", "]"),
    ("{", "["),
    ("<", ","),
    (">", "."),
    ("?", "/"),
    (":", ";"),
    ("\"", "'"),
    ("|", "\\"),
    ("_", "-"),
    ("+", "="),
    ("~", "`"),
    ("!", "1"),
    ("@", "2"),
    ("#", "3"),
    ("$", "4"),
    ("%", "5"),
    ("^", "6"),
    ("&", "7"),
    ("*", "8"),
    ("(", "9"),
    (")", "0"),
];

/// The chord a key-down is, as a binding would write it.
pub fn chord_of(keystroke: &Keystroke) -> Chord {
    let modifiers = keystroke.modifiers;
    let typed = keystroke.key.to_lowercase();
    let mut shift = modifiers.shift;
    let mut key = typed.clone();
    let mut pairs = SHIFTED_KEYS.iter();
    if let Some((_, base)) = pairs.find(|(shifted, _)| *shifted == typed) {
        key = base.to_string();
        shift = true;
    }
    Chord {
        cmd: modifiers.platform,
        ctrl: modifiers.control,
        alt: modifiers.alt,
        shift,
        key,
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Keystroke, Modifiers};
    use taco_settings::{Chord, parse_chord};

    use super::chord_of;

    fn pressed(key: &str, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            modifiers,
            key: key.to_string(),
            key_char: None,
        }
    }

    #[test]
    fn a_key_down_is_the_chord_its_binding_names() {
        let command = Modifiers::command();
        let command_shift = Modifiers::command_shift();
        let cases: [(Keystroke, &str); 5] = [
            (pressed("t", command), "cmd+t"),
            (pressed("}", command), "cmd+shift+]"),
            (pressed("<", command), "cmd+shift+,"),
            (pressed("s", command_shift), "cmd+shift+s"),
            (pressed("left", Modifiers::alt()), "alt+left"),
        ];
        for (keystroke, written) in cases {
            let chord: Chord = chord_of(&keystroke);
            let expected = parse_chord(written);
            assert_eq!(Some(chord), expected, "{written}");
        }
    }
}
