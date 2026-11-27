//! Setting a shortcut by pressing keys on its row: what a pressed chord does, and when the capture closes.

use taco_settings::{Binding, Chord, CommandId, KeymapChange, chords_for, command_for};

// Keys that type no character.
const NON_CHARACTER_KEYS: [&str; 12] = [
    "enter",
    "tab",
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
];

/// A shortcut being set on a row, from the click on a chip or + until it is written or cancelled.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub command: CommandId,
    /// The chord being changed; none when one is being added.
    pub changing: Option<Chord>,
    /// The last keys pressed, kept while the row asks about them.
    pub pending: Option<Chord>,
    pub problem: Option<CaptureProblem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureProblem {
    NeedsModifier,
    /// Another command has the chord; "Use it here" takes it.
    Clash(CommandId),
}

/// What keys pressed while a row is capturing come to.
#[derive(Debug, PartialEq)]
pub enum Pressed {
    Write(KeymapChange),
    /// The command already has the chord, so there is nothing to write.
    Unchanged,
    Ask(CaptureProblem),
}

pub fn pressed(capture: &Capture, chord: Chord, bindings: &[Binding]) -> Pressed {
    if !is_allowed(&chord) {
        return Pressed::Ask(CaptureProblem::NeedsModifier);
    }
    let owner = command_for(bindings, &chord);
    if owner == Some(capture.command) {
        return Pressed::Unchanged;
    }
    if let Some(other) = owner {
        return Pressed::Ask(CaptureProblem::Clash(other));
    }
    let change = change_for(capture, chord);
    Pressed::Write(change)
}

/// The change that puts a chord on the capturing row: in place of the chip being changed, or beside the others.
pub fn change_for(capture: &Capture, to: Chord) -> KeymapChange {
    let command = capture.command;
    match &capture.changing {
        Some(from) => KeymapChange::Replace {
            command,
            from: from.clone(),
            to,
        },
        None => KeymapChange::Add { command, chord: to },
    }
}

/// A shortcut needs cmd, ctrl or opt; shift alone only with a key that types no character.
fn is_allowed(chord: &Chord) -> bool {
    if chord.cmd || chord.ctrl || chord.alt {
        return true;
    }
    chord.shift && NON_CHARACTER_KEYS.contains(&chord.key.as_str())
}

/// Whether a capture still has something to set: the file can be changed, and a chip being changed is still there.
pub fn capture_still_open(capture: &Capture, bindings: &[Binding], unread: bool) -> bool {
    if unread {
        return false;
    }
    let Some(changing) = &capture.changing else {
        return true;
    };
    let chords = chords_for(bindings, capture.command);
    chords.contains(changing)
}

#[cfg(test)]
mod tests {
    use taco_settings::parse_chord;

    use super::*;

    fn chord(text: &str) -> Chord {
        parse_chord(text).expect(text)
    }

    fn bound(text: &str, command: CommandId) -> Binding {
        Binding {
            chord: chord(text),
            command,
        }
    }

    #[test]
    fn keys_pressed_on_a_row_are_written_asked_about_or_left() {
        let bindings = [
            bound("cmd+t", CommandId::TerminalNew),
            bound("cmd+k", CommandId::TerminalClear),
        ];
        let adding = Capture {
            command: CommandId::TerminalNew,
            changing: None,
            pending: None,
            problem: None,
        };
        let changing = Capture {
            changing: Some(chord("cmd+t")),
            ..adding.clone()
        };
        let cases = [
            (
                &adding,
                "cmd+j",
                Pressed::Write(KeymapChange::Add {
                    command: CommandId::TerminalNew,
                    chord: chord("cmd+j"),
                }),
            ),
            (
                &changing,
                "cmd+j",
                Pressed::Write(KeymapChange::Replace {
                    command: CommandId::TerminalNew,
                    from: chord("cmd+t"),
                    to: chord("cmd+j"),
                }),
            ),
            (
                &adding,
                "cmd+k",
                Pressed::Ask(CaptureProblem::Clash(CommandId::TerminalClear)),
            ),
            (&adding, "cmd+t", Pressed::Unchanged),
            (&adding, "j", Pressed::Ask(CaptureProblem::NeedsModifier)),
            (
                &adding,
                "shift+j",
                Pressed::Ask(CaptureProblem::NeedsModifier),
            ),
            (
                &adding,
                "shift+enter",
                Pressed::Write(KeymapChange::Add {
                    command: CommandId::TerminalNew,
                    chord: chord("shift+enter"),
                }),
            ),
            (
                &adding,
                "shift+left",
                Pressed::Write(KeymapChange::Add {
                    command: CommandId::TerminalNew,
                    chord: chord("shift+left"),
                }),
            ),
            (
                &adding,
                "opt+j",
                Pressed::Write(KeymapChange::Add {
                    command: CommandId::TerminalNew,
                    chord: chord("opt+j"),
                }),
            ),
        ];
        for (capture, keys, expected) in cases {
            let result = pressed(capture, chord(keys), &bindings);
            assert_eq!(result, expected, "{keys}");
        }
    }

    #[test]
    fn a_capture_closes_once_its_chip_is_gone_or_the_file_cannot_be_read() {
        let bindings = [bound("cmd+t", CommandId::TerminalNew)];
        let adding = Capture {
            command: CommandId::TerminalNew,
            changing: None,
            pending: None,
            problem: None,
        };
        let on_its_chip = Capture {
            changing: Some(chord("cmd+t")),
            ..adding.clone()
        };
        let on_a_gone_chip = Capture {
            changing: Some(chord("cmd+n")),
            ..adding.clone()
        };
        assert!(capture_still_open(&adding, &bindings, false));
        assert!(capture_still_open(&on_its_chip, &bindings, false));
        assert!(!capture_still_open(&on_a_gone_chip, &bindings, false));
        assert!(!capture_still_open(&adding, &bindings, true));
    }
}
