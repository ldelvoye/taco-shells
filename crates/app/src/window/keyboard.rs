//! The window's keyboard: key routing, commands and where focus goes.

use gpui::{ClipboardItem, Context, Focusable, KeyDownEvent, Keystroke, Window};
use taco_protocol::{DrawnState, FocusMove, Panel, WorkspaceRequest};
use taco_rules::{focus_target, selected_terminal};
use taco_settings::{CommandId, command_for};

use super::{PageKeyboard, WindowView};
use crate::key_route::{Key, KeyState, Route, route};
use crate::keys::chord_of;
use crate::middle::{attachable, line_editing_bytes};
use crate::page::PageMove;

const ESCAPE: &str = "escape";

impl WindowView {
    /// Runs the command a key-down's chord is bound to; false when the window has nothing to run for it, so the key goes on to the terminal.
    pub(super) fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let state = KeyState {
            capturing: self.capture.is_some(),
            popup_open: self.popup.is_some(),
            page_open: !self.pages.is_empty(),
            thing_open: self.opened.is_some(),
            terminal_on_screen: self.attached.on_screen.is_some(),
        };
        let chord = chord_of(&event.keystroke);
        let command = command_for(&self.shown.bindings, &chord);
        let key = Key {
            bare_escape: is_bare_escape(&event.keystroke),
            command,
            held: event.is_held,
        };
        match route(state, key) {
            Route::Capture => self.capture_key(&event.keystroke, cx),
            Route::PassOn => return false,
            Route::Swallow => {}
            Route::MovePages(page_move) => self.move_pages(page_move, cx),
            Route::CloseOpened => self.close_opened(cx),
            Route::CloseThenRun(command) => {
                self.close_pages(cx);
                self.run(command, cx);
            }
            Route::Run(command) => self.run(command, cx),
        }
        true
    }

    fn run_in_terminal(&mut self, command: CommandId, cx: &mut Context<Self>) {
        let Some(terminals) = self.terminals.clone() else {
            return;
        };
        let Some(terminal) = self.attached.on_screen.clone() else {
            return;
        };
        match command {
            CommandId::TerminalCopy => {
                let selection = terminals.update(cx, |view, _| view.selection(&terminal));
                if let Some(text) = selection {
                    let copied = ClipboardItem::new_string(text);
                    cx.write_to_clipboard(copied);
                }
            }
            CommandId::TerminalPaste => {
                let Some(clipboard) = cx.read_from_clipboard() else {
                    return;
                };
                let Some(text) = clipboard.text() else {
                    return;
                };
                terminals.update(cx, |view, cx| view.paste(&terminal, &text, cx));
            }
            CommandId::TerminalClear => terminals.update(cx, |view, _| view.clear(&terminal)),
            _ => {
                let Some(bytes) = line_editing_bytes(command) else {
                    return;
                };
                terminals.update(cx, |view, _| view.type_bytes(&terminal, bytes));
            }
        }
    }

    fn run(&mut self, command: CommandId, cx: &mut Context<Self>) {
        match command {
            CommandId::TerminalNew => {
                let request = WorkspaceRequest::OpenTerminal { file: None };
                self.port.send(request, None);
            }
            CommandId::TerminalClose => {
                let drawn = self.drawn();
                let selected = selected_terminal(drawn.arrangement.as_ref());
                if let Some(terminal) = selected {
                    self.close(terminal);
                }
            }
            CommandId::TerminalFocusNextGroup => self.move_focus(FocusMove::NextRow, cx),
            CommandId::TerminalFocusPreviousGroup => self.move_focus(FocusMove::PreviousRow, cx),
            CommandId::SidebarToggle => self.toggle_panel(Panel::Terminals),
            CommandId::ConfigOpenSettings => self.move_pages(PageMove::ToggleSettings, cx),
            CommandId::ConfigOpenKeybindings => self.move_pages(PageMove::ToggleShortcuts, cx),
            _ => self.run_in_terminal(command, cx),
        }
    }

    fn move_focus(&mut self, focus_move: FocusMove, cx: &mut Context<Self>) {
        let drawn = self.drawn();
        let Some(arrangement) = &drawn.arrangement else {
            return;
        };
        let Some(target) = focus_target(arrangement, focus_move) else {
            return;
        };
        self.select(&target, cx);
    }

    /// Tells the terminal view what changed, and keeps the keyboard in the terminal on screen.
    pub(super) fn show_terminals(
        &mut self,
        drawn: &DrawnState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let attached = attachable(drawn);
        let refocused = drawn.refocus != self.refocus;
        self.refocus = drawn.refocus;
        let switched = attached.on_screen != self.attached.on_screen || refocused;
        if let Some(terminals) = &self.terminals
            && attached != self.attached
        {
            // Only on a change: showing redraws the view, which would redraw the window again.
            terminals.update(cx, |view, cx| {
                view.show(&attached.open, attached.on_screen.as_ref(), cx);
            });
        }
        self.attached = attached;
        // A key pressed with nothing focused reaches no listener, so the keyboard goes back to the terminal, or the window.
        let focused = window.focused(cx);
        let unfocused = focused.is_none();
        // A page takes the keyboard on the frame after it opens, or whenever nothing holds it.
        if !self.pages.is_empty() {
            let mut wanted = self.page_wants_keyboard.take();
            if wanted.is_none() && unfocused {
                wanted = Some(PageKeyboard::Page);
            }
            match wanted {
                Some(PageKeyboard::Page) => window.focus(&self.page_focus, cx),
                Some(PageKeyboard::Search) => {
                    let search = self.shortcut_search.focus_handle(cx);
                    window.focus(&search, cx);
                }
                None => {}
            }
            return;
        }
        // An opened thing takes the keyboard as it opens, as the last page over it closes, or when nothing holds it.
        if self.opened.is_some() {
            let wanted = self.opened_wants_keyboard || self.terminal_wants_keyboard || unfocused;
            self.opened_wants_keyboard = false;
            self.terminal_wants_keyboard = false;
            if wanted {
                window.focus(&self.opened_focus, cx);
            }
            return;
        }
        let page_closed = self.terminal_wants_keyboard;
        self.terminal_wants_keyboard = false;
        if !switched && !unfocused && !page_closed {
            return;
        }
        let on_screen = self.attached.on_screen.clone();
        if let (Some(terminals), Some(terminal)) = (&self.terminals, &on_screen)
            && self.popup.is_none()
        {
            terminals.update(cx, |view, cx| view.focus(terminal, window, cx));
            return;
        }
        if unfocused {
            window.focus(&self.focus, cx);
        }
    }
}

pub(super) fn is_bare_escape(keystroke: &Keystroke) -> bool {
    keystroke.key == ESCAPE && !keystroke.modifiers.modified()
}
