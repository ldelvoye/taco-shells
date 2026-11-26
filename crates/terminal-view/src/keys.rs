//! Which keystrokes a view encodes itself. Text typed without Ctrl goes through the input method instead, so dead keys and composed characters work, and Cmd chords belong to the app.

use gpui::Keystroke;
use taco_ghostty::input::{Key, KeyPress, Mods};

/// The key press for `keystroke`, or none when the view leaves it to the input method or the app.
pub fn key_press(keystroke: &Keystroke, repeat: bool) -> Option<KeyPress<'_>> {
    let modifiers = keystroke.modifiers;
    if modifiers.platform {
        return None;
    }
    let mods = Mods {
        shift: modifiers.shift,
        ctrl: modifiers.control,
        alt: modifiers.alt,
        super_key: false,
    };
    if let Some(key) = named_key(&keystroke.key) {
        return Some(KeyPress {
            key,
            mods,
            text: "",
            repeat,
        });
    }
    // Only Ctrl turns a typed character into a key press; Option types its own character, as on any Mac.
    if !modifiers.control {
        return None;
    }
    let key = typed_key(&keystroke.key)?;
    Some(KeyPress {
        key,
        mods,
        text: keystroke.key.as_str(),
        repeat,
    })
}

fn typed_key(name: &str) -> Option<Key> {
    if name == "space" {
        return Some(Key::Space);
    }
    let mut characters = name.chars();
    let character = characters.next()?;
    let single = characters.next().is_none();
    if !single {
        return None;
    }
    Some(Key::Char(character))
}

fn named_key(name: &str) -> Option<Key> {
    let key = match name {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        _ => return function_key(name),
    };
    Some(key)
}

fn function_key(name: &str) -> Option<Key> {
    let number = name.strip_prefix('f')?;
    let parsed: u8 = number.parse().ok()?;
    if !(1..=12).contains(&parsed) {
        return None;
    }
    Some(Key::Function(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key, its modifiers and its text, when the view takes `source`.
    fn press(source: &'static str) -> Option<(Key, Mods, String)> {
        let keystroke = Keystroke::parse(source).unwrap();
        let taken = key_press(&keystroke, false)?;
        Some((taken.key, taken.mods, taken.text.to_string()))
    }

    #[test]
    fn the_view_takes_named_keys_and_ctrl_chords_and_leaves_the_rest() {
        // Plain and Option-typed text goes to the input method; Cmd chords to the app's key bindings.
        assert_eq!(press("a"), None);
        assert_eq!(press("alt-e"), None);
        assert_eq!(press("cmd-c"), None);
        assert_eq!(press("cmd-left"), None);

        let (key, mods, text) = press("ctrl-c").unwrap();
        assert_eq!((key, mods.ctrl, text.as_str()), (Key::Char('c'), true, "c"));
        let (key, mods, _) = press("shift-tab").unwrap();
        assert_eq!((key, mods.shift), (Key::Tab, true));
        let (key, mods, _) = press("alt-left").unwrap();
        assert_eq!((key, mods.alt), (Key::Left, true));
        let (key, _, _) = press("f5").unwrap();
        assert_eq!(key, Key::Function(5));
    }
}
