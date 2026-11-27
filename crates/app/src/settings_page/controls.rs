//! The Settings page's three controls: the segmented choice with its sliding card, the switch and the stepper.

use std::collections::HashMap;

use gpui::{
    Bounds, Context, Div, ElementId, FontWeight, InteractiveElement, ParentElement, Pixels,
    SharedString, Stateful, StatefulInteractiveElement, Styled, div, px, transparent_black, white,
};
use taco_settings::SettingChange;

use super::SettingsInput;
use crate::look::Look;
use crate::motion::{Moved, SEGMENT_TIME, SWITCH_TIME, card_name, lerp, recorder, switch_amount};
use crate::shell::{shell_orange, shell_outline};
use crate::window::WindowView;

const SHELL_OUTLINE: f32 = 1.5;
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

pub(super) struct Segment {
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

pub(super) fn segments_of<T: Copy + PartialEq>(
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

pub(super) fn segmented(
    segments: Vec<Segment>,
    input: &SettingsInput,
    cx: &mut Context<WindowView>,
) -> Div {
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
        let name = segment_name(&segment);
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

// Keyed by the setting's name, so two controls that share a choice's label never share a record.
fn track_name(segments: &[Segment]) -> SharedString {
    let mut key = "";
    if let Some(segment) = segments.first() {
        key = segment.change.key();
    }
    SharedString::from(format!("track:{key}"))
}

fn segment_name(segment: &Segment) -> SharedString {
    let key = segment.change.key();
    let label = segment.label;
    SharedString::from(format!("segment:{key}:{label}"))
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
    let end_name = segment_name(chosen);
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
            let name = segment_name(segment);
            return table.get(&name).copied();
        }
    }
    None
}

pub(super) fn switch(
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
pub(super) fn stepper(
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

#[cfg(test)]
mod tests {
    use taco_settings::{FONT_SIZE_MAX, FONT_SIZE_MIN};

    use super::*;
    use crate::settings_page::SCROLL_SPEED_MIN;

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
