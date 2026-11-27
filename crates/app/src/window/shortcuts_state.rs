//! The window's shortcuts: the search, capturing a chord and writing a keybinding.

use std::collections::HashMap;

use gpui::{AppContext, Context, Entity, Keystroke};
use taco_settings::{Chord, CommandId, KeymapChange};

use super::keyboard::is_bare_escape;
use super::{PageKeyboard, WindowView};
use crate::capture::{Capture, Pressed, capture_still_open, change_for, pressed};
use crate::keys::chord_of;
use crate::look::Look;
use crate::shortcuts_page::SEARCH_PLACEHOLDER;
use crate::text_field::{TextField, TextFieldEvent};

/// The shortcuts page's search and the shortcut being set.
pub(super) struct ShortcutsState {
    pub(super) search: Entity<TextField>,
    pub(super) query: String,
    /// A shortcut being set on the shortcuts page; while it is open, every key goes to it.
    pub(super) capture: Option<Capture>,
    /// Why each command's last shortcut change was not written, until it is changed again.
    pub(super) refused: HashMap<CommandId, String>,
}

impl WindowView {
    pub(super) fn forget_search(&mut self, cx: &mut Context<Self>) {
        self.shortcuts.query.clear();
        self.shortcuts
            .search
            .update(cx, |field, cx| field.set_text("", cx));
    }

    /// Turns a chip, or the + when `changing` is none, into "Press keys".
    pub(crate) fn start_capture(
        &mut self,
        command: CommandId,
        changing: Option<Chord>,
        cx: &mut Context<Self>,
    ) {
        self.shortcuts.capture = Some(Capture {
            command,
            changing,
            pending: None,
            problem: None,
        });
        // Keys go to the page, not to the search field.
        self.keyboard.page_wants_keyboard = Some(PageKeyboard::Page);
        cx.notify();
    }

    pub(crate) fn cancel_capture(&mut self, cx: &mut Context<Self>) {
        self.shortcuts.capture = None;
        cx.notify();
    }

    pub(super) fn capture_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(mut capture) = self.shortcuts.capture.take() else {
            return;
        };
        if is_bare_escape(keystroke) {
            cx.notify();
            return;
        }
        let chord = chord_of(keystroke);
        let result = pressed(&capture, chord.clone(), &self.shown.bindings);
        match result {
            Pressed::Write(change) => self.write_shortcut(change),
            Pressed::Unchanged => {}
            Pressed::Ask(problem) => {
                capture.pending = Some(chord);
                capture.problem = Some(problem);
                self.shortcuts.capture = Some(capture);
            }
        }
        cx.notify();
    }

    /// "Use it here": the keys the row asked about are written for it; the writer takes them from the command that had them.
    pub(crate) fn take_pending(&mut self, cx: &mut Context<Self>) {
        let Some(capture) = self.shortcuts.capture.take() else {
            return;
        };
        if let Some(pending) = capture.pending.clone() {
            let change = change_for(&capture, pending);
            self.write_shortcut(change);
        }
        cx.notify();
    }

    pub(crate) fn remove_captured(&mut self, cx: &mut Context<Self>) {
        let Some(capture) = self.shortcuts.capture.take() else {
            return;
        };
        if let Some(chord) = capture.changing {
            let command = capture.command;
            self.write_shortcut(KeymapChange::Remove { command, chord });
        }
        cx.notify();
    }

    pub(crate) fn reset_shortcut(&mut self, command: CommandId, cx: &mut Context<Self>) {
        self.write_shortcut(KeymapChange::Reset { command });
        cx.notify();
    }

    fn write_shortcut(&mut self, change: KeymapChange) {
        let command = change.command();
        self.shortcuts.refused.remove(&command);
        self.services.write_keybinding(change);
    }

    /// A new config can take away the chip a capture is changing, or the file it would write to.
    pub(super) fn close_stale_capture(&mut self) {
        let Some(capture) = &self.shortcuts.capture else {
            return;
        };
        let bindings = &self.shown.bindings;
        let unread = self.shown.unread_keybindings;
        if !capture_still_open(capture, bindings, unread) {
            self.shortcuts.capture = None;
        }
    }

    // Esc in the search clears it and hands the keyboard to the page, so the next Esc goes back to Settings.
    fn search_said(&mut self, event: &TextFieldEvent, cx: &mut Context<Self>) {
        match event {
            TextFieldEvent::Edited(text) => {
                self.shortcuts.query = text.clone();
                self.shortcuts.capture = None;
            }
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => {
                self.forget_search(cx);
                self.keyboard.page_wants_keyboard = Some(PageKeyboard::Page);
            }
        }
        cx.notify();
    }
}

pub(super) fn search_field(look: &Look, cx: &mut Context<WindowView>) -> Entity<TextField> {
    let look = look.clone();
    let field = cx.new(|cx| {
        TextField::new("", look, cx)
            .with_shell_focus()
            .with_placeholder(SEARCH_PLACEHOLDER)
    });
    let heard = cx.subscribe(&field, |view, _, event: &TextFieldEvent, cx| {
        view.search_said(event, cx);
    });
    heard.detach();
    field
}
