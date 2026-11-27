//! The small parts several places in the window draw the same way.

use gpui::{
    Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString, Stateful, Styled,
    div, px, svg,
};

use crate::assets::CLOSE_ICON;
use crate::look::Look;

pub const CLOSE_SIZE: f32 = 14.;
const CARD_PADDING: f32 = 10.;
const CARD_RADIUS: f32 = 8.;
const CARD_GAP: f32 = 8.;

/// An element id made of a kind and a key, as "kind:key".
pub fn element_id(kind: &str, key: impl std::fmt::Display) -> ElementId {
    let id = SharedString::from(format!("{kind}:{key}"));
    ElementId::Name(id)
}

/// A small button in a hairline border on the selected-card ground.
pub fn bordered_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    look: &Look,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .child(label)
}

/// The small × that dismisses a card or banner, with no click handler.
pub fn close_button(id: impl Into<ElementId>, look: &Look) -> Stateful<Div> {
    let veil = look.veil;
    let cross = svg()
        .path(CLOSE_ICON)
        .size(px(CLOSE_SIZE))
        .flex_none()
        .text_color(look.secondary_text);
    div()
        .id(id)
        .flex_none()
        .p(px(2.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(veil))
        .child(cross)
}

/// The card that lists a file's problems above a button to open it.
pub fn problem_card(problems: &[String], button: Stateful<Div>, look: &Look) -> Div {
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
    card.child(button)
}
