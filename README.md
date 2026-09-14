# Taqueria

A terminal manager for macOS, built with Electron.

I like VS Code's terminal manager: a sidebar list where splits show up as coupled rows, each with its own status. It's the only thing I've found that makes running two Claude Code sessions side by side legible. What I don't like is that it comes with an editor attached, and popping the terminal out into its own window leaves the sidebar behind.

So I'm building just that part as its own app.

Early. Right now it runs as many terminals as you like and lists them in the sidebar, each row named by whatever the program in it publishes as its title. You switch by clicking a row and reorder by dragging one. Splits, the part that makes a group of panes read as a group, are not built yet.

## Running it

```sh
npm install
npm run dev
```

## Building the app

```sh
npm run dist
```

Writes `Taqueria.app` and a dmg into `dist/`. The app is ad-hoc signed, not notarised, so it runs on the machine that built it.

## License

MIT.
