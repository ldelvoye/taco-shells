# Look and layout

The redesign of the window is built: a strip of icons, panels that move to the left, right or bottom edge, one terminal in the middle, a settings page and a keyboard shortcuts page, the app's own colors and typeface, and a setting for what a plain shell's row is called. What is left of it:

- Nothing should depend on the macOS menu bar. A Windows port is wanted, and the Mac conventions will not carry over. Setting up an agent and restarting to update are both reachable from the window now. Still to do: check that everything else the menu bar offers has a place in the window, the app's version among them, and decide what the menu bar keeps.
- The finish of dragging a row. Three things are undecided: whether the wash shown when a row is dropped into a tab group takes the orange look, the dragged card covering the slot it is about to land in, and that card's square corners.
- Two parts of the identity were never ruled: the corner radii and the wordmark.
- What the window costs in CPU while a working mark is turning has not been measured.

# Terminals

- A restored session is resumed and then forgotten when Agent Activity is not set up. At launch the app restarts each terminal's agent session, then waits 60 seconds for the agent to report its start. An agent without the plugin never reports, so the app drops the session from the save, and the next launch brings back a plain shell. Still to decide: whether a session that was resumed but never confirmed is kept, or whether nothing is resumed at all until Agent Activity is set up.
- Terminals on a remote dev machine: look into what it would take for a row to hold a session running on a cloud dev machine instead of this Mac, with the same agent state as a local row, and for that session to keep running while the laptop sleeps or the app is quit. This is an investigation before it is a build: find out what such a machine's own CLI already offers for starting, listing and attaching to sessions, and whether the app should drive that CLI or only show what it reports.
- Typing through an input method (composed characters, such as accents or Japanese) has only been checked by tests. It needs a check by hand in the real window.

# Agent Activity

- Which agent a terminal is running, at a glance. Once a second agent is supported and people run two side by side, a row should show whether it holds Claude Code, Codex or another agent, for instance an icon per agent beside the session marker. Grouping terminals by agent automatically could build on the same thing. Waits for a second harness: Claude Code is the only one today, so there is nothing to tell apart.
- The Shared section as a menu: a place where you see the important things your agents have shared at a glance, instead of reading down a list. Today Shared is a plain list of what an agent gave you. Turning it into a menu needs design of its own before anything is built. Still to decide: what makes a shared item important, how items from several agents sit together, and what opening one does.
- A subagent's report reads as its own entry. Today, when a subagent hands its report back to the agent that started it, the log shows it under "You asked", with the whole frame Claude Code wraps around the report (the `<agent-message>` tags and the paragraph saying the text is model output). It should show as a message from the subagent, with only the report.
- Subagents mapped out: see which subagents an agent started, what each was asked, whether it is still running, and what it handed back, instead of finding their steps mixed into one log. This may mean redoing what the panel holds, since it was laid out for one agent's notes and steps and now also carries Shared, messages between sessions and subagents. Needs design of its own before anything is built. Still to decide: how a subagent's work sits beside its parent's, how far down nested subagents are shown, and what happens to the log as it reads today.
- Agents that drive the app: the MCP server that gives an agent its activity tools could later also let it work with terminals the way the user does: open one in a folder with an agent and a first message, name it, move it, put it in a tab group, close it, and read the state of the others. An agent orchestrating a piece of work could then start real sessions itself and keep them organized, instead of the user opening each one by hand. This waits on things the app cannot do yet: it cannot name a row, and it cannot tell a caller which terminal it just opened. Still to decide: how a terminal the user is working in is kept apart from one an agent opened, and how many terminals an agent may open.
- Diffs in the panel: whether a step that changed a file can show what changed, and where.
- Whether the "Ask for a note" setting dims while Agent Activity is off, since it does nothing then.

# Distribution

- Release page at `tacoshells.dev`: a page whose job is to get someone who has never heard of Taco Shells to a working install. It leads with what the app does and the install command from the README in a box with a copy button, and has no download button: a build downloaded in a browser reports as damaged. The page is written and lives in its own repository, `ldelvoye/tacoshells.dev`, and the domain is bought. It is not deployed yet.
- A more durable way to release, before there are users to strand. A version is made stable by hand, from a Mac that holds the things a release needs. Update signing rests on one private key file: if it is lost with no copy, no later update can be signed for the installs already out there, and each of them has to be reinstalled by hand to take a new key. Find out what would make that survivable and less tied to one machine. Things to weigh: a spare key whose public half ships in the app before 1.0, a key per machine, Apple's Developer ID signing and notarisation, checking the first install as well as updates (the install script comes from the same release as the zip it fetches, so checking one against the other proves little), and whether promotion can move off a laptop without letting a stolen token publish an update the app accepts.

# Code

Small cleanups the rewrite's last round left, none of them a change in behavior:

- `crates/host` works out the bundle's layout itself in `services_host.rs` (where the plugin program sits beside the app's own), which repeats what `crates/services` and `crates/identity` already know.
- The terminal, harness and session id travel together as three values through `services_host.rs` and `workspace_host.rs`; one type would carry them.
- Two mechanisms serve a single case and could be plainer: the window's `Awaited`, an enum with one variant behind a counter and a map, and the remaining `Mutex<Option<T>>` slots in the services host.
- `TerminalEvent` has no `id()`, so callers match on every variant to find which terminal an event is about.
- `HookEdit` in `crates/services` now names only edits to the plugin folders, and wants a name that says so.
- No render fixture draws a draft, merged or closed pull request pill.
