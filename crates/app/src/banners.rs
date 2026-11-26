use std::collections::HashSet;

use gpui::{
    Context, Div, ElementId, FontWeight, InteractiveElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px, svg,
};

use crate::assets::CLOSE_ICON;
use crate::look::Look;
use crate::window::WindowView;

const CARD_WIDTH: f32 = 340.;
const CLOSE_SIZE: f32 = 14.;
const RESTART_TO_UPDATE: &str = "Restart to update";

/// What the services have reported that the window shows as cards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notices {
    pub config_problems: Vec<String>,
    pub problems: Vec<String>,
    /// The version of a downloaded update waiting to be applied.
    pub update: Option<String>,
    pub update_problem: Option<String>,
}

/// What a dismissed card is remembered by, so it comes back only when its news changes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dismissal {
    /// Every config problem, as one text.
    Config(String),
    Problem(String),
    /// The ready update's version.
    Update(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerAction {
    RestartToUpdate,
}

/// One card over the middle.
#[derive(Clone, Debug, PartialEq)]
pub struct Banner {
    pub lines: Vec<String>,
    pub action: Option<BannerAction>,
    /// None for a card that stays until its cause goes, the runtime's death.
    pub dismissal: Option<Dismissal>,
}

/// The cards to show, in a fixed order: the workspace's problem, the config's, the services', then a ready update.
pub fn banners_of(
    notices: &Notices,
    workspace_problem: Option<&str>,
    dismissed: &HashSet<Dismissal>,
) -> Vec<Banner> {
    let mut banners = Vec::new();
    if let Some(problem) = workspace_problem {
        banners.push(Banner {
            lines: vec![problem.to_string()],
            action: None,
            dismissal: None,
        });
    }
    if !notices.config_problems.is_empty() {
        let signature = notices.config_problems.join("\n");
        banners.push(Banner {
            lines: notices.config_problems.clone(),
            action: None,
            dismissal: Some(Dismissal::Config(signature)),
        });
    }
    for problem in &notices.problems {
        banners.push(Banner {
            lines: vec![problem.clone()],
            action: None,
            dismissal: Some(Dismissal::Problem(problem.clone())),
        });
    }
    if let Some(version) = &notices.update {
        let problem = notices.update_problem.as_deref();
        let update = update_banner(version, problem);
        banners.push(update);
    }
    banners.retain(|banner| match &banner.dismissal {
        Some(dismissal) => !dismissed.contains(dismissal),
        None => true,
    });
    banners
}

fn update_banner(version: &str, problem: Option<&str>) -> Banner {
    let mut lines = vec![format!("Taco Shells {version} is ready.")];
    let mut action = Some(BannerAction::RestartToUpdate);
    // An update that failed to apply says why where its button was.
    if let Some(problem) = problem {
        lines.push(problem.to_string());
        action = None;
    }
    Banner {
        lines,
        action,
        dismissal: Some(Dismissal::Update(version.to_string())),
    }
}

/// The banners as a stack of cards in the middle's top-right corner, drawn over the terminal.
pub fn banner_cards(banners: &[Banner], look: &Look, cx: &mut Context<WindowView>) -> Div {
    let mut cards = Vec::new();
    for (index, banner) in banners.iter().enumerate() {
        let card = div()
            .w(px(CARD_WIDTH))
            .py(px(8.))
            .px(px(10.))
            .bg(look.panels)
            .border_1()
            .border_color(look.hairline)
            .rounded(px(8.))
            .shadow(look.card_shadow());
        cards.push(filled(card, index, banner, look, cx));
    }
    div()
        .absolute()
        .top(px(10.))
        .right(px(10.))
        .flex()
        .flex_col()
        .gap(px(6.))
        .children(cards)
}

fn filled(
    container: Div,
    index: usize,
    banner: &Banner,
    look: &Look,
    cx: &mut Context<WindowView>,
) -> Div {
    let lines = banner.lines.iter().map(|line| div().child(line.clone()));
    let text = div().flex().flex_col().flex_1().min_w_0().children(lines);
    let mut line = container.flex().items_center().gap(px(10.)).child(text);
    if let Some(BannerAction::RestartToUpdate) = banner.action {
        let button = div()
            .id(element_id("banner-action", index))
            .flex_none()
            .px(px(8.))
            .py(px(2.))
            .rounded(px(6.))
            .border_1()
            .border_color(look.hairline)
            .bg(look.selected_card)
            .cursor_pointer()
            .font_weight(FontWeight::SEMIBOLD)
            .child(RESTART_TO_UPDATE)
            .on_click(cx.listener(|view, _, _, _| view.apply_update()));
        line = line.child(button);
    }
    if let Some(dismissal) = &banner.dismissal {
        let dismissed = dismissal.clone();
        let veil = look.veil;
        let cross = svg()
            .path(CLOSE_ICON)
            .size(px(CLOSE_SIZE))
            .text_color(look.secondary_text);
        let close = div()
            .id(element_id("banner-dismiss", index))
            .flex_none()
            .p(px(2.))
            .rounded(px(6.))
            .cursor_pointer()
            .hover(move |style| style.bg(veil))
            .child(cross)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.dismiss_banner(dismissed.clone(), cx);
            }));
        line = line.child(close);
    }
    line
}

fn element_id(kind: &str, index: usize) -> ElementId {
    let id = SharedString::from(format!("{kind}:{index}"));
    ElementId::Name(id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{BannerAction, Dismissal, Notices, banners_of};

    #[test]
    fn a_dismissed_card_comes_back_only_when_its_news_changes() {
        let notices = Notices {
            config_problems: vec!["settings.json: line 4".to_string()],
            problems: vec!["Every focus port is taken.".to_string()],
            update: Some("0.27.0".to_string()),
            update_problem: None,
        };
        let dismissed = HashSet::from([
            Dismissal::Config("settings.json: line 4".to_string()),
            Dismissal::Problem("Every focus port is taken.".to_string()),
            Dismissal::Update("0.27.0".to_string()),
        ]);
        let all_gone = banners_of(&notices, Some("The runtime stopped."), &dismissed);
        let kept: Vec<_> = all_gone.iter().map(|banner| banner.lines.clone()).collect();
        assert_eq!(kept, [vec!["The runtime stopped.".to_string()]]);

        let newer = Notices {
            config_problems: vec!["settings.json: line 9".to_string()],
            update: Some("0.28.0".to_string()),
            update_problem: Some("The update could not be applied.".to_string()),
            ..notices.clone()
        };
        let back = banners_of(&newer, None, &dismissed);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].lines, ["settings.json: line 9"]);
        let update = &back[1];
        let expected_lines = [
            "Taco Shells 0.28.0 is ready.",
            "The update could not be applied.",
        ];
        assert_eq!(update.lines, expected_lines);
        assert_eq!(update.action, None);

        let fresh = banners_of(&notices, None, &HashSet::new());
        let last = fresh.last().unwrap();
        assert_eq!(last.action, Some(BannerAction::RestartToUpdate));
    }
}
