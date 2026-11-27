//! The keyboard shortcuts page, a page under Settings listing each command with its shortcuts.

use gpui::{
    Context, Div, Entity, FontWeight, InteractiveElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};
use taco_settings::{Binding, Chord, CommandId};

use crate::look::{Look, SMALL_TEXT_SIZE};
use crate::settings_page::{form_column, group, page_frame, page_header};
use crate::text_field::TextField;
use crate::window::WindowView;

pub const CHIP_GAP: f32 = 5.;
pub const SEARCH_PLACEHOLDER: &str = "Search commands and shortcuts";
const TITLE: &str = "Keyboard shortcuts";
const BACK: &str = "Settings";
const NOTHING_FOUND: &str = "No command or shortcut matches.";
const NONE: &str = "None";
const ADD: &str = "+";
const BACK_TO_DEFAULT: &str = "Back to default";
const FOOT_TEXT: &str = "Changes are written to keybindings.json and apply at once.";
const OPEN_FILE_LABEL: &str = "Open keybindings.json";
const CHIP_HEIGHT: f32 = 24.;
const CHIP_PADDING: f32 = 6.;
const CHIP_RADIUS: f32 = 6.;
const CHIP_INNER_GAP: f32 = 3.;
const ROW_PADDING: f32 = 6.;
const ROW_GAP: f32 = 12.;
const BUTTON_PADDING_X: f32 = 8.;
const BUTTON_PADDING_Y: f32 = 3.;
const BUTTON_RADIUS: f32 = 6.;
const FOOT_GAP: f32 = 12.;
const FOOT_PADDING_TOP: f32 = 10.;

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

// How a query may name a modifier, against the spelling the page searches.
const MODIFIER_WORDS: [(&str, &str); 4] = [
    ("alt", "opt"),
    ("option", "opt"),
    ("command", "cmd"),
    ("control", "ctrl"),
];

const KEY_SYMBOLS: [(&str, &str); 14] = [
    ("left", "←"),
    ("right", "→"),
    ("up", "↑"),
    ("down", "↓"),
    ("enter", "↩"),
    ("backspace", "⌫"),
    ("delete", "⌦"),
    ("tab", "⇥"),
    ("escape", "Esc"),
    ("space", "Space"),
    ("home", "↖"),
    ("end", "↘"),
    ("pageup", "⇞"),
    ("pagedown", "⇟"),
];

/// Everything one drawing of the shortcuts page reads.
pub struct ShortcutsInput<'a> {
    pub bindings: &'a [Binding],
    /// The shipped bindings alone, which "Back to default" returns a command to.
    pub defaults: &'a [Binding],
    pub query: &'a str,
    pub search: &'a Entity<TextField>,
    pub look: &'a Look,
}

pub fn shortcuts_page(input: &ShortcutsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let header = page_header(TITLE, Some(BACK), look, cx);
    let search = div().w_full().child(input.search.clone());
    let mut form = form_column().child(search);
    let mut found_any = false;
    for (heading, commands) in GROUPS {
        let mut rows = Vec::new();
        for &(command, name) in commands {
            let chords = chords_for(input.bindings, command);
            if !matches(input.query, name, &chords) {
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
    page_frame(header, form.child(foot), look)
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

/// A shortcut as a small key cap with the Mac's symbols.
pub(crate) fn chip(chord: &Chord, look: &Look) -> Div {
    let label = chip_text(chord);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(CHIP_INNER_GAP))
        .h(px(CHIP_HEIGHT))
        .px(px(CHIP_PADDING))
        .rounded(px(CHIP_RADIUS))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .text_color(look.text)
        .font_weight(FontWeight::MEDIUM)
        .child(label)
}

fn small_button(label: &'static str, look: &Look) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .px(px(BUTTON_PADDING_X))
        .rounded(px(BUTTON_RADIUS))
        .text_color(look.secondary_text)
        .child(label)
}

fn foot(look: &Look, cx: &mut Context<WindowView>) -> Div {
    let veil = look.veil;
    let open = div()
        .id("shortcuts-foot-open")
        .flex_none()
        .px(px(BUTTON_PADDING_X))
        .py(px(BUTTON_PADDING_Y))
        .rounded(px(BUTTON_RADIUS))
        .border_1()
        .border_color(look.hairline)
        .text_color(look.text)
        .cursor_pointer()
        .hover(move |style| style.bg(veil))
        .child(OPEN_FILE_LABEL)
        .on_click(cx.listener(|view, _, _, cx| view.open_keybindings_file(cx)));
    div()
        .flex()
        .items_center()
        .gap(px(FOOT_GAP))
        .pt(px(FOOT_PADDING_TOP))
        .border_t_1()
        .border_color(look.hairline)
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .child(FOOT_TEXT)
        .child(open)
}

/// The command's chords, in the order the keymap holds them.
pub(crate) fn chords_for(bindings: &[Binding], command: CommandId) -> Vec<Chord> {
    let mut chords = Vec::new();
    for binding in bindings {
        if binding.command == command {
            chords.push(binding.chord.clone());
        }
    }
    chords
}

/// Whether a command's chords are not exactly its defaults, in any order.
fn differs(chords: &[Chord], defaults: &[Chord]) -> bool {
    let same_count = chords.len() == defaults.len();
    let all_default = chords.iter().all(|chord| defaults.contains(chord));
    !(same_count && all_default)
}

/// A chord in the Mac's symbols: "⌘⇧," for cmd+shift+,.
pub fn chip_text(chord: &Chord) -> String {
    let mut text = String::new();
    if chord.cmd {
        text.push('⌘');
    }
    if chord.ctrl {
        text.push('⌃');
    }
    if chord.alt {
        text.push('⌥');
    }
    if chord.shift {
        text.push('⇧');
    }
    let mut symbols = KEY_SYMBOLS.iter();
    let named = symbols.find(|(key, _)| *key == chord.key);
    match named {
        Some((_, symbol)) => text.push_str(symbol),
        None => text.push_str(&chord.key.to_uppercase()),
    }
    text
}

/// A chord as the file spells it, with "opt" for the option key: "cmd+opt+left".
pub fn file_spelling(chord: &Chord) -> String {
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

/// Whether a command is found by the query: in its name, or in one of its chords as the file spells them.
pub fn matches(query: &str, name: &str, chords: &[Chord]) -> bool {
    let wanted = normalized_query(query);
    if wanted.is_empty() {
        return true;
    }
    let lower_name = name.to_lowercase();
    if lower_name.contains(&wanted) {
        return true;
    }
    for chord in chords {
        let spelled = file_spelling(chord);
        if spelled.contains(&wanted) {
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

    #[test]
    fn a_chip_reads_in_the_macs_symbols() {
        let cases = [
            ("cmd+shift+,", "⌘⇧,"),
            ("opt+left", "⌥←"),
            ("shift+enter", "⇧↩"),
            ("ctrl+k", "⌃K"),
            ("cmd+backspace", "⌘⌫"),
        ];
        for (written, shown) in cases {
            assert_eq!(chip_text(&chord(written)), shown, "{written}");
        }
    }

    #[test]
    fn search_finds_a_command_by_name_or_by_any_spelling_of_its_shortcut() {
        let clear = [chord("cmd+k")];
        let word_left = [chord("opt+left")];
        assert!(matches("", "Clear", &clear));
        assert!(matches("  CLE ", "Clear", &clear));
        assert!(matches("cmd+k", "Clear", &clear));
        assert!(matches("command+k", "Clear", &clear));
        assert!(matches("alt+left", "Back a word", &word_left));
        assert!(matches("option+left", "Back a word", &word_left));
        assert!(!matches("cmd+j", "Clear", &clear));
    }

    #[test]
    fn back_to_default_shows_only_when_the_chords_are_not_the_defaults() {
        let both = [chord("cmd+t"), chord("cmd+n")];
        let reversed = [chord("cmd+n"), chord("cmd+t")];
        let one = [chord("cmd+t")];
        assert!(!differs(&reversed, &both));
        assert!(differs(&one, &both));
        assert!(differs(&both, &one));
        assert!(differs(&[], &one));
    }
}
