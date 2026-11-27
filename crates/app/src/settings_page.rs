use std::collections::HashMap;

use gpui::{
    Context, Div, ElementId, Entity, FontWeight, InteractiveElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div, px,
};
use taco_identity::CURRENT;
use taco_settings::{
    Binding, CommandId, CursorStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, SettingChange, Settings,
    ShellRowName, ThemeChoice, UnreadSettings,
};

use crate::drive::mark;
use crate::look::{Look, SMALL_TEXT_SIZE};
use crate::motion::Motion;
use crate::page::PageMove;
use crate::page_parts::{
    CHIP_GAP, ROW_GAP, ROW_PADDING, UNREAD_OPACITY, chip, form_column, group, page_foot,
    page_frame, page_header, text_button,
};
use crate::parts;
use crate::shortcuts_page::chords_for;
use crate::text_field::TextField;
use crate::window::WindowView;

mod controls;
use controls::{segmented, segments_of, stepper, switch};

const SCROLL_ID: &str = "settings-scroll";
const SCROLLBACK_MARK: &str = "settings:scrollback";
const TITLE: &str = "Settings";
const OPEN_FILE_LABEL: &str = "Open settings.json";
const FOOT_TEXT: &str = "Changes are written to settings.json and apply at once.";
const KEYBOARD_GROUP: &str = "Keyboard";
const KEYBOARD_ROW: &str = "Keyboard shortcuts";
const KEYBOARD_HINT: &str = "See and change what each key combination does.";
const MORE: &str = "›";
const FIELD_FONT: &str = "Menlo";
const THEME_OPTIONS: [(&str, ThemeChoice); 3] = [
    ("System", ThemeChoice::System),
    ("Light", ThemeChoice::Light),
    ("Dark", ThemeChoice::Dark),
];
const CURSOR_OPTIONS: [(&str, CursorStyle); 3] = [
    ("Block", CursorStyle::Block),
    ("Underline", CursorStyle::Underline),
    ("Bar", CursorStyle::Bar),
];
const ROW_NAME_OPTIONS: [(&str, ShellRowName); 2] = [
    ("Folder name", ShellRowName::Folder),
    ("Short path", ShellRowName::Path),
];
const FONT_SIZE_STEP: f64 = 1.0;
const SCROLL_SPEED_STEP: f64 = 1.0;
const SCROLL_SPEED_MIN: f64 = 1.0;
const FONT_FIELD_WIDTH: f32 = 270.;
const SCROLLBACK_FIELD_WIDTH: f32 = 96.;

/// Everything one drawing of the settings page reads.
pub struct SettingsInput<'a> {
    pub settings: &'a Settings,
    /// The config's problem lines; when there are any, a card at the top of the form shows them.
    pub problems: &'a [String],
    /// The settings the file could not give, whose controls are drawn disabled.
    pub unread: &'a UnreadSettings,
    /// Why a setting's last write was refused, by its name in settings.json.
    pub refused: &'a HashMap<&'static str, String>,
    pub fields: &'a SettingsFields,
    /// What the keyboard shortcuts row shows as its own shortcut.
    pub bindings: &'a [Binding],
    pub motion: &'a Motion<'a>,
    pub look: &'a Look,
}

/// The page's two typed fields.
pub struct SettingsFields {
    pub font: Entity<TextField>,
    pub scrollback: Entity<TextField>,
}

pub fn settings_page(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let header = page_header(TITLE, None, look, cx);
    let form = form(input, cx);
    page_frame(SCROLL_ID, header, form, look)
}

fn form(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let mut form = form_column();
    if !input.problems.is_empty() {
        let card = problem_card(input.problems, look, cx);
        form = form.child(card);
    }
    let look_group = look_group(input, cx);
    let terminal_group = terminal_group(input, cx);
    let list_group = list_group(input, cx);
    let start_group = start_group(input, cx);
    let agents_group = agents_group(input, cx);
    let keyboard_group = keyboard_group(input, cx);
    let foot = foot(look, cx);
    form.child(look_group)
        .child(terminal_group)
        .child(list_group)
        .child(start_group)
        .child(agents_group)
        .child(keyboard_group)
        .child(foot)
}

fn keyboard_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let shortcut = chords_for(input.bindings, CommandId::ConfigOpenKeybindings);
    let mut shown = div().flex().flex_none().items_center().gap(px(CHIP_GAP));
    for chord in &shortcut {
        shown = shown.child(chip(chord, look));
    }
    let more = div().text_color(look.secondary_text).child(MORE);
    shown = shown.child(more);
    let name = div().font_weight(FontWeight::MEDIUM).child(KEYBOARD_ROW);
    let hint = div()
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .child(KEYBOARD_HINT);
    let text = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(name)
        .child(hint);
    let veil = look.veil;
    let open_shortcuts = PageMove::OpenShortcuts;
    let row = div()
        .id("keyboard-shortcuts")
        .flex()
        .items_center()
        .justify_between()
        .gap(px(ROW_GAP))
        .py(px(ROW_PADDING))
        .border_t_1()
        .border_color(look.hairline)
        .cursor_pointer()
        .hover(move |style| style.bg(veil))
        .child(text)
        .child(shown)
        .on_click(cx.listener(move |view, _, _, cx| view.move_pages(open_shortcuts, cx)));
    group(KEYBOARD_GROUP, look).child(row)
}

fn problem_card(problems: &[String], look: &Look, cx: &mut Context<WindowView>) -> Div {
    let open = open_file_button("settings-problems-open", look, cx);
    parts::problem_card(problems, open, look)
}

fn open_file_button(id: &'static str, look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let button = text_button(id, OPEN_FILE_LABEL, look);
    button.on_click(cx.listener(|view, _, _, cx| view.open_settings_file(cx)))
}

fn look_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let settings = input.settings;
    let look = input.look;
    let theme_options = segments_of(&THEME_OPTIONS, settings.theme, SettingChange::Theme);
    let theme = segmented(theme_options, input, cx);
    let font = typed_field(&input.fields.font)
        .min_w_0()
        .w(px(FONT_FIELD_WIDTH));
    let size = stepper(
        "font-size",
        settings.font_size,
        FONT_SIZE_STEP,
        FONT_SIZE_MIN,
        Some(FONT_SIZE_MAX),
        SettingChange::FontSize,
        look,
        cx,
    );
    let theme_hint = Some("System follows this computer.");
    let font_hint = Some("The terminal’s font, first one found.");
    let theme_shown = SettingChange::Theme(settings.theme);
    let font_shown = SettingChange::FontFamily(settings.font_family.clone());
    let size_shown = SettingChange::FontSize(settings.font_size);
    let theme_row = setting_row(&theme_shown, "Theme", theme_hint, theme, input);
    let font_row = setting_row(&font_shown, "Font", font_hint, font, input);
    let size_row = setting_row(&size_shown, "Font size", None, size, input);
    group("Look", look)
        .child(theme_row)
        .child(font_row)
        .child(size_row)
}

fn terminal_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let settings = input.settings;
    let look = input.look;
    let cursor_options = segments_of(
        &CURSOR_OPTIONS,
        settings.cursor_style,
        SettingChange::CursorStyle,
    );
    let cursor = segmented(cursor_options, input, cx);
    let blink = switch(
        "cursor-blink",
        settings.cursor_blink,
        SettingChange::CursorBlink,
        input,
        cx,
    );
    let scrollback_field = typed_field(&input.fields.scrollback).w(px(SCROLLBACK_FIELD_WIDTH));
    let scrollback = mark(scrollback_field, SCROLLBACK_MARK);
    let speed = stepper(
        "scroll-speed",
        settings.scroll_sensitivity,
        SCROLL_SPEED_STEP,
        SCROLL_SPEED_MIN,
        None,
        SettingChange::ScrollSensitivity,
        look,
        cx,
    );
    let scrollback_hint = Some("Lines of history kept per terminal.");
    let speed_hint = Some("Lines per notch of the wheel.");
    let cursor_shown = SettingChange::CursorStyle(settings.cursor_style);
    let blink_shown = SettingChange::CursorBlink(settings.cursor_blink);
    let scrollback_shown = SettingChange::Scrollback(settings.scrollback);
    let speed_shown = SettingChange::ScrollSensitivity(settings.scroll_sensitivity);
    let cursor_row = setting_row(&cursor_shown, "Cursor", None, cursor, input);
    let blink_row = setting_row(&blink_shown, "Cursor blinks", None, blink, input);
    let scrollback_row = setting_row(
        &scrollback_shown,
        "Scrollback",
        scrollback_hint,
        scrollback,
        input,
    );
    let speed_row = setting_row(&speed_shown, "Scroll speed", speed_hint, speed, input);
    group("Terminal", look)
        .child(cursor_row)
        .child(blink_row)
        .child(scrollback_row)
        .child(speed_row)
}

fn list_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let chosen = input.settings.shell_row_name;
    let segments = segments_of(&ROW_NAME_OPTIONS, chosen, SettingChange::ShellRowName);
    let names = segmented(segments, input, cx);
    let hint = Some("Rows with an agent keep the agent’s title.");
    let names_shown = SettingChange::ShellRowName(chosen);
    let names_row = setting_row(
        &names_shown,
        "A plain shell’s row shows",
        hint,
        names,
        input,
    );
    group("Terminals list", look).child(names_row)
}

fn start_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let settings = input.settings;
    let look = input.look;
    let restore = switch(
        "restore-on-launch",
        settings.restore_on_launch,
        SettingChange::RestoreOnLaunch,
        input,
        cx,
    );
    let hint = Some("Each in its folder, when the app starts.");
    let restore_shown = SettingChange::RestoreOnLaunch(settings.restore_on_launch);
    let restore_row = setting_row(&restore_shown, "Reopen my terminals", hint, restore, input);
    group("Starting up", look).child(restore_row)
}

fn agents_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let settings = input.settings;
    let look = input.look;
    let activity = switch(
        "agent-activity",
        settings.agent_activity,
        SettingChange::AgentActivity,
        input,
        cx,
    );
    let plugin_name = CURRENT.agent_app_name;
    let description = format!(
        "Agents in this app's terminals can write to the Agent Activity panel. Claude Code lists it in /plugin as the plugin {plugin_name}, inline."
    );
    let hint = Some(description.as_str());
    let activity_shown = SettingChange::AgentActivity(settings.agent_activity);
    let label = "Agent Activity in this app's terminals";
    let activity_row = setting_row(&activity_shown, label, hint, activity, input);
    let ask = switch(
        "ask-for-activity-notes",
        settings.ask_for_activity_notes,
        SettingChange::AskForActivityNotes,
        input,
        cx,
    );
    let ask_hint = Some("When an agent stops after real work without writing one.");
    let ask_shown = SettingChange::AskForActivityNotes(settings.ask_for_activity_notes);
    let ask_row = setting_row(&ask_shown, "Ask for a note", ask_hint, ask, input);
    group("Agents", look).child(activity_row).child(ask_row)
}

fn foot(look: &Look, cx: &mut Context<WindowView>) -> Div {
    let open = open_file_button("settings-foot-open", look, cx);
    page_foot(FOOT_TEXT, open, look)
}

/// One setting's row: `shown` is its current value, which names its key. An unread setting's control is dimmed and takes no clicks; a refusal replaces the hint.
fn setting_row(
    shown: &SettingChange,
    label: &str,
    hint: Option<&str>,
    control: Div,
    input: &SettingsInput,
) -> Div {
    let look = input.look;
    let key = shown.key();
    let name = div()
        .font_weight(FontWeight::MEDIUM)
        .child(label.to_string());
    // The text wraps rather than pushing its control out of the row.
    let mut text = div().flex().flex_col().flex_1().min_w_0().child(name);
    let refusal = input.refused.get(key);
    if let Some(reason) = refusal {
        let said = div()
            .text_size(px(SMALL_TEXT_SIZE))
            .text_color(look.needs)
            .child(reason.clone());
        text = text.child(said);
    } else if let Some(hint) = hint {
        let hint = div()
            .text_size(px(SMALL_TEXT_SIZE))
            .text_color(look.secondary_text)
            .child(hint.to_string());
        text = text.child(hint);
    }
    let mut control = control;
    if is_unread(input.unread, key) {
        let blocker_id = SharedString::from(format!("unread:{key}"));
        let blocker = div()
            .id(ElementId::Name(blocker_id))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude();
        control = div()
            .relative()
            .opacity(UNREAD_OPACITY)
            .child(control)
            .child(blocker);
    }
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(ROW_GAP))
        .py(px(ROW_PADDING))
        .border_t_1()
        .border_color(look.hairline)
        .child(text)
        .child(control)
}

/// Whether the file could not give this setting, so the page cannot change it.
pub fn is_unread(unread: &UnreadSettings, key: &str) -> bool {
    match unread {
        UnreadSettings::None => false,
        UnreadSettings::Keys(keys) => keys.contains(&key),
        UnreadSettings::Whole => true,
    }
}

fn typed_field(field: &Entity<TextField>) -> Div {
    div()
        .font_family(FIELD_FONT)
        .text_size(px(SMALL_TEXT_SIZE))
        .child(field.clone())
}
