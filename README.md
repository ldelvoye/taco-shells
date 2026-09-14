# Taco Shells

A terminal manager for macOS, built with Electron.

I like VS Code's terminal manager: a sidebar list where splits show up as coupled rows, each with its own status. It's the only thing I've found that makes running two Claude Code sessions side by side legible. What I don't like is that it comes with an editor attached, and popping the terminal out into its own window leaves the sidebar behind.

So I'm building just that part as its own app.

Early. Right now it runs as many terminals as you like and lists them in the sidebar, each row named by whatever the program in it publishes as its title. You switch by clicking a row and reorder by dragging one. Splitting a terminal puts its panes side by side, with a divider you can drag, and brackets their rows together in the sidebar so a group reads as a group. A split starts in the directory the terminal you split was sitting in.

It reads `~/.taco-shells/settings.json` and `~/.taco-shells/keybindings.json`, both written on first launch holding the defaults, so the file you open is the list of everything you can change. Edits apply when you save, without a restart. Comments and trailing commas are fine. Theme follows macOS unless you pin it to `light` or `dark`.

Your file is layered over the defaults rather than replacing them, so deleting a line brings the default back. To turn a key off, bind it to `null`.

## Keys

|                               |                                                              |
| ----------------------------- | ------------------------------------------------------------ |
| `cmd+t`                       | new terminal                                                 |
| `cmd+\`                       | split the current one                                        |
| `cmd+w`                       | close the current pane                                       |
| `cmd+opt+←` / `cmd+opt+→`     | move between the panes of a split                            |
| `cmd+shift+[` / `cmd+shift+]` | move between terminals                                       |
| `cmd+b`                       | show or hide the sidebar                                     |
| `cmd+c` / `cmd+v`             | copy and paste                                               |
| `cmd+k`                       | clear                                                        |
| `cmd+,` / `cmd+shift+,`       | open settings and keybindings in a terminal, using `$EDITOR` |

## Running it

```sh
npm install
npm run dev
```

## Tests

```sh
npm test          # the pure units, ~100ms
npm run test:e2e  # builds, drives the real app, then installs it
```

The e2e run drives a real app but never puts a window on screen, so it can run while you work. When it passes it reinstalls `/Applications/Taco Shells.app`, so the Taco Shells that Spotlight opens is always the last build that passed its tests. `npm run install-app` does that step on its own.

The e2e tests launch Taco Shells, press real keys at it through Chromium's input pipeline, and read the result back out of the DOM. They cover the wiring the unit tests can't reach: that a key actually reaches a command, and that the command reaches the screen.

## Building the app

```sh
npm run dist
```

Writes `Taco Shells.app` and a dmg into `dist.noindex/`. The app is ad-hoc signed, not notarised, so it runs on the machine that built it. The directory is named that way so Spotlight skips it: otherwise the build output is indexed next to the installed app and searching for Taco Shells offers you two, one of which is stale.

## License

MIT.
