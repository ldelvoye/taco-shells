# Taqueria

A terminal manager for macOS, built with Electron.

I like VS Code's terminal manager: a sidebar list where splits show up as coupled rows, each with its own status. It's the only thing I've found that makes running two Claude Code sessions side by side legible. What I don't like is that it comes with an editor attached, and popping the terminal out into its own window leaves the sidebar behind.

So I'm building just that part as its own app.

Early. Right now it runs as many terminals as you like and lists them in the sidebar, each row named by whatever the program in it publishes as its title. You switch by clicking a row and reorder by dragging one. Splitting a terminal puts its panes side by side, with a divider you can drag, and brackets their rows together in the sidebar so a group reads as a group. A split starts in the directory the terminal you split was sitting in.

Still missing: any configuration, so the keys below are the keys, and a light theme.

## Keys

| | |
| --- | --- |
| `cmd+t` | new terminal |
| `cmd+\` | split the current one |
| `cmd+w` | close the current pane |
| `cmd+opt+←` / `cmd+opt+→` | move between the panes of a split |
| `cmd+shift+[` / `cmd+shift+]` | move between terminals |
| `cmd+b` | show or hide the sidebar |
| `cmd+c` / `cmd+v` | copy and paste |
| `cmd+k` | clear |

## Running it

```sh
npm install
npm run dev
```

## Tests

```sh
npm test          # the pure units, ~100ms
npm run test:e2e  # builds, then drives the real app
```

The e2e tests launch Taqueria, press real keys at it through Chromium's input pipeline, and read the result back out of the DOM. They cover the wiring the unit tests can't reach: that a key actually reaches a command, and that the command reaches the screen.

## Building the app

```sh
npm run dist
```

Writes `Taqueria.app` and a dmg into `dist/`. The app is ad-hoc signed, not notarised, so it runs on the machine that built it.

## License

MIT.
