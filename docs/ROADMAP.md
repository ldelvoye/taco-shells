# Features

- Settings tab: `cmd+,` opens a tab with a control for every setting (theme, font and size, cursor, scrollback, restore on launch) instead of opening `settings.json` in `$EDITOR` in a new terminal. The file stays the source of truth: a change in the tab is written back to it and applies live, the same as a hand edit, so the write has to keep the comments and formatting the user put in the file. Keybindings can follow as a second section once settings work.
- Which agent a terminal is running, at a glance. Once a second agent is supported and people run two side by side, a row should show whether it holds Claude Code, Codex or another agent, for instance an icon per agent beside the session marker. Grouping terminals by agent automatically could build on the same thing. Waits for a second harness: Claude Code is the only one today, so there is nothing to tell apart.

# Distribution

- An install a stranger can finish. A dmg downloaded in a browser reports as damaged until the `xattr` command in the README is run. Candidates: an install script served from the site (a file fetched by `curl` is not quarantined, which needs confirming on a Mac with Gatekeeper on), or signing and notarising with a Developer ID. The other half is done: the app now wires Claude Code's hooks itself, from the installed copy. This comes before the release page, which can only show the install that exists.
- Release page at `tacoshells.dev`: a page whose job is to get someone who has never heard of Taco Shells to a working install. It leads with what the app does and how to install it, and its download points at GitHub's `/releases/latest`, which is already the stable link. The domain is not bought yet; it was open on Namecheap on 2026-11-23. Still to decide: where the page's source lives and where it is hosted.

# Architecture

- Rename `RowColorPicker`: it is the whole right-click popup of a row now, and holds Fork with Claude beside the color dots. The component, its file and its props should be named for the popup, not for the colors.
