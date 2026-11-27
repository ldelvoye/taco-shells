//! Where a key goes: a shortcut being set, a page, the opened thing, a command, or on to whatever holds the keyboard.

use taco_settings::CommandId;

use crate::page::PageMove;

/// What a key meets when it arrives.
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyState {
    pub capturing: bool,
    pub popup_open: bool,
    pub page_open: bool,
    pub thing_open: bool,
    pub terminal_on_screen: bool,
}

/// A key as routing reads it: a bare Esc, the command its chord is bound to, and whether it repeats from being held.
#[derive(Clone, Copy, Debug, Default)]
pub struct Key {
    pub bare_escape: bool,
    pub command: Option<CommandId>,
    pub held: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Capture,
    /// The key goes on to whatever holds the keyboard: the terminal, a page's field, the name field.
    PassOn,
    /// Taken, and nothing runs.
    Swallow,
    MovePages(PageMove),
    CloseOpened,
    /// The pages close, then the command runs.
    CloseThenRun(CommandId),
    Run(CommandId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Window(OverPage),
    /// Acts on the terminal on screen.
    Terminal,
}

/// What a window command does while a page is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OverPage {
    Move(PageMove),
    ShowTerminal,
}

pub fn route(state: KeyState, key: Key) -> Route {
    // A shortcut being set takes every key first, so cmd+w sets cmd+w instead of closing anything.
    if state.capturing {
        return Route::Capture;
    }
    // The name field keeps every key while it is open.
    if state.popup_open {
        return Route::PassOn;
    }
    if state.page_open && key.bare_escape {
        return Route::MovePages(PageMove::Back);
    }
    if state.thing_open && key.bare_escape {
        return Route::CloseOpened;
    }
    let Some(command) = key.command else {
        return Route::PassOn;
    };
    let kind = kind_of(command);
    if state.page_open {
        return over_page(kind, command, key.held);
    }
    // Close-terminal closes the opened thing first, as it closes a page.
    if state.thing_open && command == CommandId::TerminalClose {
        if key.held {
            return Route::Swallow;
        }
        return Route::CloseOpened;
    }
    // The terminal's own commands need a terminal on screen to act on, not one an opened thing covers.
    let terminal_shown = state.terminal_on_screen && !state.thing_open;
    let runs = kind != Kind::Terminal || terminal_shown;
    if !runs {
        return Route::PassOn;
    }
    if key.held && !repeats(command) {
        return Route::Swallow;
    }
    Route::Run(command)
}

// Nothing closes a terminal nobody can see, and anything that shows one closes the pages first.
fn over_page(kind: Kind, command: CommandId, held: bool) -> Route {
    if held {
        return Route::PassOn;
    }
    match kind {
        Kind::Window(OverPage::Move(page_move)) => Route::MovePages(page_move),
        Kind::Window(OverPage::ShowTerminal) => Route::CloseThenRun(command),
        Kind::Terminal => Route::PassOn,
    }
}

fn kind_of(command: CommandId) -> Kind {
    match command {
        CommandId::TerminalNew
        | CommandId::TerminalFocusNextGroup
        | CommandId::TerminalFocusPreviousGroup => Kind::Window(OverPage::ShowTerminal),
        CommandId::TerminalClose | CommandId::SidebarToggle => {
            Kind::Window(OverPage::Move(PageMove::CloseAll))
        }
        CommandId::ConfigOpenSettings => Kind::Window(OverPage::Move(PageMove::ToggleSettings)),
        CommandId::ConfigOpenKeybindings => Kind::Window(OverPage::Move(PageMove::ToggleShortcuts)),
        CommandId::TerminalCopy
        | CommandId::TerminalPaste
        | CommandId::TerminalClear
        | CommandId::TerminalKillLine
        | CommandId::TerminalLineStart
        | CommandId::TerminalLineEnd
        | CommandId::TerminalWordLeft
        | CommandId::TerminalWordRight
        | CommandId::TerminalInsertNewline => Kind::Terminal,
    }
}

// Moving a word at a time repeats while held, as in any terminal; every other command runs once.
fn repeats(command: CommandId) -> bool {
    matches!(
        command,
        CommandId::TerminalWordLeft | CommandId::TerminalWordRight
    )
}

#[cfg(test)]
mod tests {
    use taco_settings::CommandId;

    use super::{Key, KeyState, Route, route};
    use crate::page::PageMove;

    #[test]
    fn a_key_goes_to_the_first_thing_that_claims_it() {
        let terminal = KeyState {
            terminal_on_screen: true,
            ..KeyState::default()
        };
        let capturing = KeyState {
            capturing: true,
            ..terminal
        };
        let popup = KeyState {
            popup_open: true,
            ..terminal
        };
        let page = KeyState {
            page_open: true,
            ..terminal
        };
        let opened = KeyState {
            thing_open: true,
            ..terminal
        };
        let page_over_opened = KeyState {
            page_open: true,
            ..opened
        };
        let no_terminal = KeyState::default();

        let escape = Key {
            bare_escape: true,
            ..Key::default()
        };
        let unbound = Key::default();
        let bound = |command| Key {
            command: Some(command),
            ..Key::default()
        };
        let held = |command| Key {
            command: Some(command),
            held: true,
            ..Key::default()
        };

        let close = CommandId::TerminalClose;
        let cases = [
            (capturing, bound(close), Route::Capture),
            (popup, bound(close), Route::PassOn),
            (page, escape, Route::MovePages(PageMove::Back)),
            (page_over_opened, escape, Route::MovePages(PageMove::Back)),
            (opened, escape, Route::CloseOpened),
            (terminal, escape, Route::PassOn),
            (terminal, unbound, Route::PassOn),
            (page, bound(close), Route::MovePages(PageMove::CloseAll)),
            (
                page,
                bound(CommandId::TerminalNew),
                Route::CloseThenRun(CommandId::TerminalNew),
            ),
            (page, bound(CommandId::TerminalPaste), Route::PassOn),
            (page, held(close), Route::PassOn),
            (opened, bound(close), Route::CloseOpened),
            (opened, held(close), Route::Swallow),
            (opened, bound(CommandId::TerminalPaste), Route::PassOn),
            (no_terminal, bound(CommandId::TerminalCopy), Route::PassOn),
            (
                no_terminal,
                bound(CommandId::TerminalNew),
                Route::Run(CommandId::TerminalNew),
            ),
            (
                terminal,
                held(CommandId::TerminalWordLeft),
                Route::Run(CommandId::TerminalWordLeft),
            ),
            (terminal, held(CommandId::TerminalClear), Route::Swallow),
        ];
        for (state, key, expected) in cases {
            assert_eq!(route(state, key), expected, "{state:?} {key:?}");
        }
    }
}
