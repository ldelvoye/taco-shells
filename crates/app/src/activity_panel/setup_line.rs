//! The agent setup line: which one a terminal shows, and the card it is drawn in beside an agent.

use gpui::{
    Context, Div, FontWeight, InteractiveElement, ParentElement, Stateful,
    StatefulInteractiveElement, Styled, div, px, svg,
};
use taco_protocol::{HarnessId, TerminalId};
use taco_services::{AgentMenuAction, AgentSetupNotice, AgentSetupNoticeKind};

use super::{ActivityInput, card};
use crate::assets::{CLOSE_ICON, DONE_ICON};
use crate::look::{Look, MARK_SIZE, SMALL_TEXT_SIZE};
use crate::window::WindowView;

const SHELL_TEXT: &str = "No agent here, just a shell.";
const NO_TERMINAL_TEXT: &str = "No terminal is open.";
const SET_UP_LABEL: &str = "Set up";
const CLOSE_SIZE: f32 = 14.;
const DONE_LINE_HEIGHT: f32 = 18.;
const DONE_CHECK_GAP: f32 = 7.;

/// The agent setup line for a terminal.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NoAgentLine {
    /// No terminal is open, after the last one closed.
    NoTerminal,
    Shell,
    Offer {
        harness: HarnessId,
        action: AgentMenuAction,
        text: String,
    },
    Unavailable {
        text: String,
        reason: String,
    },
    Done {
        harness: HarnessId,
        text: String,
        detail: Option<String>,
    },
    Problem {
        harness: HarnessId,
        text: String,
    },
}

pub(super) fn no_agent_line(terminal: &TerminalId, notices: &[AgentSetupNotice]) -> NoAgentLine {
    let mut about_terminal = notices
        .iter()
        .filter(|notice| notice.terminals.contains(terminal));
    let Some(notice) = about_terminal.next() else {
        return NoAgentLine::Shell;
    };
    let harness = notice.harness.clone();
    let text = notice.text.clone();
    match &notice.kind {
        AgentSetupNoticeKind::Offer { action } => NoAgentLine::Offer {
            harness,
            action: *action,
            text,
        },
        AgentSetupNoticeKind::Unavailable { reason } => {
            let reason = reason.clone();
            NoAgentLine::Unavailable { text, reason }
        }
        AgentSetupNoticeKind::Done => {
            let detail = notice.detail.clone();
            NoAgentLine::Done {
                harness,
                text,
                detail,
            }
        }
        AgentSetupNoticeKind::Problem => NoAgentLine::Problem { harness, text },
    }
}

pub(super) fn no_agent_for(input: &ActivityInput) -> NoAgentLine {
    match input.terminal {
        Some(terminal) => no_agent_line(terminal, input.notices),
        None => NoAgentLine::NoTerminal,
    }
}

// A row becomes an agent row from its title alone, hooks or not, so the offer has to draw beside one: that is the
// terminal it is for.
pub(super) fn shows_beside_agent(line: &NoAgentLine) -> bool {
    !matches!(line, NoAgentLine::Shell | NoAgentLine::NoTerminal)
}

pub(super) fn no_agent_element(
    line: &NoAgentLine,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let row = div().flex().items_center().gap(px(10.));
    match line {
        NoAgentLine::NoTerminal => div()
            .text_color(look.secondary_text)
            .child(NO_TERMINAL_TEXT),
        NoAgentLine::Shell => div().text_color(look.secondary_text).child(SHELL_TEXT),
        NoAgentLine::Offer {
            harness,
            action,
            text,
        } => {
            let shown = div().min_w_0().child(text.clone());
            let button = set_up_button(harness, *action, look, cx);
            div()
                .flex()
                .flex_col()
                .items_start()
                .gap(px(8.))
                .child(shown)
                .child(button)
        }
        NoAgentLine::Unavailable { text, reason } => {
            let shown = div().min_w_0().child(text.clone());
            let why = div()
                .min_w_0()
                .text_color(look.secondary_text)
                .child(reason.clone());
            div().flex().flex_col().gap(px(4.)).child(shown).child(why)
        }
        NoAgentLine::Done {
            harness,
            text,
            detail,
        } => {
            let said = done_said(text, detail.as_deref(), look);
            let dismiss = dismiss_button(harness, look, cx);
            row.items_start().child(said).child(dismiss)
        }
        NoAgentLine::Problem { harness, text } => {
            let shown = text_cell(text);
            let dismiss = dismiss_button(harness, look, cx);
            row.child(shown).child(dismiss)
        }
    }
}

// The check sits in a box one headline line tall, so it stays on the first line when the headline wraps.
fn done_said(text: &str, detail: Option<&str>, look: &Look) -> Div {
    let check = svg()
        .path(DONE_ICON)
        .size(px(MARK_SIZE))
        .text_color(look.done);
    let check_box = div()
        .flex_none()
        .h(px(DONE_LINE_HEIGHT))
        .flex()
        .items_center()
        .child(check);
    let words = div().min_w_0().child(text.to_string());
    let headline = div()
        .flex()
        .items_start()
        .gap(px(DONE_CHECK_GAP))
        .line_height(px(DONE_LINE_HEIGHT))
        .font_weight(FontWeight::SEMIBOLD)
        .child(check_box)
        .child(words);
    let mut said = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(headline);
    if let Some(detail) = detail {
        let quiet = div()
            .min_w_0()
            .text_size(px(SMALL_TEXT_SIZE))
            .text_color(look.secondary_text)
            .child(detail.to_string());
        said = said.child(quiet);
    }
    said
}

pub(super) fn setup_card(line: &NoAgentLine, look: &Look, cx: &mut Context<WindowView>) -> Div {
    let said = no_agent_element(line, look, cx);
    card(look).child(said)
}

fn text_cell(text: &str) -> Div {
    div().flex_1().min_w_0().child(text.to_string())
}

fn set_up_button(
    harness: &HarnessId,
    action: AgentMenuAction,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Stateful<Div> {
    let chosen = harness.clone();
    div()
        .id("activity-set-up")
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(6.))
        .border_1()
        .border_color(look.hairline)
        .bg(look.selected_card)
        .cursor_pointer()
        .font_weight(FontWeight::SEMIBOLD)
        .child(SET_UP_LABEL)
        .on_click(cx.listener(move |view, _, _, _| view.set_up(&chosen, action)))
}

fn dismiss_button(harness: &HarnessId, look: &Look, cx: &mut Context<WindowView>) -> Stateful<Div> {
    let dismissed = harness.clone();
    let veil = look.veil;
    let cross = svg()
        .path(CLOSE_ICON)
        .size(px(CLOSE_SIZE))
        .flex_none()
        .text_color(look.secondary_text);
    div()
        .id("activity-dismiss")
        .flex_none()
        .p(px(2.))
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |style| style.bg(veil))
        .child(cross)
        .on_click(cx.listener(move |view, _, _, _| view.dismiss_notice(&dismissed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(kind: AgentSetupNoticeKind, terminal: &str) -> AgentSetupNotice {
        AgentSetupNotice {
            harness: HarnessId("claude".to_string()),
            kind,
            text: "Set up Claude?".to_string(),
            detail: None,
            terminals: vec![TerminalId(terminal.to_string())],
        }
    }

    #[test]
    fn no_agent_line_follows_the_notice_for_this_terminal() {
        let here = TerminalId("here".to_string());
        let reason = "Turned off".to_string();
        let unavailable = AgentSetupNoticeKind::Unavailable {
            reason: reason.clone(),
        };
        let harness = HarnessId("claude".to_string());
        let text = "Set up Claude?".to_string();

        assert_eq!(no_agent_line(&here, &[]), NoAgentLine::Shell);
        let offer_kind = AgentSetupNoticeKind::Offer {
            action: AgentMenuAction::SetUp,
        };
        let elsewhere = notice(offer_kind.clone(), "there");
        assert_eq!(no_agent_line(&here, &[elsewhere]), NoAgentLine::Shell);

        let offer = notice(offer_kind, "here");
        let expected = NoAgentLine::Offer {
            harness: harness.clone(),
            action: AgentMenuAction::SetUp,
            text: text.clone(),
        };
        assert_eq!(no_agent_line(&here, &[offer]), expected);
        assert!(shows_beside_agent(&expected));
        assert!(!shows_beside_agent(&NoAgentLine::Shell));

        let unavailable = notice(unavailable, "here");
        let expected = NoAgentLine::Unavailable {
            text: text.clone(),
            reason,
        };
        assert_eq!(no_agent_line(&here, &[unavailable]), expected);
        assert!(shows_beside_agent(&expected));

        let detail = Some("One line changed in ~/.claude/settings.json.".to_string());
        let mut done = notice(AgentSetupNoticeKind::Done, "here");
        done.detail = detail.clone();
        let expected = NoAgentLine::Done {
            harness: harness.clone(),
            text: text.clone(),
            detail,
        };
        assert_eq!(no_agent_line(&here, &[done]), expected);
        assert!(shows_beside_agent(&expected));

        let problem = notice(AgentSetupNoticeKind::Problem, "here");
        let expected = NoAgentLine::Problem {
            harness: harness.clone(),
            text: text.clone(),
        };
        assert_eq!(no_agent_line(&here, &[problem]), expected);
        assert!(shows_beside_agent(&expected));
    }
}
