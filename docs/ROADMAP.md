# Features

- Settings tab: `cmd+,` opens a tab with a control for every setting (theme, font and size, cursor, scrollback, restore on launch) instead of opening `settings.json` in `$EDITOR` in a new terminal. The file stays the source of truth: a change in the tab is written back to it and applies live, the same as a hand edit, so the write has to keep the comments and formatting the user put in the file. Keybindings can follow as a second section once settings work.

# Architecture

- Harness template: make the Claude Code support one implementation of a harness interface, so someone else can add support for another coding agent without touching the rest of the app. Today the Claude pieces are spread through it: the session hook script, the mapping from hook events to a row's state (`signalForHook`), the idle glyph read from the terminal title, the resume command on restore, and the `claude` field of the workspace snapshot. A harness would declare all of that in one place: how its sessions report in, which signals mean working, waiting and idle, how to read its title, and the commands that resume or fork a session.
- Rename `RowColorPicker`: it is the whole right-click popup of a row now, and holds Fork with Claude beside the color dots. The component, its file and its props should be named for the popup, not for the colors.
