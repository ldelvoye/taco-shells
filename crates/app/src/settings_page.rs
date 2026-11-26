use std::collections::HashMap;

use gpui::{
    Context, Div, ElementId, FontWeight, Hsla, InteractiveElement, ParentElement, SharedString,
    Stateful, StatefulInteractiveElement, Styled, div, px, transparent_black,
};
use taco_settings::{
    CommandId, CursorStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, SettingChange, Settings, ShellRowName,
    ThemeChoice, UnreadSettings,
};

use crate::look::{FONT_FAMILY, Look, SMALL_TEXT_SIZE, TEXT_SIZE, TITLE_BAR_HEIGHT};
use crate::window::WindowView;

const SCROLL_ID: &str = "settings-scroll";
const TITLE: &str = "Settings";
const OPEN_FILE_LABEL: &str = "Open settings.json";
const FOOT_TEXT: &str = "Changes are written to settings.json and apply at once.";
const FIELD_FONT: &str = "Menlo";
const UNREAD_OPACITY: f32 = 0.45;
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
const FIELD_HEIGHT: f32 = 26.;
const FIELD_PADDING: f32 = 8.;
const FIELD_RADIUS: f32 = 6.;
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
    /// The pages open, the top one drawn; more than one shows a back arrow.
    pub depth: usize,
    /// The settings the file could not give, whose controls are drawn disabled.
    pub unread: &'a UnreadSettings,
    /// Why a setting's last write was refused, by its name in settings.json.
    pub refused: &'a HashMap<&'static str, String>,
    pub look: &'a Look,
}

struct Segment {
    label: &'static str,
    chosen: bool,
    change: Option<SettingChange>,
}

/// Nothing closes a terminal nobody can see, and anything that shows one closes the pages first.
pub fn over_page(command: CommandId) -> OverPage {
    match command {
        CommandId::TerminalClose => OverPage::ClosePage,
        CommandId::ConfigOpenSettings => OverPage::ToggleSettings,
        CommandId::TerminalNew
        | CommandId::TerminalFocusNextGroup
        | CommandId::TerminalFocusPreviousGroup => OverPage::ShowTerminal,
        CommandId::SidebarToggle | CommandId::ConfigOpenKeybindings => OverPage::Run,
        _ => OverPage::PassOn,
    }
}

pub fn settings_page(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let header = header(input.depth, look, cx);
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

fn header(depth: usize, look: &Look, cx: &mut Context<WindowView>) -> Div {
    let mut header = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(TITLE_BAR_HEIGHT))
        .pl(px(HEADER_PADDING_LEFT))
        .pr(px(HEADER_PADDING_RIGHT))
        .border_b_1()
        .border_color(look.hairline);
    if depth > 1 {
        let back = header_button("settings-back", "‹", look.veil, look.secondary_text, look);
        let back = back.on_click(cx.listener(|view, _, _, cx| view.close_page(cx)));
        header = header.child(back);
    }
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
    let theme_options = theme_segments(settings.theme);
    let theme = segmented(theme_options, look, cx);
    let font = field_box(settings.font_family.clone(), look)
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
    let theme_now = SettingChange::Theme(settings.theme);
    let font_now = SettingChange::FontFamily(settings.font_family.clone());
    let size_now = SettingChange::FontSize(settings.font_size);
    let theme_row = setting_row(&theme_now, "Theme", theme_hint, theme, input);
    let font_row = setting_row(&font_now, "Font", font_hint, font, input);
    let size_row = setting_row(&size_now, "Font size", None, size, input);
    group("Look", look)
        .child(theme_row)
        .child(font_row)
        .child(size_row)
}

fn terminal_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let settings = input.settings;
    let look = input.look;
    let cursor_options = cursor_segments(settings.cursor_style);
    let cursor = segmented(cursor_options, look, cx);
    let blink = switch(
        "cursor-blink",
        settings.cursor_blink,
        SettingChange::CursorBlink,
        look,
        cx,
    );
    let scrollback = field_box(settings.scrollback.to_string(), look).w(px(SCROLLBACK_FIELD_WIDTH));
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
    let cursor_now = SettingChange::CursorStyle(settings.cursor_style);
    let blink_now = SettingChange::CursorBlink(settings.cursor_blink);
    let scrollback_now = SettingChange::Scrollback(settings.scrollback);
    let speed_now = SettingChange::ScrollSensitivity(settings.scroll_sensitivity);
    let cursor_row = setting_row(&cursor_now, "Cursor", None, cursor, input);
    let blink_row = setting_row(&blink_now, "Cursor blinks", None, blink, input);
    let scrollback_row = setting_row(
        &scrollback_now,
        "Scrollback",
        scrollback_hint,
        scrollback,
        input,
    );
    let speed_row = setting_row(&speed_now, "Scroll speed", speed_hint, speed, input);
    group("Terminal", look)
        .child(cursor_row)
        .child(blink_row)
        .child(scrollback_row)
        .child(speed_row)
}

fn list_group(input: &SettingsInput, cx: &mut Context<WindowView>) -> Div {
    let look = input.look;
    let chosen = input.settings.shell_row_name;
    let options = [
        ("Folder name", ShellRowName::Folder),
        ("Short path", ShellRowName::Path),
    ];
    let mut segments = Vec::new();
    for (label, name) in options {
        let is_chosen = name == chosen;
        let mut change = None;
        if !is_chosen {
            change = Some(SettingChange::ShellRowName(name));
        }
        segments.push(Segment {
            label,
            chosen: is_chosen,
            change,
        });
    }
    let names = segmented(segments, look, cx);
    let hint = Some("Rows with an agent keep the agent’s title.");
    let names_now = SettingChange::ShellRowName(chosen);
    let names_row = setting_row(&names_now, "A plain shell’s row shows", hint, names, input);
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
    let restore_now = SettingChange::RestoreOnLaunch(settings.restore_on_launch);
    let restore_row = setting_row(&restore_now, "Reopen my terminals", hint, restore, input);
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
    let hooks_now = SettingChange::ManageAgentHooks(settings.manage_agent_hooks);
    let hooks_row = setting_row(&hooks_now, "Keep agent hooks set up", hint, hooks, input);
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

// A setting the file could not give is drawn at the default in use, and cannot be changed; a refused write says why where the hint is.
fn setting_row(
    setting: &SettingChange,
    label: &str,
    hint: Option<&str>,
    control: Div,
    input: &SettingsInput,
) -> Div {
    let look = input.look;
    let key = setting.key();
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

fn theme_segments(chosen: ThemeChoice) -> Vec<Segment> {
    let options = [
        ("System", ThemeChoice::System),
        ("Light", ThemeChoice::Light),
        ("Dark", ThemeChoice::Dark),
    ];
    let mut segments = Vec::new();
    for (label, theme) in options {
        segments.push(Segment {
            label,
            chosen: theme == chosen,
            change: Some(SettingChange::Theme(theme)),
        });
    }
    segments
}

fn cursor_segments(chosen: CursorStyle) -> Vec<Segment> {
    let options = [
        ("Block", CursorStyle::Block),
        ("Underline", CursorStyle::Underline),
        ("Bar", CursorStyle::Bar),
    ];
    let mut segments = Vec::new();
    for (label, style) in options {
        segments.push(Segment {
            label,
            chosen: style == chosen,
            change: Some(SettingChange::CursorStyle(style)),
        });
    }
    segments
}

fn segmented(segments: Vec<Segment>, look: &Look, cx: &mut Context<WindowView>) -> Div {
    segment_track(segments, look, Some(cx))
}

fn segment_track(
    segments: Vec<Segment>,
    look: &Look,
    mut cx: Option<&mut Context<WindowView>>,
) -> Div {
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
            shown = shown
                .bg(look.selected_card)
                .border_color(look.card_edge())
                .shadow(look.card_shadow())
                .text_color(look.text)
                .font_weight(FontWeight::SEMIBOLD);
        }
        let clickable = segment.change.filter(|_| !segment.chosen);
        if let (Some(change), Some(cx)) = (clickable, cx.as_deref_mut()) {
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
    let track_color = if on { look.text } else { look.strong_veil };
    let border_color = if on { look.text } else { look.hairline };
    let knob_color = if on { look.panels } else { look.secondary_text };
    let knob_left = if on { KNOB_LEFT_ON } else { KNOB_LEFT_OFF };
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

fn field_box(text: String, look: &Look) -> Div {
    div()
        .flex()
        .items_center()
        .h(px(FIELD_HEIGHT))
        .px(px(FIELD_PADDING))
        .rounded(px(FIELD_RADIUS))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .overflow_hidden()
        .whitespace_nowrap()
        .font_family(FIELD_FONT)
        .text_size(px(SMALL_TEXT_SIZE))
        .child(text)
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
        .on_click(cx.listener(|view, _, _, _| view.open_settings_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            (CommandId::SidebarToggle, OverPage::Run),
            (CommandId::TerminalPaste, OverPage::PassOn),
            (CommandId::TerminalKillLine, OverPage::PassOn),
        ];
        for (command, expected) in cases {
            assert_eq!(over_page(command), expected, "{command:?}");
        }
    }

    #[test]
    fn stepper_stays_inside_its_range() {
        assert_eq!(
            stepped(12.0, 1.0, FONT_SIZE_MIN, Some(FONT_SIZE_MAX)),
            Some(13.0)
        );
        assert_eq!(
            stepped(FONT_SIZE_MAX, 1.0, FONT_SIZE_MIN, Some(FONT_SIZE_MAX)),
            None
        );
        assert_eq!(stepped(1.0, -1.0, SCROLL_SPEED_MIN, None), None);
        assert_eq!(stepped(0.5, 1.0, SCROLL_SPEED_MIN, None), Some(1.5));
        assert_eq!(stepped(3.0, -1.0, SCROLL_SPEED_MIN, None), Some(2.0));
    }
}
