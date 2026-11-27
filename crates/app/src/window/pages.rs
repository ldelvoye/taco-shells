//! The window's pages: opening, closing and drawing Settings and Shortcuts over the middle.

use std::time::Instant;

use gpui::{Context, Div, FocusHandle};
use taco_protocol::{ConfigFile, WorkspaceRequest};

use super::{PageKeyboard, WindowView};
use crate::motion::{LONGEST_SETTING_MOVE, Motion, PAGE_TIME, progress};
use crate::page::{Page, PageMove, pages_after};
use crate::settings_page::{SettingsInput, settings_page};
use crate::shortcuts_page::{ShortcutsInput, shortcuts_page};

/// The pages over the middle and the one fading out.
pub(super) struct PagesState {
    /// The pages over the middle, the top one drawn; empty when the terminal shows.
    pub(super) open: Vec<Page>,
    pub(super) focus: FocusHandle,
    /// When the page on top came up, so it fades in.
    pub(super) since: Option<Instant>,
    /// The last page to close, fading out over the terminal.
    pub(super) leaving: Option<(Page, Instant)>,
}

impl WindowView {
    fn open_file(&mut self, file: ConfigFile, cx: &mut Context<Self>) {
        self.close_pages(cx);
        let request = WorkspaceRequest::OpenTerminal { file: Some(file) };
        self.port.send(request, None);
    }

    pub(crate) fn open_settings_file(&mut self, cx: &mut Context<Self>) {
        self.open_file(ConfigFile::Settings, cx);
    }

    pub(crate) fn open_keybindings_file(&mut self, cx: &mut Context<Self>) {
        self.open_file(ConfigFile::Keybindings, cx);
    }

    /// Moves through the pages, and gives the keyboard to the page now on top, or to the terminal once none is.
    pub(crate) fn move_pages(&mut self, page_move: PageMove, cx: &mut Context<Self>) {
        let after = pages_after(&self.pages.open, page_move);
        if after == self.pages.open {
            return;
        }
        let opened = after.len() > self.pages.open.len();
        let shortcuts_left = !after.contains(&Page::Shortcuts);
        let was_on_top = self.pages.open.last().copied();
        let now = Instant::now();
        let closing_all = after.is_empty();
        if closing_all && let Some(closed) = was_on_top {
            self.pages.leaving = Some((closed, now));
            self.pages.since = None;
        } else {
            self.pages.leaving = None;
            self.pages.since = Some(now);
        }
        self.pages.open = after;
        if shortcuts_left {
            self.forget_search(cx);
            self.shortcuts.capture = None;
        }
        let top = self.pages.open.last().copied();
        match top {
            None => {
                self.keyboard.page_wants_keyboard = None;
                self.keyboard.terminal_wants_keyboard = true;
            }
            Some(page) if opened => {
                self.keyboard.page_wants_keyboard = Some(keyboard_on_opening(page))
            }
            Some(_) => self.keyboard.page_wants_keyboard = Some(PageKeyboard::Page),
        }
        cx.notify();
    }

    /// Closes every page, for an action that means to show a terminal.
    pub(super) fn close_pages(&mut self, cx: &mut Context<Self>) {
        self.move_pages(PageMove::CloseAll, cx);
    }

    pub(super) fn page_element(
        &self,
        page: Page,
        now: Instant,
        reduce_motion: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let look = &self.look;
        match page {
            Page::Settings => {
                let motion = Motion {
                    moved: &self.settings.moved,
                    now,
                    reduce_motion,
                    bounds: &self.settings.drawn_bounds,
                };
                let input = SettingsInput {
                    settings: &self.shown.settings,
                    problems: &self.shown.notices.config_problems,
                    unread: &self.shown.unread,
                    refused: &self.settings.refused,
                    fields: &self.settings.fields,
                    bindings: &self.shown.bindings,
                    motion: &motion,
                    look,
                };
                settings_page(&input, cx)
            }
            Page::Shortcuts => {
                let input = ShortcutsInput {
                    bindings: &self.shown.bindings,
                    defaults: &self.shown.default_bindings,
                    query: &self.shortcuts.query,
                    search: &self.shortcuts.search,
                    capture: self.shortcuts.capture.as_ref(),
                    refused: &self.shortcuts.refused,
                    unread: self.shown.unread_keybindings,
                    problems: &self.shown.notices.config_problems,
                    look,
                };
                shortcuts_page(&input, cx)
            }
        }
    }

    /// How far the page on top has faded in since it came up.
    pub(super) fn page_appeared(&self, now: Instant, reduce_motion: bool) -> f32 {
        let Some(since) = self.pages.since else {
            return 1.;
        };
        progress(since, now, PAGE_TIME, reduce_motion)
    }

    /// Whether anything on a page is still moving, so another frame is wanted.
    pub(super) fn pages_moving(&self, now: Instant, reduce_motion: bool) -> bool {
        if reduce_motion {
            return false;
        }
        let page_fading = self.page_appeared(now, reduce_motion) < 1.;
        let mut leaving = false;
        if let Some((_, since)) = self.pages.leaving {
            leaving = progress(since, now, PAGE_TIME, reduce_motion) < 1.;
        }
        let mut setting_moving = false;
        for moved in self.settings.moved.values() {
            if progress(moved.since, now, LONGEST_SETTING_MOVE, reduce_motion) < 1. {
                setting_moving = true;
            }
        }
        page_fading || leaving || setting_moving
    }
}

// The shortcuts page opens on its search, so typing finds a command at once.
fn keyboard_on_opening(page: Page) -> PageKeyboard {
    match page {
        Page::Settings => PageKeyboard::Page,
        Page::Shortcuts => PageKeyboard::Search,
    }
}
