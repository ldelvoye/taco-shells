use std::collections::HashMap;

use gpui::{
    Background, Context, Div, ElementId, Entity, FontWeight, Hsla, InteractiveElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, div, px,
    transparent_black, white,
};
use taco_settings::{
    CommandId, CursorStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, SettingChange, Settings, ShellRowName,
    ThemeChoice, UnreadSettings,
};

use crate::drive::mark;
use crate::look::{FONT_FAMILY, Look, SMALL_TEXT_SIZE, TEXT_SIZE, TITLE_BAR_HEIGHT};
use crate::shell::{shell_gradient, shell_outline};
use crate::text_field::TextField;
use crate::window::WindowView;

const SCROLL_ID: &str = "settings-scroll";
const SCROLLBACK_MARK: &str = "settings:scrollback";
const SHELL_ANGLE: f32 = 90.;
const SHELL_OUTLINE: f32 = 1.5;
const TITLE: &str = "Settings";
const OPEN_FILE_LABEL: &str = "Open settings.json";
const FOOT_TEXT: &str = "Changes are written to settings.json and apply at once.";
const FIELD_FONT: &str = "Menlo";
const UNREAD_OPACITY: f32 = 0.45;
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
const HEADER_PADDING_LEFT: f32 = 16.;
const HEADER_PADDING_RIGHT: f32 = 10.;
const HEADER_BUTTON_SIZE: f32 = 22.;
const HEADER_GLYPH_SIZE: f32 = 15.;
const TITLE_SIZE: f32 = 13.;
const FORM_MAX_WIDTH: f32 = 600.;
const FORM_PADDING_TOP: f32 = 20.;
const FORM_PADDING_SIDES: f32 = 24.;
const FORM_PADDING_BOTTOM: f32 = 28.;
const FORM_GAP: f32 = 22.;
const CARD_PADDING: f32 = 10.;
const CARD_RADIUS: f32 = 8.;
const CARD_GAP: f32 = 8.;
const GROUP_GAP: f32 = 2.;
const HEADING_SIZE: f32 = 10.5;
const HEADING_GAP: f32 = 4.;
const ROW_PADDING: f32 = 7.;
const ROW_GAP: f32 = 16.;
const TRACK_PADDING: f32 = 2.;
const TRACK_RADIUS: f32 = 7.;
const SEGMENT_RADIUS: f32 = 5.;
const SEGMENT_PADDING_Y: f32 = 3.;
const SEGMENT_PADDING_X: f32 = 10.;
const SWITCH_WIDTH: f32 = 34.;
const SWITCH_HEIGHT: f32 = 20.;
const KNOB_SIZE: f32 = 14.;
const KNOB_TOP: f32 = 2.;
const KNOB_LEFT_OFF: f32 = 3.;
const KNOB_LEFT_ON: f32 = 17.;
const STEPPER_BUTTON_SIZE: f32 = 24.;
const STEPPER_RADIUS: f32 = 6.;
const STEPPER_GAP: f32 = 6.;
const STEPPER_VALUE_MIN_WIDTH: f32 = 22.;
const STEPPER_DISABLED_OPACITY: f32 = 0.5;
const FIELD_MAX_WIDTH: f32 = 280.;
const SCROLLBACK_FIELD_WIDTH: f32 = 96.;
const BUTTON_RADIUS: f32 = 6.;
const BUTTON_PADDING_Y: f32 = 3.;
const BUTTON_PADDING_X: f32 = 9.;
const FOOT_GAP: f32 = 12.;
const FOOT_PADDING_TOP: f32 = 10.;

/// What a command does while a page covers the terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverPage {
    ClosePage,
    ToggleSettings,
    /// It shows a terminal, so the pages close first.
    ShowTerminal,
    Run,
    /// The terminal's own commands: left to whatever on the page has the keyboard.
    PassOn,
}

/// A page drawn in the middle in place of the terminal; the window keeps a stack of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Settings,
}

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
    pub look: &'a Look,
}

/// The settings typed rather than chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedSetting {
    Font,
    Scrollback,
}

#[derive(Debug, PartialEq)]
pub enum Typed {
    Change(SettingChange),
    Unchanged,
    /// Scrollback that is not a whole number, which is never sent.
    NotWhole,
}

/// The page's two typed fields.
pub struct SettingsFields {
    pub font: Entity<TextField>,
    pub scrollback: Entity<TextField>,
}

struct Segment {
    label: &'static str,
    chosen: bool,
    change: SettingChange,
}

/// Nothing closes a terminal nobody can see, and anything that shows one closes the pages first.
pub fn over_page(command: CommandId) -> OverPage {
    match command {
        CommandId::TerminalClose => OverPage::ClosePage,
        CommandId::ConfigOpenSettings => OverPage::ToggleSettings,
        CommandId::TerminalNew
        | CommandId::TerminalFocusNextGroup
        | CommandId::TerminalFocusPreviousGroup
        | CommandId::ConfigOpenKeybindings => OverPage::ShowTerminal,
        CommandId::SidebarToggle => OverPage::Run,
        _ => OverPage::PassOn,
    }
}

pub fn settings_page(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let header = header(look, cx);
    let form = form(input, cx);
    let body = div()
        .id(SCROLL_ID)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(form);
    div()
        .flex()
        .flex_col()
        .size_full()
        .bg(look.panels)
        .text_color(look.text)
        .font_family(FONT_FAMILY)
        .text_size(px(TEXT_SIZE))
        .child(header)
        .child(body)
}

fn header(look: &Look, cx: &mut Context<WindowView>) -> Div {
    let header = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(TITLE_BAR_HEIGHT))
        .pl(px(HEADER_PADDING_LEFT))
        .pr(px(HEADER_PADDING_RIGHT))
        .border_b_1()
        .border_color(look.hairline);
    let title = div()
        .text_size(px(TITLE_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .child(TITLE);
    let gap = div().flex_1();
    let close = header_button("settings-close", "×", look.strong_veil, look.text, look);
    let close = close.on_click(cx.listener(|view, _, _, cx| view.close_page(cx)));
    header.child(title).child(gap).child(close)
}

fn header_button(
    id: &'static str,
    glyph: &'static str,
    hover_background: Hsla,
    hover_text: Hsla,
    look: &Look,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(HEADER_BUTTON_SIZE))
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .text_size(px(HEADER_GLYPH_SIZE))
        .text_color(look.secondary_text)
        .hover(move |style| style.bg(hover_background).text_color(hover_text))
        .child(glyph)
}

fn form(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let mut form = div()
        .flex()
        .flex_col()
        .gap(px(FORM_GAP))
        .w_full()
        .max_w(px(FORM_MAX_WIDTH))
        .mx_auto()
        .pt(px(FORM_PADDING_TOP))
        .px(px(FORM_PADDING_SIDES))
        .pb(px(FORM_PADDING_BOTTOM));
    if !input.problems.is_empty() {
        let card = problem_card(input.problems, look, cx);
        form = form.child(card);
    }
    let look_group = look_group(input, cx);
    let terminal_group = terminal_group(input, cx);
    let list_group = list_group(input, cx);
    let start_group = start_group(input, cx);
    let agents_group = agents_group(input, cx);
    let foot = foot(look, cx);
    form.child(look_group)
        .child(terminal_group)
        .child(list_group)
        .child(start_group)
        .child(agents_group)
        .child(foot)
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
    let open = open_file_button("settings-problems-open", look, cx);
    card.child(open)
}

fn look_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let settings = input.settings;
    let look = input.look;
    let theme_options = segments_of(&THEME_OPTIONS, settings.theme, SettingChange::Theme);
    let theme = segmented(theme_options, look, cx);
    let font = typed_field(&input.fields.font)
        .flex_1()
        .min_w_0()
        .max_w(px(FIELD_MAX_WIDTH));
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
    let cursor = segmented(cursor_options, look, cx);
    let blink = switch(
        "cursor-blink",
        settings.cursor_blink,
        SettingChange::CursorBlink,
        look,
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
    let names = segmented(segments, look, cx);
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
        look,
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
    let hooks = switch(
        "manage-agent-hooks",
        settings.manage_agent_hooks,
        SettingChange::ManageAgentHooks,
        look,
        cx,
    );
    let hint = Some(
        "The app offers and keeps up the hooks coding agents need. Turn off to wire them by hand.",
    );
    let hooks_shown = SettingChange::ManageAgentHooks(settings.manage_agent_hooks);
    let hooks_row = setting_row(&hooks_shown, "Keep agent hooks set up", hint, hooks, input);
    group("Agents", look).child(hooks_row)
}

fn foot(look: &Look, cx: &mut Context<WindowView>) -> Div {
    let open = open_file_button("settings-foot-open", look, cx);
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

fn group(heading: &str, look: &Look) -> Div {
    let shout = heading.to_uppercase();
    let title = div()
        .pb(px(HEADING_GAP))
        .text_size(px(HEADING_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(look.secondary_text)
        .child(shout);
    div().flex().flex_col().gap(px(GROUP_GAP)).child(title)
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
    let mut text = div().flex().flex_col().child(name);
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

fn segments_of<T: Copy + PartialEq>(
    options: &[(&'static str, T)],
    chosen: T,
    make: fn(T) -> SettingChange,
) -> Vec<Segment> {
    let mut segments = Vec::new();
    for &(label, option) in options {
        segments.push(Segment {
            label,
            chosen: option == chosen,
            change: make(option),
        });
    }
    segments
}

fn segmented(segments: Vec<Segment>, look: &Look, cx: &mut Context<WindowView>) -> Div {
    let mut track = div()
        .flex()
        .flex_none()
        .p(px(TRACK_PADDING))
        .rounded(px(TRACK_RADIUS))
        .bg(look.strong_veil);
    for segment in segments {
        let id = SharedString::from(format!("segment:{}", segment.label));
        let mut shown = div()
            .id(ElementId::Name(id))
            .px(px(SEGMENT_PADDING_X))
            .py(px(SEGMENT_PADDING_Y))
            .rounded(px(SEGMENT_RADIUS))
            .border_1()
            .border_color(transparent_black())
            .text_color(look.secondary_text)
            .child(segment.label);
        if segment.chosen {
            let outline = shell_outline(SHELL_OUTLINE, SEGMENT_RADIUS, SHELL_ANGLE);
            shown = shown
                .relative()
                .bg(look.selected_card)
                .shadow(look.card_shadow())
                .text_color(look.text)
                .font_weight(FontWeight::SEMIBOLD)
                .child(outline);
        } else {
            let change = segment.change;
            shown = shown
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, _, _| {
                    view.change_setting(change.clone());
                }));
        }
        track = track.child(shown);
    }
    track
}

fn switch(
    id: &'static str,
    on: bool,
    make: fn(bool) -> SettingChange,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let mut track_color: Background = look.strong_veil.into();
    let mut border_color = look.hairline;
    let mut knob_color = look.secondary_text;
    let mut knob_left = KNOB_LEFT_OFF;
    if on {
        track_color = shell_gradient(SHELL_ANGLE);
        border_color = transparent_black();
        knob_color = white();
        knob_left = KNOB_LEFT_ON;
    }
    let knob = div()
        .absolute()
        .top(px(KNOB_TOP))
        .left(px(knob_left))
        .size(px(KNOB_SIZE))
        .rounded_full()
        .bg(knob_color);
    let opposite = !on;
    let track = div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(SWITCH_WIDTH))
        .h(px(SWITCH_HEIGHT))
        .rounded_full()
        .border_1()
        .border_color(border_color)
        .bg(track_color)
        .cursor_pointer()
        .child(knob)
        .on_click(cx.listener(move |view, _, _, _| view.change_setting(make(opposite))));
    div().flex_none().child(track)
}

#[allow(clippy::too_many_arguments)]
fn stepper(
    id: &'static str,
    value: f64,
    step: f64,
    min: f64,
    max: Option<f64>,
    make: fn(f64) -> SettingChange,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let lower = stepped(value, -step, min, max);
    let higher = stepped(value, step, min, max);
    let minus_id = SharedString::from(format!("{id}-minus"));
    let plus_id = SharedString::from(format!("{id}-plus"));
    let minus = stepper_button(minus_id, "−", lower, make, look, cx);
    let plus = stepper_button(plus_id, "+", higher, make, look, cx);
    let shown = stepper_text(value);
    let number = div()
        .min_w(px(STEPPER_VALUE_MIN_WIDTH))
        .flex()
        .justify_center()
        .child(shown);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(STEPPER_GAP))
        .child(minus)
        .child(number)
        .child(plus)
}

fn stepper_button(
    id: SharedString,
    glyph: &'static str,
    target: Option<f64>,
    make: fn(f64) -> SettingChange,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Stateful<Div> {
    let mut button = div()
        .id(ElementId::Name(id))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(STEPPER_BUTTON_SIZE))
        .rounded(px(STEPPER_RADIUS))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .child(glyph);
    match target {
        Some(target) => {
            button = button
                .cursor_pointer()
                .on_click(cx.listener(move |view, _, _, _| {
                    view.change_setting(make(target));
                }));
        }
        None => {
            button = button.opacity(STEPPER_DISABLED_OPACITY);
        }
    }
    button
}

fn stepper_text(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

fn stepped(value: f64, by: f64, min: f64, max: Option<f64>) -> Option<f64> {
    let next = value + by;
    if next < min {
        return None;
    }
    if let Some(max) = max
        && next > max
    {
        return None;
    }
    Some(next)
}

fn typed_field(field: &Entity<TextField>) -> Div {
    div()
        .font_family(FIELD_FONT)
        .text_size(px(SMALL_TEXT_SIZE))
        .child(field.clone())
}

impl TypedSetting {
    pub fn key(self) -> &'static str {
        let named = current_setting(self, &Settings::default());
        named.key()
    }
}

/// What a typed value comes to once Enter is pressed or the field is left.
pub fn typed(setting: TypedSetting, text: &str, settings: &Settings) -> Typed {
    let trimmed = text.trim();
    let change = match setting {
        TypedSetting::Font => SettingChange::FontFamily(trimmed.to_string()),
        TypedSetting::Scrollback => {
            let parsed = trimmed.parse::<u32>();
            let Ok(lines) = parsed else {
                return Typed::NotWhole;
            };
            SettingChange::Scrollback(lines)
        }
    };
    let current = current_setting(setting, settings);
    if change == current {
        return Typed::Unchanged;
    }
    Typed::Change(change)
}

fn current_setting(setting: TypedSetting, settings: &Settings) -> SettingChange {
    match setting {
        TypedSetting::Font => SettingChange::FontFamily(settings.font_family.clone()),
        TypedSetting::Scrollback => SettingChange::Scrollback(settings.scrollback),
    }
}

/// The text a field shows for the setting as the file has it.
pub fn published_text(setting: TypedSetting, settings: &Settings) -> String {
    match setting {
        TypedSetting::Font => settings.font_family.clone(),
        TypedSetting::Scrollback => settings.scrollback.to_string(),
    }
}

fn open_file_button(id: &'static str, look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let veil = look.veil;
    div()
        .id(id)
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
        .on_click(cx.listener(|view, _, _, cx| view.open_settings_file(cx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_is_sent_only_as_a_whole_number_and_nothing_is_sent_when_nothing_changed() {
        let settings = Settings::default();
        let padded_zeros = format!("00{}", settings.scrollback);
        let padded_font = format!("  {}  ", settings.font_family);
        let not_whole = typed(TypedSetting::Scrollback, "12.5", &settings);
        assert_eq!(not_whole, Typed::NotWhole);
        let more = typed(TypedSetting::Scrollback, " 5000 ", &settings);
        assert_eq!(more, Typed::Change(SettingChange::Scrollback(5000)));
        let same_lines = typed(TypedSetting::Scrollback, &padded_zeros, &settings);
        assert_eq!(same_lines, Typed::Unchanged);
        let same_font = typed(TypedSetting::Font, &padded_font, &settings);
        assert_eq!(same_font, Typed::Unchanged);
    }

    #[test]
    fn a_setting_the_file_could_not_give_cannot_be_changed_and_a_broken_file_locks_them_all() {
        let one = UnreadSettings::Keys(vec!["fontSize"]);
        assert!(is_unread(&one, "fontSize"));
        assert!(!is_unread(&one, "theme"));
        assert!(is_unread(&UnreadSettings::Whole, "theme"));
        assert!(!is_unread(&UnreadSettings::None, "fontSize"));
    }

    #[test]
    fn no_command_closes_a_terminal_behind_the_page_or_types_into_it() {
        let cases = [
            (CommandId::TerminalClose, OverPage::ClosePage),
            (CommandId::ConfigOpenSettings, OverPage::ToggleSettings),
            (CommandId::TerminalNew, OverPage::ShowTerminal),
            (CommandId::TerminalFocusNextGroup, OverPage::ShowTerminal),
            (CommandId::ConfigOpenKeybindings, OverPage::ShowTerminal),
            (CommandId::SidebarToggle, OverPage::Run),
            (CommandId::TerminalPaste, OverPage::PassOn),
            (CommandId::TerminalKillLine, OverPage::PassOn),
        ];
        for (command, expected) in cases {
            assert_eq!(over_page(command), expected, "{command:?}");
        }
    }

    #[test]
    fn stepper_reaches_its_bounds_and_stops_there() {
        let below_max = FONT_SIZE_MAX - 1.0;
        let above_min = FONT_SIZE_MIN + 1.0;
        let up_to_max = stepped(below_max, 1.0, FONT_SIZE_MIN, Some(FONT_SIZE_MAX));
        let past_max = stepped(FONT_SIZE_MAX, 1.0, FONT_SIZE_MIN, Some(FONT_SIZE_MAX));
        let down_to_min = stepped(above_min, -1.0, FONT_SIZE_MIN, Some(FONT_SIZE_MAX));
        let past_min = stepped(FONT_SIZE_MIN, -1.0, FONT_SIZE_MIN, Some(FONT_SIZE_MAX));
        assert_eq!(up_to_max, Some(FONT_SIZE_MAX));
        assert_eq!(past_max, None);
        assert_eq!(down_to_min, Some(FONT_SIZE_MIN));
        assert_eq!(past_min, None);
        assert_eq!(stepped(0.5, 1.0, SCROLL_SPEED_MIN, None), Some(1.5));
    }
}
