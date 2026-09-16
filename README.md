# Taco Shells

A terminal manager for macOS, built with Electron.

I like VS Code's terminal manager: a sidebar list where splits show up as coupled rows, each with its own status. It's the only thing I've found that makes running two Claude Code sessions side by side legible. What I don't like is that it comes with an editor attached, and popping the terminal out into its own window leaves the sidebar behind.

So I'm building just that part as its own app.

Early. Right now it runs as many terminals as you like and lists them in the sidebar, each row named by whatever the program in it publishes as its title. You switch by clicking a row and reorder by dragging one. Splitting a terminal puts its panes side by side, with a divider you can drag, and brackets their rows together in the sidebar so a group reads as a group. A split starts in the directory the terminal you split was sitting in.

## Installing

Download the dmg from [the latest release](https://github.com/ldelvoye/taco-shells/releases/latest) and drag Taco Shells into Applications.

It won't open yet. The app is signed only ad-hoc and isn't notarised, so macOS reports a downloaded copy as damaged rather than merely unverified, and offers no way past it. One command fixes that:

```sh
xattr -dr com.apple.quarantine "/Applications/Taco Shells.app"
```

Releases marked as pre-releases are points in the project's history rather than builds to install; they carry no download.

## Jumping to the terminal that wants you

[Clawd on Desk](https://github.com/rullerzhou-afk/clawd-on-desk) shows a bubble when a Claude Code session needs you, with a "go to terminal" button. Clicking it raises Taco Shells and switches to the terminal that asked, rather than leaving you on whichever one you had open.

It needs the hook below, which is what tells Taco Shells a session is waiting. Without it nothing is ever flagged and the button only raises the app. Add it to `~/.claude/settings.json`:

```json
{
  "hooks": {
    "Notification": [
      { "hooks": [{ "type": "command", "command": "/path/to/taco-shells/hooks/taco-shells-attention.sh" }] }
    ],
    "Stop": [
      { "hooks": [{ "type": "command", "command": "/path/to/taco-shells/hooks/taco-shells-attention.sh" }] }
    ]
  }
}
```

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
