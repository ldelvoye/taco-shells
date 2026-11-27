//! The keyboard shortcuts page, a page under Settings listing each command with its shortcuts.

use std::collections::HashMap;

use gpui::{
    Context, Div, ElementId, Entity, FontWeight, InteractiveElement, MouseButton, ParentElement,
    SharedString, Stateful, StatefulInteractiveElement, Styled, div, px,
};
use taco_settings::{Binding, Chord, CommandId, KeymapChange, command_for};

use crate::drive::mark;
use crate::look::{Look, SMALL_TEXT_SIZE};
use crate::page::{
    BUTTON_RADIUS, CHIP_GAP, ROW_GAP, ROW_PADDING, chip, chip_text, form_column, group, page_foot,
    page_frame, page_header, text_button,
};
use crate::shell::{shell_orange, shell_outline};
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
const PRESS_KEYS: &str = "Press keys";
const REMOVE: &str = "Remove";
const CANCEL: &str = "Cancel";
const USE_IT_HERE: &str = "Use it here";
const NEEDS_MODIFIER: &str = "A shortcut needs cmd, ctrl or opt.";
const SMALL_BUTTON_HEIGHT: f32 = 24.;
const SMALL_BUTTON_PADDING: f32 = 8.;
const CAPTURE_PADDING: f32 = 9.;
const CAPTURE_RADIUS: f32 = 6.;
const CAPTURE_OUTLINE: f32 = 2.;
const CAPTURE_WASH: f32 = 0.12;
const HINT_GAP: f32 = 4.;
const CARD_PADDING: f32 = 10.;
const CARD_RADIUS: f32 = 8.;
const CARD_GAP: f32 = 8.;
const UNREAD_OPACITY: f32 = 0.45;

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

/// Everything one drawing of the shortcuts page reads.
pub struct ShortcutsInput<'a> {
    pub bindings: &'a [Binding],
    pub defaults: &'a [Binding],
    pub query: &'a str,
    pub search: &'a Entity<TextField>,
    pub capture: Option<&'a Capture>,
    /// Why each command's last change was not written, until it is changed again.
    pub refused: &'a HashMap<CommandId, String>,
    /// The file could not be read at all, so nothing on the page can change it.
    pub unread: bool,
    pub problems: &'a [String],
    pub look: &'a Look,
}

/// Whether a row is setting a shortcut, and which chip it is changing.
#[derive(Clone, Copy)]
enum RowSetting<'a> {
    Idle,
    Changing(&'a Capture, &'a Chord),
    Adding(&'a Capture),
}

/// One command's row as the page draws it.
struct Row<'a> {
    command: CommandId,
    name: &'static str,
    chords: &'a [Chord],
    /// Its chords are not the shipped ones, so "Back to default" shows.
    changed: bool,
}

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

pub fn shortcuts_page(input: &ShortcutsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let header = page_header(TITLE, Some(BACK), look, cx);
    // Clicking into the search ends a capture, which would otherwise take every key typed there.
    let search = div().w_full().child(input.search.clone()).on_mouse_down(
        MouseButton::Left,
        cx.listener(|view, _, _, cx| view.cancel_capture(cx)),
    );
    let wanted = normalized_query(input.query);
    let mut form = form_column();
    if !input.problems.is_empty() {
        let card = problem_card(input.problems, look, cx);
        form = form.child(card);
    }
    form = form.child(search);
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
            let row = Row {
                command,
                name,
                chords: &chords,
                changed,
            };
            rows.push(command_row(&row, input, cx));
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

fn command_row(row: &Row, input: &ShortcutsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let command = row.command;
    let setting = row_setting(command, input.capture);
    let label = div()
        .flex_1()
        .min_w_0()
        .font_weight(FontWeight::MEDIUM)
        .child(row.name.to_string());
    let mut keys = div()
        .flex()
        .flex_wrap()
        .flex_none()
        .items_center()
        .justify_end()
        .gap(px(CHIP_GAP));
    for (index, chord) in row.chords.iter().enumerate() {
        if let RowSetting::Changing(open, changed) = setting
            && changed == chord
        {
            keys = keys.child(capture_cap(open, look));
            continue;
        }
        let shown = chip(chord, look);
        if input.unread {
            keys = keys.child(shown.opacity(UNREAD_OPACITY));
            continue;
        }
        let id = SharedString::from(format!("chip:{}:{index}", command.id()));
        let target = Some(chord.clone());
        let clickable = div()
            .id(ElementId::Name(id))
            .cursor_pointer()
            .child(shown)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.start_capture(command, target.clone(), cx);
            }));
        keys = keys.child(clickable);
    }
    let adding = matches!(setting, RowSetting::Adding(_));
    if row.chords.is_empty() && !adding {
        let none = div().text_color(look.secondary_text).child(NONE);
        keys = keys.child(none);
    }
    if let RowSetting::Adding(open) = setting {
        keys = keys.child(capture_cap(open, look));
    }
    let buttons = match setting {
        RowSetting::Changing(open, _) | RowSetting::Adding(open) => capture_buttons(open, look, cx),
        RowSetting::Idle => row_buttons(row, input, cx),
    };
    keys = keys.children(buttons);
    let line = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(ROW_GAP))
        .child(label)
        .child(keys);
    let mut shown_row = div()
        .flex()
        .flex_col()
        .py(px(ROW_PADDING))
        .border_t_1()
        .border_color(look.hairline)
        .child(line);
    let hint = row_hint(command, setting.capture(), input, cx);
    if let Some(hint) = hint {
        shown_row = shown_row.child(hint);
    }
    shown_row
}

fn row_setting(command: CommandId, capture: Option<&Capture>) -> RowSetting<'_> {
    let Some(open) = capture else {
        return RowSetting::Idle;
    };
    if open.command != command {
        return RowSetting::Idle;
    }
    match &open.changing {
        Some(chord) => RowSetting::Changing(open, chord),
        None => RowSetting::Adding(open),
    }
}

/// The chord being set: "Press keys" until keys are pressed.
fn capture_cap(capture: &Capture, look: &Look) -> Div {
    let mut text = PRESS_KEYS.to_string();
    if let Some(pending) = &capture.pending {
        text = chip_text(pending);
    }
    let outline = shell_outline(CAPTURE_OUTLINE, CAPTURE_RADIUS);
    let wash = shell_orange().opacity(CAPTURE_WASH);
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .h(px(SMALL_BUTTON_HEIGHT))
        .px(px(CAPTURE_PADDING))
        .rounded(px(CAPTURE_RADIUS))
        .bg(wash)
        .text_color(look.text)
        .font_weight(FontWeight::MEDIUM)
        .child(text)
        .child(outline)
}

fn capture_buttons(
    capture: &Capture,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Vec<Stateful<Div>> {
    let mut buttons = Vec::new();
    if capture.changing.is_some() {
        let remove = small_button("shortcut-remove", REMOVE, look)
            .on_click(cx.listener(|view, _, _, cx| view.remove_captured(cx)));
        buttons.push(remove);
    }
    let cancel = small_button("shortcut-cancel", CANCEL, look)
        .on_click(cx.listener(|view, _, _, cx| view.cancel_capture(cx)));
    buttons.push(cancel);
    buttons
}

fn row_buttons(
    row: &Row,
    input: &ShortcutsInput,
    cx: &mut Context<WindowView>,
) -> Vec<Stateful<Div>> {
    let mut buttons = Vec::new();
    if input.unread {
        return buttons;
    }
    let look = input.look;
    let command = row.command;
    let add_id = SharedString::from(format!("shortcut-add:{}", command.id()));
    let add = small_button(add_id.clone(), ADD, look)
        .on_click(cx.listener(move |view, _, _, cx| view.start_capture(command, None, cx)));
    // The smoke check finds the + by this mark.
    buttons.push(mark(add, add_id));
    if row.changed {
        let reset_id = SharedString::from(format!("shortcut-reset:{}", command.id()));
        let reset = small_button(reset_id, BACK_TO_DEFAULT, look)
            .on_click(cx.listener(move |view, _, _, cx| view.reset_shortcut(command, cx)));
        buttons.push(reset);
    }
    buttons
}

/// The line under a row: a question about the keys pressed, or why the last change was refused.
fn row_hint(
    command: CommandId,
    capture: Option<&Capture>,
    input: &ShortcutsInput,
    cx: &mut Context<WindowView>,
) -> Option<Div> {
    let look = input.look;
    let line = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(CHIP_GAP))
        .pt(px(HINT_GAP))
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.needs);
    if let Some(open) = capture
        && let Some(problem) = open.problem
        && let Some(pending) = &open.pending
    {
        return Some(problem_line(line, problem, pending, look, cx));
    }
    let refusal = input.refused.get(&command)?;
    Some(line.child(refusal.clone()))
}

fn problem_line(
    line: Div,
    problem: CaptureProblem,
    pending: &Chord,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    match problem {
        CaptureProblem::NeedsModifier => line.child(NEEDS_MODIFIER),
        CaptureProblem::Clash(other) => {
            let keys = chip_text(pending);
            let said = format!("{keys} is the shortcut for {}.", name_of(other));
            let take = text_button("shortcut-take", USE_IT_HERE, look)
                .on_click(cx.listener(|view, _, _, cx| view.take_pending(cx)));
            line.child(said).child(take)
        }
    }
}

fn small_button(id: impl Into<SharedString>, label: &'static str, look: &Look) -> Stateful<Div> {
    let veil = look.strong_veil;
    let text = look.text;
    let name: SharedString = id.into();
    div()
        .id(ElementId::Name(name))
        .flex()
        .flex_none()
        .items_center()
        .h(px(SMALL_BUTTON_HEIGHT))
        .px(px(SMALL_BUTTON_PADDING))
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .text_color(look.secondary_text)
        .hover(move |style| style.bg(veil).text_color(text))
        .child(label)
}

fn problem_card(problems: &[String], look: &Look, cx: &mut Context<WindowView>) -> Div {
    let mut card = div()
        .flex()
        .flex_col()
        .items_start()
        .gap(px(CARD_GAP))
        .p(px(CARD_PADDING))
        .rounded(px(CARD_RADIUS))
        .bg(look.selected_card)
        .border_1()
        .border_color(look.hairline)
        .text_color(look.text);
    for problem in problems {
        let line = div().child(problem.clone());
        card = card.child(line);
    }
    let open = text_button("shortcuts-problems-open", OPEN_FILE_LABEL, look)
        .on_click(cx.listener(|view, _, _, cx| view.open_keybindings_file(cx)));
    card.child(open)
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

impl<'a> RowSetting<'a> {
    fn capture(self) -> Option<&'a Capture> {
        match self {
            RowSetting::Idle => None,
            RowSetting::Changing(open, _) | RowSetting::Adding(open) => Some(open),
        }
    }
}

fn name_of(command: CommandId) -> &'static str {
    for (_, commands) in GROUPS {
        for &(listed, name) in commands {
            if listed == command {
                return name;
            }
        }
    }
    command.id()
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
