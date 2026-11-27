//! The window's activity log and shared things: what it watches, folds and opens.

use std::collections::HashSet;

use gpui::{AppContext, Context};
use taco_protocol::{
    ActivityRequest, DrawnState, HarnessId, SharedTarget, SharedThing, TerminalId,
};
use taco_rules::selected_terminal;
use taco_services::AgentMenuAction;

use super::WindowView;
use crate::activity_log::ActivityLog;
use crate::shared_view::{Content, OpenedThing, file_content, is_https};

impl WindowView {
    /// Watches the selected terminal's log while Agent Activity is open, and nothing while it is closed.
    pub(super) fn watch_activity(&mut self, drawn: &DrawnState) {
        let mut wanted = None;
        let activity = drawn.panels.place(taco_protocol::Panel::AgentActivity);
        if activity.open {
            let selected = selected_terminal(drawn.arrangement.as_ref());
            wanted = selected.cloned();
        }
        if wanted == self.activity.terminal {
            return;
        }
        self.activity = ActivityLog {
            terminal: wanted.clone(),
            ..ActivityLog::default()
        };
        self.folds_open.clear();
        self.texts_open.clear();
        self.shared_all_shown = false;
        let request = ActivityRequest::Watch { terminal: wanted };
        self.port.activity(request);
    }

    pub(crate) fn set_up(&self, harness: &HarnessId, action: AgentMenuAction) {
        self.services.agent_action(harness, action);
    }

    pub(crate) fn dismiss_notice(&self, harness: &HarnessId) {
        self.services.dismiss_notice(harness);
    }

    pub(crate) fn toggle_fold(&mut self, stretch: u64, cx: &mut Context<Self>) {
        toggle(&mut self.folds_open, stretch);
        cx.notify();
    }

    pub(crate) fn toggle_text(&mut self, seq: u64, cx: &mut Context<Self>) {
        toggle(&mut self.texts_open, seq);
        cx.notify();
    }

    pub(crate) fn toggle_shared_all(&mut self, cx: &mut Context<Self>) {
        self.shared_all_shown = !self.shared_all_shown;
        cx.notify();
    }

    /// A shared thing opened: an https link in the browser, anything else in the middle; nothing is handed to the system.
    pub(crate) fn open_shared(&mut self, thing: &SharedThing, cx: &mut Context<Self>) {
        let Some(terminal) = self.activity.terminal.clone() else {
            return;
        };
        let content = match &thing.target {
            SharedTarget::Link { url } | SharedTarget::PullRequest { url: Some(url), .. } => {
                if is_https(url) {
                    cx.open_url(url);
                    return;
                }
                Content::Address(url.clone())
            }
            SharedTarget::PullRequest { url: None, .. } => return,
            SharedTarget::Text { .. } => {
                let request = ActivityRequest::Body {
                    terminal: terminal.clone(),
                    thing: thing.id,
                };
                self.port.activity(request);
                Content::Loading
            }
            SharedTarget::File { path } => {
                Self::read_shared_file(&terminal, thing.id, path.clone(), cx);
                Content::Loading
            }
        };
        self.opened = Some(OpenedThing {
            terminal,
            thing: thing.clone(),
            content,
        });
        self.opened_wants_keyboard = true;
        cx.notify();
    }

    fn read_shared_file(terminal: &TerminalId, thing: u64, path: String, cx: &mut Context<Self>) {
        let terminal = terminal.clone();
        let reading = cx.background_spawn(async move { file_content(&path) });
        let filling = cx.spawn(async move |this, cx| {
            let content = reading.await;
            this.update(cx, |view, cx| {
                view.fill_opened(&terminal, thing, content, cx)
            })
            .ok();
        });
        filling.detach();
    }

    /// What a shared thing holds, shown only while that thing is still the one open.
    pub(super) fn fill_opened(
        &mut self,
        terminal: &TerminalId,
        thing: u64,
        content: Content,
        cx: &mut Context<Self>,
    ) {
        let Some(opened) = &mut self.opened else {
            return;
        };
        let still_open = &opened.terminal == terminal && opened.thing.id == thing;
        if !still_open {
            return;
        }
        opened.content = content;
        cx.notify();
    }

    pub(crate) fn close_opened(&mut self, cx: &mut Context<Self>) {
        let was_open = self.opened.take().is_some();
        if was_open {
            self.terminal_wants_keyboard = true;
            cx.notify();
        }
    }

    // A thing belongs to the terminal whose log it came from, so another selection closes it.
    pub(super) fn close_opened_elsewhere(&mut self, drawn: &DrawnState) {
        let Some(opened) = &self.opened else {
            return;
        };
        let selected = selected_terminal(drawn.arrangement.as_ref());
        if selected != Some(&opened.terminal) {
            self.opened = None;
            self.terminal_wants_keyboard = true;
        }
    }

    pub(crate) fn show_older(&self) {
        let Some(terminal) = &self.activity.terminal else {
            return;
        };
        let Some(before) = self.activity.oldest() else {
            return;
        };
        let request = ActivityRequest::Older {
            terminal: terminal.clone(),
            before,
        };
        self.port.activity(request);
    }
}

fn toggle(shown: &mut HashSet<u64>, key: u64) {
    let was_shown = shown.remove(&key);
    if !was_shown {
        shown.insert(key);
    }
}
