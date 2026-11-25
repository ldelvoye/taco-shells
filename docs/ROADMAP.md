# Features

- Settings tab: `cmd+,` opens a tab with a control for every setting (theme, font and size, cursor, scrollback, restore on launch) instead of opening `settings.json` in `$EDITOR` in a new terminal. The file stays the source of truth: a change in the tab is written back to it and applies live, the same as a hand edit, so the write has to keep the comments and formatting the user put in the file. Keybindings can follow as a second section once settings work.
- Which agent a terminal is running, at a glance. Once a second agent is supported and people run two side by side, a row should show whether it holds Claude Code, Codex or another agent, for instance an icon per agent beside the session marker. Grouping terminals by agent automatically could build on the same thing. Waits for a second harness: Claude Code is the only one today, so there is nothing to tell apart.

# Distribution

- Release page at `tacoshells.dev`: a page whose job is to get someone who has never heard of Taco Shells to a working install. It leads with what the app does and the install command from the README in a box with a copy button, and has no download button: a build downloaded in a browser reports as damaged. The page can serve `install.sh` at its own address or redirect to the copy on GitHub's `/releases/latest`. The domain is not bought yet; it was open on Namecheap on 2026-11-23. Still to decide: where the page's source lives and where it is hosted.

# Architecture

- The legacy session routes go. Claude's hook script posts to `/agent-session/*` now, and the focus server still answers `/claude-session/*` for copies of the script from before that. Delete those routes once no hand-wired setup points at an older copy.

# Tests

- Tests that do not earn their place are cut. About ten unit assertions copy a literal from the source or repeat a neighbouring case, and six e2e checks assert again what the model's own tests already pin; each of those keeps only the half proving a chord reaches the screen. Two tests whose name promises more than they assert get a truthful name, the test for declining gains the one line proving the written entries are left alone, and the five unit files that never remove their temp directories get a shared helper. Not worth doing: a fake `node-pty` to test `sessions.ts`, a test for the updater's startup pruning, sharing workspace builders across test files, or sharing one app between e2e tests.
- e2e harness in pieces. `e2e/app.ts` is a 1,000-line file around one 536-line function, and every helper is named three times. It splits along its seams: keys, the DevTools connection, reading the screen, mouse and drag input, and the app's lifecycle. Helpers only one test file uses move beside it, and `rightClickHeader`, `clickUngroupItem` and `dragHeader` have no caller and go. The refused-split test waits half a second after each of ten presses and needs one wait.
