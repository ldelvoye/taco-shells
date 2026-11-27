//! The keyboard shortcuts page, a page under Settings listing each command with its shortcuts.

use gpui::{
    Context, Div, Entity, FontWeight, ParentElement, StatefulInteractiveElement, Styled, div, px,
};
use taco_settings::{Binding, Chord, CommandId};

use crate::look::Look;
use crate::page::{
    BUTTON_RADIUS, CHIP_GAP, ROW_GAP, ROW_PADDING, chip, form_column, group, page_foot, page_frame,
    page_header, text_button,
};
use crate::text_field::TextField;
use crate::window::WindowView;

pub const SEARCH_PLACEHOLDER: &str = "Search commands and shortcuts";
const SCROLL_ID: &str = "shortcuts-scroll";
const TITLE: &str = "Keyboard shortcuts";
const BACK: &str = "Settings";
const NOTHING_FOUND: &str = "No command or shortcut matches.";
const NONE: &str = "None";
const ADD: &str = "+";
const BACK_TO_DEFAULT: &str = "Back to default";
const FOOT_TEXT: &str = "Changes are written to keybindings.json and apply at once.";
const OPEN_FILE_LABEL: &str = "Open keybindings.json";
const SMALL_BUTTON_HEIGHT: f32 = 24.;
const SMALL_BUTTON_PADDING: f32 = 8.;

const GROUPS: [(&str, &[(CommandId, &str)]); 3] = [
    (
        "Terminals",
        &[
            (CommandId::TerminalNew, "New terminal"),
            (CommandId::TerminalClose, "Close terminal"),
            (CommandId::TerminalFocusNextGroup, "Next terminal"),
            (CommandId::TerminalFocusPreviousGroup, "Previous terminal"),
        ],
    ),
    (
        "In the terminal",
        &[
            (CommandId::TerminalCopy, "Copy"),
            (CommandId::TerminalPaste, "Paste"),
            (CommandId::TerminalClear, "Clear"),
            (
                CommandId::TerminalKillLine,
                "Delete to the start of the line",
            ),
            (CommandId::TerminalLineStart, "Go to the start of the line"),
            (CommandId::TerminalLineEnd, "Go to the end of the line"),
            (CommandId::TerminalWordLeft, "Back a word"),
            (CommandId::TerminalWordRight, "Forward a word"),
            (CommandId::TerminalInsertNewline, "New line without running"),
        ],
    ),
    (
        "Window",
        &[
            (CommandId::SidebarToggle, "Show or hide Terminals"),
            (CommandId::ConfigOpenSettings, "Settings"),
            (CommandId::ConfigOpenKeybindings, "Keyboard shortcuts"),
        ],
    ),
];

/// Query words for a modifier, as the file spells it: "alt" and "option" search as "opt".
const MODIFIER_WORDS: [(&str, &str); 4] = [
    ("alt", "opt"),
    ("option", "opt"),
    ("command", "cmd"),
    ("control", "ctrl"),
];

/// Everything one drawing of the shortcuts page reads.
pub struct ShortcutsInput<'a> {
    pub bindings: &'a [Binding],
    pub defaults: &'a [Binding],
    pub query: &'a str,
    pub search: &'a Entity<TextField>,
    pub look: &'a Look,
}

pub fn shortcuts_page(input: &ShortcutsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let header = page_header(TITLE, Some(BACK), look, cx);
    let search = div().w_full().child(input.search.clone());
    let wanted = normalized_query(input.query);
    let mut form = form_column().child(search);
    let mut found_any = false;
    for (heading, commands) in GROUPS {
        let mut rows = Vec::new();
        for &(command, name) in commands {
            let chords = chords_for(input.bindings, command);
            if !is_found(&wanted, name, &chords) {
                continue;
            }
            let defaults = chords_for(input.defaults, command);
            let changed = differs(&chords, &defaults);
            rows.push(command_row(name, &chords, changed, look));
        }
        if rows.is_empty() {
            continue;
        }
        found_any = true;
        let shown = group(heading, look).children(rows);
        form = form.child(shown);
    }
    if !found_any {
        let nothing = div().text_color(look.secondary_text).child(NOTHING_FOUND);
        form = form.child(nothing);
    }
    let foot = foot(look, cx);
    let whole = form.child(foot);
    page_frame(SCROLL_ID, header, whole, look)
}

fn command_row(name: &str, chords: &[Chord], changed: bool, look: &Look) -> Div {
    let label = div()
        .flex_1()
        .min_w_0()
        .font_weight(FontWeight::MEDIUM)
        .child(name.to_string());
    let mut keys = div()
        .flex()
        .flex_wrap()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(CHIP_GAP));
    for chord in chords {
        keys = keys.child(chip(chord, look));
    }
    if chords.is_empty() {
        let none = div().text_color(look.secondary_text).child(NONE);
        keys = keys.child(none);
    }
    keys = keys.child(small_button(ADD, look));
    if changed {
        keys = keys.child(small_button(BACK_TO_DEFAULT, look));
    }
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(ROW_GAP))
        .py(px(ROW_PADDING))
        .border_t_1()
        .border_color(look.hairline)
        .child(label)
        .child(keys)
}

fn small_button(label: &'static str, look: &Look) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(SMALL_BUTTON_HEIGHT))
        .px(px(SMALL_BUTTON_PADDING))
        .rounded(px(BUTTON_RADIUS))
        .text_color(look.secondary_text)
        .child(label)
}

fn foot(look: &Look, cx: &mut Context<WindowView>) -> Div {
    let button = text_button("shortcuts-foot-open", OPEN_FILE_LABEL, look);
    let open = button.on_click(cx.listener(|view, _, _, cx| view.open_keybindings_file(cx)));
    page_foot(FOOT_TEXT, open, look)
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
fn differs(chords: &[Chord], defaults: &[Chord]) -> bool {
    let same_count = chords.len() == defaults.len();
    let all_default = chords.iter().all(|chord| defaults.contains(chord));
    !(same_count && all_default)
}

/// A chord as the file spells it, with "opt" for the option key: "cmd+opt+left".
fn file_spelling(chord: &Chord) -> String {
    let mut parts = Vec::new();
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
    parts.push(chord.key.as_str());
    parts.join("+")
}

/// Whether the normalized query finds a command by its name or by a chord in the file's spelling.
fn is_found(wanted: &str, name: &str, chords: &[Chord]) -> bool {
    if wanted.is_empty() {
        return true;
    }
    let lower_name = name.to_lowercase();
    if lower_name.contains(wanted) {
        return true;
    }
    for chord in chords {
        let spelled = file_spelling(chord);
        if spelled.contains(wanted) {
            return true;
        }
    }
    false
}

fn normalized_query(query: &str) -> String {
    let lower = query.trim().to_lowercase();
    let mut parts = Vec::new();
    for part in lower.split('+') {
        let mut words = MODIFIER_WORDS.iter();
        let renamed = words.find(|(said, _)| *said == part);
        match renamed {
            Some((_, spelled)) => parts.push(spelled.to_string()),
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
    fn every_command_is_listed_once() {
        let mut listed = Vec::new();
        for (_, commands) in GROUPS {
            for &(command, _) in commands {
                listed.push(command);
            }
        }
        for command in CommandId::ALL {
            let times = listed.iter().filter(|shown| **shown == command).count();
            assert_eq!(times, 1, "{command:?}");
        }
        assert_eq!(listed.len(), CommandId::ALL.len());
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

    #[test]
    fn back_to_default_shows_only_when_the_chords_are_not_the_defaults() {
        let both = [chord("cmd+t"), chord("cmd+n")];
        let reversed = [chord("cmd+n"), chord("cmd+t")];
        let one = [chord("cmd+t")];
        assert!(!differs(&reversed, &both));
        assert!(differs(&one, &both));
        assert!(differs(&both, &one));
    }
}
