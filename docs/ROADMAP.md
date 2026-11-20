# Features

- Move terminals in and out of splits: drag a standalone terminal into an existing split to add it as a pane, and drag a pane out of its split to make it a standalone terminal again.
- Chrome-style tab groups: gather terminals into a named, colored group in the sidebar that can be collapsed to a single row and moved as one.

# Architecture

- Harness template: make the Claude Code support one implementation of a harness interface, so someone else can add support for another coding agent without touching the rest of the app. Today the Claude pieces are spread through it: the session hook script, the mapping from hook events to a row's state (`signalForHook`), the idle glyph read from the terminal title, the resume command on restore, and the `claude` field of the workspace snapshot. A harness would declare all of that in one place: how its sessions report in, which signals mean working, waiting and idle, how to read its title, and the commands that resume or fork a session.
- Rename `RowColorPicker`: it is the whole right-click popup of a row now, and holds Fork with Claude beside the color dots. The component, its file and its props should be named for the popup, not for the colors.

# Bugfixes

- The terminal sometimes goes blank: text disappears and only background fills stay (diff highlight bands, the grey block behind a Claude tool call), and the only known fix is restarting the app. It happens rarely, with no known way to trigger it. Unconfirmed guess: xterm's WebGL glyph texture atlas going stale. Backgrounds and glyphs are drawn separately, so losing only the glyphs fits. `loadWebglRenderer` in `src/renderer/terminal/registry.ts` handles context loss, but nothing ever calls `term.clearTextureAtlas()`. VS Code calls it to recover from the same symptom. Before relying on that guess, check whether forcing a full redraw (resizing the window, toggling the sidebar) brings the text back.
- A split can lose a race with its own shell. `splitTerminal` in `src/renderer/App.tsx` checks that the split is allowed, then waits for the shell to spawn, and does not look again. If the group fills up in that gap, the model refuses the pane and the shell keeps running with nothing showing it. If the active pane changes in that gap, the new pane lands beside the wrong terminal. `forkTerminal` already checks again after the spawn and disposes the shell when the split no longer fits; `cmd+\` needs the same guard, and should name its donor pane when the key is pressed.
