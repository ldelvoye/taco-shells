//! The parts the pages share: the frame, header, column, groups, buttons, foot and key chips.

use gpui::{
    Context, Div, FontWeight, Hsla, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px,
};
use taco_settings::Chord;

use crate::drive::mark;
use crate::look::{FONT_FAMILY, Look, SMALL_TEXT_SIZE, TEXT_SIZE, TITLE_BAR_HEIGHT};
use crate::page::PageMove;
use crate::window::WindowView;

pub const ROW_PADDING: f32 = 7.;
pub const ROW_GAP: f32 = 16.;
pub const BUTTON_RADIUS: f32 = 6.;
pub const BUTTON_PADDING_Y: f32 = 3.;
pub const BUTTON_PADDING_X: f32 = 9.;
pub const CHIP_GAP: f32 = 5.;
pub const UNREAD_OPACITY: f32 = 0.45;
const PAGE_COLUMN_MARK: &str = "page:column";
const HEADER_PADDING_LEFT: f32 = 16.;
const HEADER_PADDING_RIGHT: f32 = 10.;
const HEADER_BUTTON_SIZE: f32 = 22.;
const HEADER_GLYPH_SIZE: f32 = 15.;
const TITLE_SIZE: f32 = 13.;
const BACK_GAP: f32 = 4.;
const FORM_MAX_WIDTH: f32 = 600.;
const FORM_PADDING_TOP: f32 = 20.;
const FORM_PADDING_SIDES: f32 = 24.;
const FORM_PADDING_BOTTOM: f32 = 28.;
const FORM_GAP: f32 = 22.;
const GROUP_GAP: f32 = 2.;
const HEADING_SIZE: f32 = 10.5;
const HEADING_GAP: f32 = 4.;
const FOOT_GAP: f32 = 12.;
const FOOT_PADDING_TOP: f32 = 10.;
const CHIP_HEIGHT: f32 = 24.;
const CHIP_PADDING: f32 = 6.;
const CHIP_RADIUS: f32 = 6.;
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

/// A page's header above its scrolling form; each page scrolls on its own id, so one never opens at the other's offset.
pub fn page_frame(scroll_id: &'static str, header: Div, form: Div, look: &Look) -> Div {
    let marked_form = mark(form, PAGE_COLUMN_MARK);
    let body = div()
        .id(scroll_id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(marked_form);
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

/// The title, with "‹ back" before it on a page under another, and × to close every page.
pub fn page_header(
    title: &'static str,
    back: Option<&'static str>,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let mut header = div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(TITLE_BAR_HEIGHT))
        .pl(px(HEADER_PADDING_LEFT))
        .pr(px(HEADER_PADDING_RIGHT))
        .border_b_1()
        .border_color(look.hairline);
    if let Some(back) = back {
        let label = format!("‹ {back}");
        let hover_background = look.strong_veil;
        let hover_text = look.text;
        let back = div()
            .id("page-back")
            .flex_none()
            .px(px(BUTTON_PADDING_X))
            .py(px(BUTTON_PADDING_Y))
            .mr(px(BACK_GAP))
            .rounded(px(BUTTON_RADIUS))
            .cursor_pointer()
            .text_color(look.secondary_text)
            .hover(move |style| style.bg(hover_background).text_color(hover_text))
            .child(label)
            .on_click(cx.listener(|view, _, _, cx| view.move_pages(PageMove::Back, cx)));
        header = header.child(back);
    }
    let title = div()
        .text_size(px(TITLE_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .child(title);
    let gap = div().flex_1();
    let close = header_button("page-close", "×", look.strong_veil, look.text, look);
    let close =
        close.on_click(cx.listener(|view, _, _, cx| view.move_pages(PageMove::CloseAll, cx)));
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

/// The centred column a page's groups stack in.
pub fn form_column() -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(FORM_GAP))
        .w_full()
        .max_w(px(FORM_MAX_WIDTH))
        .mx_auto()
        .pt(px(FORM_PADDING_TOP))
        .px(px(FORM_PADDING_SIDES))
        .pb(px(FORM_PADDING_BOTTOM))
}

pub fn group(heading: &str, look: &Look) -> Div {
    let shout = heading.to_uppercase();
    let title = div()
        .pb(px(HEADING_GAP))
        .text_size(px(HEADING_SIZE))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(look.secondary_text)
        .child(shout);
    div().flex().flex_col().gap(px(GROUP_GAP)).child(title)
}

/// A bordered button with a word on it, as the foot's "Open settings.json".
pub fn text_button(id: &'static str, label: &'static str, look: &Look) -> Stateful<Div> {
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
        .child(label)
}

/// The line under a page's groups: where changes go, and a button for the file.
pub fn page_foot(text: &'static str, button: Stateful<Div>, look: &Look) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(FOOT_GAP))
        .pt(px(FOOT_PADDING_TOP))
        .border_t_1()
        .border_color(look.hairline)
        .text_size(px(SMALL_TEXT_SIZE))
        .text_color(look.secondary_text)
        .child(text)
        .child(button)
}

/// A shortcut as a small key cap with the Mac's symbols.
pub fn chip(chord: &Chord, look: &Look) -> Div {
    let label = chip_text(chord);
    div()
        .flex()
        .flex_none()
        .items_center()
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
        None => {
            let upper = chord.key.to_uppercase();
            text.push_str(&upper);
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use taco_settings::parse_chord;

    use super::*;

    #[test]
    fn a_chip_reads_in_the_macs_symbols() {
        let cases = [
            ("cmd+shift+,", "⌘⇧,"),
            ("opt+left", "⌥←"),
            ("ctrl+opt+shift+k", "⌃⌥⇧K"),
        ];
        for (written, shown) in cases {
            let chord = parse_chord(written).expect(written);
            assert_eq!(chip_text(&chord), shown, "{written}");
        }
    }
}
