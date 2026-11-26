use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Pixels, Point, Stateful, StatefulInteractiveElement, Styled, Subscription, anchored, deferred,
    div, px,
};
use taco_protocol::{DrawnState, TabGroupId, TerminalId};
use taco_rules::can_fork;

use crate::look::{Look, ROW_RADIUS};
use crate::text_field::{TextField, TextFieldEvent};
use crate::window::WindowView;

const POPUP_WIDTH: f32 = 180.;
const ITEM_HEIGHT: f32 = 26.;
const WINDOW_MARGIN: f32 = 8.;
const UNGROUP: &str = "Ungroup";
const NEW_GROUP: &str = "New group";

/// The one popup open over the window, if any: on a row, or on a tab group's header.
pub enum Popup {
    Row {
        terminal: TerminalId,
        at: Point<Pixels>,
    },
    Group {
        tab_group: TabGroupId,
        at: Point<Pixels>,
        field: Entity<TextField>,
        /// Whether the field has yet to take the keyboard.
        needs_focus: bool,
        /// Holding it is what keeps the field's edits reaching the window.
        _edits: Subscription,
    },
}

/// What the window does with an event from the rename field.
#[derive(Debug, PartialEq)]
pub enum RenameStep {
    Rename(String),
    Close,
    /// Esc undoes: the name the popup opened with goes back, since each keystroke was already applied.
    RenameAndClose(String),
}

impl Popup {
    /// Whether the row or tab group the popup is on still exists.
    pub fn still_shown_in(&self, drawn: &DrawnState) -> bool {
        let Some(arrangement) = &drawn.arrangement else {
            return false;
        };
        match self {
            Popup::Row { terminal, .. } => {
                let mut rows = arrangement.rows.iter();
                rows.any(|row| &row.terminal == terminal)
            }
            Popup::Group { tab_group, .. } => {
                let mut tab_groups = arrangement.tab_groups.iter();
                tab_groups.any(|group| &group.id == tab_group)
            }
        }
    }
}

/// The popup card, drawn over the window next to where it was opened.
pub fn rename_step(event: &TextFieldEvent, opening_name: &str) -> RenameStep {
    match event {
        TextFieldEvent::Edited(text) => RenameStep::Rename(text.clone()),
        TextFieldEvent::Submitted => RenameStep::Close,
        TextFieldEvent::Cancelled => RenameStep::RenameAndClose(opening_name.to_string()),
    }
}

pub fn popup_layer(
    popup: &Popup,
    drawn: &DrawnState,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> AnyElement {
    let (at, contents) = match popup {
        Popup::Row { terminal, at } => (*at, row_items(terminal, *at, drawn, look, cx)),
        Popup::Group {
            tab_group,
            at,
            field,
            ..
        } => (*at, group_items(tab_group, field, look, cx)),
    };
    let card = div()
        .occlude()
        .w(px(POPUP_WIDTH))
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(4.))
        .bg(look.panels)
        .border_1()
        .border_color(look.hairline)
        .rounded(px(ROW_RADIUS))
        .shadow(look.card_shadow())
        .on_mouse_down_out(cx.listener(|view, _, _, cx| view.close_popup(cx)))
        .children(contents);
    let placed = anchored()
        .position(at)
        .snap_to_window_with_margin(px(WINDOW_MARGIN))
        .child(card);
    deferred(placed).priority(1).into_any_element()
}

fn row_items(
    terminal: &TerminalId,
    at: Point<Pixels>,
    drawn: &DrawnState,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Vec<AnyElement> {
    let forks = can_fork(&drawn.agent_rows, &drawn.agents, terminal);
    let agent_name = agent_name_of(terminal, drawn);
    let fork_label = format!("Fork with {agent_name}");
    let forked = terminal.clone();
    let mut fork = item("popup-fork", fork_label, forks, look);
    if forks {
        fork = fork.on_click(cx.listener(move |view, _, _, cx| view.fork(&forked, cx)));
    }
    let grouped = terminal.clone();
    // The group's popup opens where this one was, as the name field replaces it.
    let new_group = item("popup-new-group", NEW_GROUP.to_string(), true, look)
        .on_click(cx.listener(move |view, _, _, cx| view.new_group(&grouped, at, cx)));
    vec![fork.into_any_element(), new_group.into_any_element()]
}

fn group_items(
    tab_group: &TabGroupId,
    field: &Entity<TextField>,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Vec<AnyElement> {
    let ungrouped = tab_group.clone();
    let ungroup = item("popup-ungroup", UNGROUP.to_string(), true, look)
        .on_click(cx.listener(move |view, _, _, cx| view.ungroup(&ungrouped, cx)));
    vec![field.clone().into_any_element(), ungroup.into_any_element()]
}

fn agent_name_of(terminal: &TerminalId, drawn: &DrawnState) -> String {
    let Some(agent_row) = drawn.agent_rows.get(terminal) else {
        return drawn.default_agent_name.clone();
    };
    let Some(label) = drawn.agents.get(&agent_row.harness) else {
        return drawn.default_agent_name.clone();
    };
    label.name.clone()
}

fn item(id: &'static str, label: String, enabled: bool, look: &Look) -> Stateful<Div> {
    let mut shown = div()
        .id(id)
        .flex()
        .items_center()
        .h(px(ITEM_HEIGHT))
        .px(px(8.))
        .rounded(px(6.))
        .font_weight(FontWeight::NORMAL)
        .child(label);
    if enabled {
        shown = shown.hover(|style| style.bg(look.veil));
    } else {
        shown = shown.text_color(look.secondary_text);
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_after_several_keystrokes_puts_back_the_opening_name() {
        let typed = ["T", "Ta", "Tac"];
        let mut last_sent = String::new();
        for text in typed {
            let edited = TextFieldEvent::Edited(text.to_string());
            if let RenameStep::Rename(name) = rename_step(&edited, "Tacos") {
                last_sent = name;
            }
        }
        assert_eq!(last_sent, "Tac");
        let undone = rename_step(&TextFieldEvent::Cancelled, "Tacos");
        assert_eq!(undone, RenameStep::RenameAndClose("Tacos".to_string()));
        let kept = rename_step(&TextFieldEvent::Submitted, "Tacos");
        assert_eq!(kept, RenameStep::Close);
    }
}
