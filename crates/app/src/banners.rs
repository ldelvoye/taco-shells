use gpui::{Div, FontWeight, ParentElement, Styled, div, px, svg};

use crate::assets::CLOSE_ICON;
use crate::look::Look;

const CARD_WIDTH: f32 = 340.;
const CLOSE_SIZE: f32 = 14.;

/// One notice the window shows over the middle: a problem, or something to act on.
#[derive(Clone, Debug, PartialEq)]
pub struct Banner {
    pub text: String,
    pub action: Option<String>,
}

/// The banners as a stack of cards in the middle's top-right corner, drawn over the terminal.
pub fn banner_cards(banners: &[Banner], look: &Look) -> Div {
    let cards = banners.iter().map(|banner| {
        let card = div()
            .w(px(CARD_WIDTH))
            .py(px(8.))
            .px(px(10.))
            .bg(look.panels)
            .border_1()
            .border_color(look.hairline)
            .rounded(px(8.))
            .shadow(look.card_shadow());
        filled(card, banner, look)
    });
    div()
        .absolute()
        .top(px(10.))
        .right(px(10.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(cards)
}

/// The banners as strips across the top of the middle, each pushing the terminal down.
pub fn banner_strips(banners: &[Banner], look: &Look) -> Vec<Div> {
    let strips = banners.iter().map(|banner| {
        let strip = div()
            .flex_none()
            .py(px(6.))
            .px(px(12.))
            .bg(look.panels)
            .border_b_1()
            .border_color(look.hairline);
        filled(strip, banner, look)
    });
    strips.collect()
}

fn filled(container: Div, banner: &Banner, look: &Look) -> Div {
    let text = div().flex_1().min_w_0().child(banner.text.clone());
    let close = svg()
        .path(CLOSE_ICON)
        .size(px(CLOSE_SIZE))
        .flex_none()
        .text_color(look.secondary_text);
    let mut line = container.flex().items_center().gap(px(10.)).child(text);
    if let Some(action) = &banner.action {
        let button = div()
            .flex_none()
            .px(px(8.))
            .py(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(look.hairline)
            .bg(look.selected_card)
            .font_weight(FontWeight::SEMIBOLD)
            .child(action.clone());
        line = line.child(button);
    }
    line.child(close)
}
