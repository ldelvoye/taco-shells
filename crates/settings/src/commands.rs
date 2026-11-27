#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandId {
    TerminalNew,
    TerminalClose,
    TerminalFocusNextGroup,
    TerminalFocusPreviousGroup,
    TerminalCopy,
    TerminalPaste,
    TerminalClear,
    TerminalKillLine,
    TerminalLineStart,
    TerminalLineEnd,
    TerminalWordLeft,
    TerminalWordRight,
    TerminalInsertNewline,
    SidebarToggle,
    ConfigOpenSettings,
    ConfigOpenKeybindings,
}

impl CommandId {
    pub const ALL: [CommandId; 16] = [
        CommandId::TerminalNew,
        CommandId::TerminalClose,
        CommandId::TerminalFocusNextGroup,
        CommandId::TerminalFocusPreviousGroup,
        CommandId::TerminalCopy,
        CommandId::TerminalPaste,
        CommandId::TerminalClear,
        CommandId::TerminalKillLine,
        CommandId::TerminalLineStart,
        CommandId::TerminalLineEnd,
        CommandId::TerminalWordLeft,
        CommandId::TerminalWordRight,
        CommandId::TerminalInsertNewline,
        CommandId::SidebarToggle,
        CommandId::ConfigOpenSettings,
        CommandId::ConfigOpenKeybindings,
    ];

    pub fn id(self) -> &'static str {
        match self {
            CommandId::TerminalNew => "terminal.new",
            CommandId::TerminalClose => "terminal.close",
            CommandId::TerminalFocusNextGroup => "terminal.focusNextGroup",
            CommandId::TerminalFocusPreviousGroup => "terminal.focusPreviousGroup",
            CommandId::TerminalCopy => "terminal.copy",
            CommandId::TerminalPaste => "terminal.paste",
            CommandId::TerminalClear => "terminal.clear",
            CommandId::TerminalKillLine => "terminal.killLine",
            CommandId::TerminalLineStart => "terminal.lineStart",
            CommandId::TerminalLineEnd => "terminal.lineEnd",
            CommandId::TerminalWordLeft => "terminal.wordLeft",
            CommandId::TerminalWordRight => "terminal.wordRight",
            CommandId::TerminalInsertNewline => "terminal.insertNewline",
            CommandId::SidebarToggle => "sidebar.toggle",
            CommandId::ConfigOpenSettings => "config.openSettings",
            CommandId::ConfigOpenKeybindings => "config.openKeybindings",
        }
    }

    pub fn from_id(id: &str) -> Option<CommandId> {
        let mut commands = CommandId::ALL.into_iter();
        commands.find(|command| command.id() == id)
    }

    pub fn repeats(self) -> bool {
        matches!(
            self,
            CommandId::TerminalWordLeft | CommandId::TerminalWordRight
        )
    }
}
