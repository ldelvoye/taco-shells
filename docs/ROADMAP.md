# Features

- Settings tab: `cmd+,` opens a tab with a control for every setting (theme, font and size, cursor, scrollback, restore on launch) instead of opening `settings.json` in `$EDITOR` in a new terminal. The file stays the source of truth: a change in the tab is written back to it and applies live, the same as a hand edit, so the write has to keep the comments and formatting the user put in the file. Keybindings can follow as a second section once settings work.

# Architecture

- Rename `RowColorPicker`: it is the whole right-click popup of a row now, and holds Fork with Claude beside the color dots. The component, its file and its props should be named for the popup, not for the colors.
