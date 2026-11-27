# Taco Shells

A terminal manager for macOS.

I like VS Code's terminal manager: a sidebar list where splits show up as coupled rows, each with its own status. It's the only thing I've found that makes running two Claude Code sessions side by side legible. What I don't like is that it comes with an editor attached, and popping the terminal out into its own window leaves the sidebar behind.

So I'm building just that part as its own app.

Taco Shells shows your terminals as a list of rows instead of tabs. Each row names its terminal by its folder, or by the agent running in it, and clicking a row shows that terminal. Hover a row for a trash can that closes it; "+ New terminal" at the top of the list opens one.

Gather rows into tab groups: right-click a row and choose New group. A group draws as a tray with its name and how many terminals it holds. Click its header to collapse it, hover the header for a + that opens a terminal inside it, and right-click it to rename it or ungroup it. Drag a row to move it, or to drop it into or out of a group; a slot opens where it will land.

The Terminals list and Agent Activity are panels. Drag a panel by its title to the left, right or bottom edge, drag its inner edge to resize it, and show or hide it from the strip of icons down the left side, which also opens Settings and Keyboard shortcuts. cmd+B shows or hides the terminal list.

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

Releases marked as pre-releases are points in the project's history rather than builds to install; they carry no download.

## Updates

The app checks for a new release every six hours and downloads it in the background. When it's ready, a "Restart to update" button and an Update menu appear, and nothing changes until you choose to restart. Every update must carry a valid signature from the release key. If one doesn't, it's refused, nothing is installed, and the app says so once for that version.

## Agent Activity

Agent Activity follows the terminal you have selected when an agent such as Claude Code runs in it. A card at the top says what the agent is doing now. Below it, Shared lists what the agent shared or the app noticed: files, images, links and pull requests, with whether a pull request was open or merged when last seen. Click a thing to open it in the middle, or a link in your browser. Below that is the log: what you asked, the notes the agent writes when it finishes a piece of work, and its steps folded under each note, with any that failed marked. The first time an agent shows up, Agent Activity offers to set it up for you.

Right-click a row running Claude Code and choose Fork with Claude to start a copy of that conversation in a new terminal.

## Setting up Claude Code

Turn Agent Activity on with the "Agent Activity in this app's terminals" switch on the settings page, or with Set Up Claude Code in the Agents menu. When Claude Code is running in a terminal but isn't set up, the app also offers a Set up button beside it.

Set up installs nothing into Claude Code. The app keeps its own plugin in `~/.taco-shells/claude-plugin`, and while the switch is on, every new terminal hands that folder to Claude Code through `CLAUDE_CODE_PLUGIN_DIRS`. Terminals that were already open keep what they started with, so start Claude Code in a new terminal after setting up.

Set up edits your Claude Code `settings.json` only when it has to, and the app tells you when it did ("One line changed in …"). If your settings set `CLAUDE_CODE_PLUGIN_DIRS` in their `env` block, which would hide the app's folder, Set up adds the folder to that value. Switching Agent Activity off takes it back out, but only while the line is still exactly what the app wrote. If your settings still call the old app's hook scripts, Set up removes those entries. Managed settings are never edited.

The app finds Claude Code's settings in `CLAUDE_CONFIG_DIR` when that's set in the app's own environment, and in `~/.claude` otherwise. Claude Code 2.1.280 or later is needed; with an older one, the Agents menu says so.

## Seeing which session needs you

Each row running an agent carries a small mark: a turning ring while it works, a red dot with a ripple when it is waiting for you, and a green check when it has finished a turn you have not looked at yet. A row with no agent, or one you have already looked at, has no mark. When any agent is waiting, the title bar shows a red "N waiting" pill counting them.

## Picking up where you left off

Quit Taco Shells and open it again, and your terminals come back as you left them: the same rows in the same order, each in its directory, in the same tab groups (with their names, and collapsed or open as they were), with the same row selected and the panels where you put them.

A terminal that was running Claude Code when you quit starts it again with `claude --resume`, in the directory the conversation began in, so the conversation picks up where it stopped. This needs Claude Code set up in Taco Shells, since that is how the app learns which conversation each terminal holds. A terminal where Claude Code had already exited comes back as a plain shell.

Agent Activity comes back with its terminal: the asks, steps, notes and Shared are all kept across a relaunch. A terminal's activity is deleted only when you close that terminal.

To start with an empty window instead, turn off "Reopen my terminals" in Settings, or set `"restoreOnLaunch": false`. With it off nothing is restored, and the Agent Activity of the old terminals is deleted at launch.

## Settings

Settings live in `~/.taco-shells/settings.json` and `~/.taco-shells/keybindings.json`, written with every default on first launch. An edit applies as soon as you save the file, and comments and trailing commas are fine. Anything you leave out keeps its default. A value the app can't use is reported, and the default stays until you fix it. `"theme": "system"` follows macOS; `"light"` or `"dark"` pins it. In `keybindings.json`, binding a key to `null` turns it off. The settings page writes to the same file and keeps your comments.

## Building it yourself

It needs [rustup](https://rustup.rs). From the repo:

```sh
rustup toolchain install   # the Rust version rust-toolchain.toml pins
cargo xtask run       # build and start the app from target/
cargo xtask install   # a release build in Applications
```

A build from the working tree is **Taco Shells Dev**, a separate app that sits beside the released one and reads its own config, `~/.taco-shells-dev`, so developing it never disturbs the copy you use. `TACO_SHELLS_CONFIG_DIR` points either app at another folder.

[AGENTS.md](AGENTS.md) covers the rest: testing, the two apps, and how versions and releases work. It's written for coding agents, but you're welcome to read it.

## License

MIT.
