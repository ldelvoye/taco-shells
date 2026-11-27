use gpui::{
    AnyElement, App, Div, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, Window, div, img, px,
};
use taco_identity::CURRENT;
use taco_protocol::DrawnState;
use taco_rules::{selected_terminal, waiting_count};

use crate::assets::MARK;
use crate::look::{Look, SMALL_TEXT_SIZE, TITLE_BAR_HEIGHT};
use crate::platform::{
    THIS_MACHINE, TRAFFIC_LIGHT_DIAMETER, TrafficLights, traffic_light_centers, traffic_lights_room,
};

const EDGE_PADDING: f32 = 12.;
const MARK_HEIGHT: f32 = 14.;
// The mark's view box is 800 by 478.
const MARK_WIDTH: f32 = MARK_HEIGHT * 800. / 478.;

/// The strip across the top: the mark and name, where the selected terminal is, and how many agents wait.
pub fn title_bar(
    drawn: &DrawnState,
    look: &Look,
    full_screen: bool,
    traffic_lights: TrafficLights,
) -> Div {
    let room = traffic_lights_room(full_screen);
    let start = room.max(EDGE_PADDING);
    let brand = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(7.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(13.))
        .child(img(MARK).w(px(MARK_WIDTH)).h(px(MARK_HEIGHT)))
        .child(CURRENT.application_name);

    let mut place = div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_center()
        .text_color(look.secondary_text);
    if let Some(line) = place_line(drawn) {
        place = place.child(line);
    }

    let mut bar = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(12.))
        .h(px(TITLE_BAR_HEIGHT))
        .pl(px(start))
        .pr(px(EDGE_PADDING))
        .border_b_1()
        .border_color(look.hairline)
        .on_mouse_down(MouseButton::Left, move_or_zoom)
        .child(brand)
        .child(place)
        .child(waiting_pill(drawn, look));
    if traffic_lights == TrafficLights::StandIns && !full_screen {
        bar = bar.children(traffic_light_stand_ins(look));
    }
    bar
}

fn move_or_zoom(event: &MouseDownEvent, window: &mut Window, _cx: &mut App) {
    if event.click_count == 2 {
        window.titlebar_double_click();
        return;
    }
    window.start_window_move();
}

/// The selected terminal's directory and machine: "~/code/sentry · this Mac".
fn place_line(drawn: &DrawnState) -> Option<String> {
    let selected = selected_terminal(drawn.arrangement.as_ref())?;
    let directory = drawn.directories.get(selected)?;
    let Some(home) = std::env::home_dir() else {
        return Some(format!("{directory} · {THIS_MACHINE}"));
    };
    let home_text = home.to_string_lossy();
    let shown = with_home_as_tilde(directory, &home_text);
    Some(format!("{shown} · {THIS_MACHINE}"))
}

fn with_home_as_tilde(directory: &str, home: &str) -> String {
    let Some(rest) = directory.strip_prefix(home) else {
        return directory.to_string();
    };
    let is_inside = rest.is_empty() || rest.starts_with('/');
    if !is_inside {
        return directory.to_string();
    }
    format!("~{rest}")
}

fn waiting_pill(drawn: &DrawnState, look: &Look) -> AnyElement {
    let count = waiting_count(&drawn.agent_rows);
    if count == 0 {
        return div().flex_none().into_any_element();
    }
    let dot = div().size(px(7.)).rounded_full().bg(look.needs);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(1.))
        .border_1()
        .border_color(look.needs)
        .rounded_full()
        .text_size(px(SMALL_TEXT_SIZE))
        .whitespace_nowrap()
        .child(dot)
        .child(format!("{count} waiting"))
        .into_any_element()
}

fn traffic_light_stand_ins(look: &Look) -> Vec<Div> {
    let radius = TRAFFIC_LIGHT_DIAMETER / 2.;
    let centers = traffic_light_centers();
    let lights = centers.into_iter().map(|center| {
        div()
            .absolute()
            .left(center.x - px(radius))
            .top(center.y - px(radius))
            .size(px(TRAFFIC_LIGHT_DIAMETER))
            .rounded_full()
            .bg(look.strong_veil)
    });
    lights.collect()
}

#[cfg(test)]
mod tests {
    use super::with_home_as_tilde;

    #[test]
    fn only_the_home_folder_and_what_is_inside_it_become_a_tilde() {
        let home = "/Users/lucas";
        let cases = [
            ("/Users/lucas", "~"),
            ("/Users/lucas/code/sentry", "~/code/sentry"),
            ("/Users/lucasx/code", "/Users/lucasx/code"),
            ("/workspace/sentry", "/workspace/sentry"),
        ];
        for (directory, shown) in cases {
            assert_eq!(with_home_as_tilde(directory, home), shown, "{directory}");
        }
    }
}
