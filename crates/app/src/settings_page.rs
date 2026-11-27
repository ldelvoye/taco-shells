use std::collections::HashMap;

use gpui::{
    Bounds, Context, Div, ElementId, Entity, FontWeight, InteractiveElement, ParentElement, Pixels,
    SharedString, Stateful, StatefulInteractiveElement, Styled, div, px, transparent_black, white,
};
use taco_identity::CURRENT;
use taco_settings::{
    Binding, CommandId, CursorStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, SettingChange, Settings,
    ShellRowName, ThemeChoice, UnreadSettings,
};

use crate::drive::mark;
use crate::look::{Look, SMALL_TEXT_SIZE};
use crate::motion::{
    Motion, Moved, SEGMENT_TIME, SWITCH_TIME, card_name, lerp, recorder, switch_amount,
};
use crate::page::{
    CHIP_GAP, PageMove, ROW_GAP, ROW_PADDING, UNREAD_OPACITY, chip, form_column, group, page_foot,
    page_frame, page_header, text_button,
};
use crate::parts;
use crate::shell::{shell_orange, shell_outline};
use crate::shortcuts_page::chords_for;
use crate::text_field::TextField;
use crate::window::WindowView;

const SCROLL_ID: &str = "settings-scroll";
const SCROLLBACK_MARK: &str = "settings:scrollback";
const SHELL_OUTLINE: f32 = 1.5;
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

/// Where a sliding card is drawn, in points from its track's corner.
#[derive(Clone, Copy)]
struct CardPlace {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
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

fn segmented(segments: Vec<Segment>, input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let bounds = input.motion.bounds;
    let track_name = track_name(&segments);
    let slide = card_slide(&segments, &track_name, input);
    let mut card_recorded_as = None;
    if let Some(chosen) = chosen_segment(&segments) {
        let key = chosen.change.key();
        card_recorded_as = Some(card_name(key));
    }
    let mut track = div()
        .relative()
        .flex()
        .flex_none()
        .p(px(TRACK_PADDING))
        .rounded(px(TRACK_RADIUS))
        .bg(look.strong_veil)
        .child(recorder(bounds, track_name));
    // While sliding, the raised card is drawn once behind the labels, not on the chosen segment.
    if let Some(card) = slide {
        let mut shown_card = raised_card(look)
            .absolute()
            .left(px(card.left))
            .top(px(card.top))
            .w(px(card.width))
            .h(px(card.height));
        if let Some(name) = card_recorded_as.clone() {
            shown_card = shown_card.child(recorder(bounds, name));
        }
        track = track.child(shown_card);
    }
    for segment in segments {
        let name = segment_name(segment.label);
        let id = name.clone();
        let mut shown = div()
            .id(ElementId::Name(id))
            .relative()
            .px(px(SEGMENT_PADDING_X))
            .py(px(SEGMENT_PADDING_Y))
            .rounded(px(SEGMENT_RADIUS))
            .border_1()
            .border_color(transparent_black())
            .text_color(look.secondary_text)
            .child(segment.label)
            .child(recorder(bounds, name));
        if segment.chosen && slide.is_none() {
            let outline = shell_outline(SHELL_OUTLINE, SEGMENT_RADIUS);
            shown = shown
                .bg(look.selected_card)
                .shadow(look.card_shadow())
                .text_color(look.text)
                .font_weight(FontWeight::SEMIBOLD)
                .child(outline);
            if let Some(name) = card_recorded_as.clone() {
                shown = shown.child(recorder(bounds, name));
            }
        } else if segment.chosen {
            shown = shown
                .text_color(look.text)
                .font_weight(FontWeight::SEMIBOLD);
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

fn track_name(segments: &[Segment]) -> SharedString {
    let mut first = "";
    if let Some(segment) = segments.first() {
        first = segment.label;
    }
    SharedString::from(format!("track:{first}"))
}

fn segment_name(label: &str) -> SharedString {
    SharedString::from(format!("segment:{label}"))
}

fn raised_card(look: &Look) -> Div {
    let outline = shell_outline(SHELL_OUTLINE, SEGMENT_RADIUS);
    div()
        .rounded(px(SEGMENT_RADIUS))
        .bg(look.selected_card)
        .shadow(look.card_shadow())
        .child(outline)
}

fn chosen_segment(segments: &[Segment]) -> Option<&Segment> {
    let mut all = segments.iter();
    all.find(|segment| segment.chosen)
}

/// Where the chosen card is drawn mid-slide, within its track; none once it has arrived or before both ends are drawn.
fn card_slide(
    segments: &[Segment],
    track_name: &SharedString,
    input: &SettingsInput,
) -> Option<CardPlace> {
    let chosen = chosen_segment(segments)?;
    let key = chosen.change.key();
    let (moved, done) = input.motion.moving(key, SEGMENT_TIME)?;
    let table = input.motion.bounds.borrow();
    let track = table.get(track_name)?;
    let start = slide_start(segments, moved, &table)?;
    let end_name = segment_name(chosen.label);
    let end = table.get(&end_name)?;
    let start_offset = start.left() - track.left();
    let end_offset = end.left() - track.left();
    let top_offset = end.top() - track.top();
    let start_left = f32::from(start_offset);
    let end_left = f32::from(end_offset);
    let start_width = f32::from(start.size.width);
    let end_width = f32::from(end.size.width);
    let place = CardPlace {
        left: lerp(start_left, end_left, done),
        top: f32::from(top_offset),
        width: lerp(start_width, end_width, done),
        height: f32::from(end.size.height),
    };
    Some(place)
}

// A move that began part way through another starts where the card was drawn, so it never jumps back.
fn slide_start(
    segments: &[Segment],
    moved: &Moved,
    table: &HashMap<SharedString, Bounds<Pixels>>,
) -> Option<Bounds<Pixels>> {
    if let Some(drawn) = moved.start_bounds {
        return Some(drawn);
    }
    for segment in segments {
        if segment.change == moved.from {
            let name = segment_name(segment.label);
            return table.get(&name).copied();
        }
    }
    None
}

fn switch(
    id: &'static str,
    on: bool,
    make: fn(bool) -> SettingChange,
    input: &SettingsInput,
    cx: &mut Context<WindowView>,
) -> Div {
    let look = input.look;
    let shown = make(on);
    let mut on_amount = switch_amount(on, None, 1.);
    if let Some((moved, done)) = input.motion.moving(shown.key(), SWITCH_TIME) {
        on_amount = switch_amount(on, moved.start_amount, done);
    }
    let knob_left = lerp(KNOB_LEFT_OFF, KNOB_LEFT_ON, on_amount);
    let off_amount = 1. - on_amount;
    let border = look.hairline.opacity(off_amount);
    let orange = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .rounded_full()
        .bg(shell_orange())
        .opacity(on_amount);
    let white_knob = div()
        .size_full()
        .rounded_full()
        .bg(white())
        .opacity(on_amount);
    let knob = div()
        .absolute()
        .top(px(KNOB_TOP))
        .left(px(knob_left))
        .size(px(KNOB_SIZE))
        .rounded_full()
        .bg(look.secondary_text)
        .child(white_knob);
    let opposite = !on;
    let track = div()
        .id(id)
        .relative()
        .flex_none()
        .w(px(SWITCH_WIDTH))
        .h(px(SWITCH_HEIGHT))
        .rounded_full()
        .border_1()
        .border_color(border)
        .bg(look.strong_veil)
        .cursor_pointer()
        .child(orange)
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
