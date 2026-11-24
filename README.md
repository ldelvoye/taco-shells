# Taco Shells

A terminal manager for macOS, built with Electron.

I like VS Code's terminal manager: a sidebar list where splits show up as coupled rows, each with its own status. It's the only thing I've found that makes running two Claude Code sessions side by side legible. What I don't like is that it comes with an editor attached, and popping the terminal out into its own window leaves the sidebar behind.

So I'm building just that part as its own app.

Early. Right now it runs as many terminals as you like and lists them in the sidebar, each row named by whatever the program in it publishes as its title. You switch by clicking a row and move a terminal by dragging its row: onto the edge of another row to reorder, onto the middle of one to join it in a split, and back out to stand alone. A split's bracket drags the whole split. Right-click a row to start a group: a named, colored header you can collapse, drag as one, and drop terminals onto. Splitting a terminal puts its panes side by side, with a divider you can drag, and brackets their rows together in the sidebar so a group reads as a group. Hovering a row shows a trash can that closes that terminal, hovering a group's header shows a plus that opens a terminal in that group, and the plus beside the Taco Shells name opens one at the bottom of the list. A split starts in the directory the terminal you split was sitting in.

## Installing

Paste this into a terminal:

```sh
curl -fsSL https://github.com/ldelvoye/taco-shells/releases/latest/download/install.sh | sh
```

It downloads the latest stable release, puts Taco Shells in Applications and opens it. [The script](install.sh) is short enough to read first. It needs a Mac with Apple silicon.

To install by hand, download the zip from [the latest release](https://github.com/ldelvoye/taco-shells/releases/latest), unzip it and drag Taco Shells into Applications. A copy downloaded in a browser won't open yet: the app is signed only ad-hoc and isn't notarised, so macOS reports it as damaged and offers no way past it. One command fixes that:

```sh
xattr -dr com.apple.quarantine "/Applications/Taco Shells.app"
```

Once it's installed, the app checks for a new stable release, downloads it in the background, and offers a restart that swaps in the new version and brings your terminals back.

Releases marked as pre-releases are points in the project's history rather than builds to install; they carry no download.

## Setting up Claude Code

The session markers, restoring a conversation, forking one and jumping to the terminal that wants you all rely on Claude Code telling Taco Shells what its sessions are doing, through hooks in Claude Code's `settings.json`. The app adds them for you. The first time you run Claude Code in a Taco Shells terminal without them, a strip at the top of the window offers to do it: **Set up** writes the hook entries, and **Not now** closes the strip for good. The markers start with the next Claude Code session you open.

After that, setup lives in the **Agents** menu in the menu bar. **Claude Code** there shows whether it is set up, and holds one action:

- **Set Up Claude Code** writes the entries, whether or not you said Not now earlier.
- **Remove Setup** deletes the entries the app wrote and nothing else. The app does not put them back by itself.
- **Switch to Managed Setup** appears when your settings already run the hook scripts from somewhere else, such as a clone of this repo. The app leaves those entries alone until you pick it, then replaces them with its own.

The app only touches entries that run its own scripts, which it keeps in `~/.taco-shells/hooks/`, and leaves the rest of the file the way you wrote it. When an update needs a new entry the app adds it, and an entry you deleted stays deleted.

To wire the hooks yourself, set `"manageAgentHooks": false` in [settings](#settings): the app then neither offers nor changes anything. **Show Hook Scripts in Finder**, in the same menu, opens the folder holding the scripts, and [`setup.ts`](src/shared/harness/claude/setup.ts) lists which script goes on which event and which entries are `async`. An entry looks like this:

```json
{ "hooks": [{ "type": "command", "command": "/Users/you/.taco-shells/hooks/taco-shells-claude-session.sh", "async": true }] }
```

If you moved Claude Code's config with `CLAUDE_CONFIG_DIR`, the app follows it only when it can see the variable. An app opened from Finder does not see one exported in a shell startup file, and looks in `~/.claude`.

## Jumping to the terminal that wants you

[Clawd on Desk](https://github.com/rullerzhou-afk/clawd-on-desk) shows a bubble when a Claude Code session needs you, with a "go to terminal" button. Clicking it raises Taco Shells and switches to the terminal that asked, rather than leaving you on whichever one you had open.

It needs Claude Code [set up](#setting-up-claude-code), which is what tells Taco Shells a session is waiting. Without it nothing is ever flagged and the button only raises the app.

## Picking up where you left off

Quit and relaunch, and the terminals you had come back, each in the directory it was in. A terminal that was running Claude Code picks its conversation back up by resuming it. That part needs Claude Code [set up](#setting-up-claude-code), which is how Taco Shells learns which terminal is running which conversation. Set `"restoreOnLaunch": false` in settings to turn the whole thing off.

## Coloring terminals

Any row in the sidebar can carry a color, shown as a stripe down its left edge. The colors mean whatever you want: what's on your plate, what's waiting on someone else, work and personal. Right-click a row and hover the dots to preview one on the row; only a click keeps it. The crossed-out dot removes the color. `cmd+opt+1` to `cmd+opt+6` color the terminal you're in, and pressing the same one again removes it. Colors come back when you relaunch.

## Seeing which session needs you

A terminal running Claude Code gets a marker in front of its name in the sidebar: a grey ring while it sits idle, a spinner while Claude works, an amber dot when it is waiting on you, and a green check when it finished while you were looking at another terminal. The check clears once you switch to it. With the sidebar hidden, a "2 waiting" pill in the titlebar counts the amber ones.

The markers come from Claude Code's hooks, so they need Claude Code [set up](#setting-up-claude-code).

## Forking a Claude conversation

Right-click a row that is running Claude Code and pick **Fork with Claude**. A copy of the conversation opens in a split beside it, so you can try a second direction while the first carries on untouched, even mid-reply. The fork wears the same color as its original and its name starts with `⑂`, then `⑂2`, `⑂3` for the ones after it. The item is greyed out where there is no conversation to copy: on a terminal that is not running Claude Code, and on a brand-new session or a fresh fork until its first message. It needs Claude Code [set up](#setting-up-claude-code).

## Other coding agents

Claude Code is the only agent Taco Shells supports today, and everything the app knows about it lives in one folder. Support for an agent is a harness: a folder under `src/shared/harness/` that says how to read the agent's hook events, session ids and terminal title, and which commands resume and fork a session. Each harness also has a hook script in `hooks/` that posts those events to the app. [`types.ts`](src/shared/harness/types.ts) lists what a harness declares, and [`claude/`](src/shared/harness/claude) is the worked example. A new agent's script posts to `/agent-session/start`, `/agent-session/end` and `/agent-session/state` with its harness id in the body, and one line in [`registry.ts`](src/shared/harness/registry.ts) registers it. A harness that also declares a `setup` (the settings file its hooks go in, and the entries) gets its own entry in the Agents menu and the same one-time offer Claude Code gets.

## Settings

It reads `~/.taco-shells/settings.json` and `~/.taco-shells/keybindings.json`, both written on first launch holding the defaults, so the file you open is the list of everything you can change. Edits apply when you save, without a restart. Comments and trailing commas are fine. Theme follows macOS unless you pin it to `light` or `dark`.

Your file is layered over the defaults rather than replacing them, so deleting a line brings the default back. To turn a key off, bind it to `null`.

## Building it yourself

```sh
npm install
npm run dev
```

A build from the working tree installs as **Taco Shells Dev**, a separate app that sits beside the released one and reads its own config, so developing it never disturbs the copy you use.

[AGENTS.md](AGENTS.md) covers the rest — testing, the two-app split, and how versions and releases work. It's written for coding agents, but you're welcome to read it.

## License

MIT.
