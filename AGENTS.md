# Working in this repo

Notes for coding agents, and for anyone curious about how the thing is built. The [README](README.md) is the user-facing half: what the app is and how to run it.

Nothing here is a style guide. It is the handful of mechanics that are not obvious from the code, and the traps that have already cost someone an afternoon.

## Layout

Electron, TypeScript throughout, React for the window chrome only. The terminal itself is xterm.js over `node-pty`; none of it is our own emulation.

```
src/protocol/   every type that crosses between the window, the workspace and the runtime, and the compiler check that they are plain data
src/rules/      pure functions over those types: drops, names, row summaries
src/settings/   settings, themes, key bindings and commands
src/agents/     what the app knows about each coding agent
src/runtime/    the ptys, in a process of their own: sessions, working directories, foreground programs
src/workspace/  the arrangement and its reducer, opening and closing terminals, agent sessions, attention, and the save file
src/app/        the Electron side: services, the window host, and the preload bridge
src/window/     React: the sidebar, panes, and the terminal views
src/testing/    helpers for tests
walls/          the import checker and the table of what each package may import
hooks/          the scripts an agent's hooks run, shipped beside the app as files
e2e/            drives the real app over the Chrome DevTools protocol
scripts/        build, install and release commands
crates/         the Rust app that will replace this one: a Cargo workspace, one crate per layer
```

A package may import only what `walls/areas.ts` allows, and `npm test` fails on anything else. Imports across packages use the package's alias; imports inside one are relative. There is no list of exceptions: an import that crosses a wall fails the test, and the only way through is a line in the table. A line there is a decision about which layer owns something, so it is added when ownership changes and not to get an import to compile.

**The arrangement of terminals is not the window's to change.** It lives in `src/workspace/` in the main process; the window sends a `WorkspaceRequest` for each thing the user does and draws the `DrawnState` it is sent. Keyboard focus is the one thing the window moves first: `src/window/workspace/link.ts` lays its own focus move over the workspace's state until the workspace has applied it.

**The window never starts a shell.** It asks the workspace for a terminal, a split, a fork or a close; the workspace names the terminal, places it and tells the runtime to open it through the `Runtime` interface in `src/protocol/runtime.ts`. Restore runs in the main process at launch, before the window asks for anything.

**The shells run in their own process.** `out/main/runtime.js` is started with `utilityProcess`; the main process talks to it through `Runtime` (`src/app/host/runtime/proxy.ts`), and the window's keystrokes and output go to it directly over a message port the preload asks for. If it dies, the terminals go and the window says why; nothing restarts.

**The runtime holds every terminal's screen, and the window's view follows it.** Each terminal has a headless xterm screen in the runtime (`src/runtime/screens.ts`), fed by the shell's output, and the window draws it with an xterm view. Each half keeps the engine in one folder: `src/runtime/engine/` (`@xterm/headless` and its serialize add-on) behind the `Screen` interface, and `src/window/terminal/engine/` (`@xterm/xterm`) behind the `TerminalView` interface in `src/window/terminal/view.ts`. One place in each half picks the engine (`src/runtime/main.ts`, and the registry), and everything else works through the interface. The window keeps a view for every terminal on screen, and for the most recently shown others until there are eight (`src/window/terminal/shown.ts`); showing another attaches a fresh view, which loads the runtime's snapshot, and one that falls out of that set is detached and disposed. A fresh view starts scrolled to the bottom with nothing selected; what the snapshot carries beyond the text is the hidden cursor and any colors the program set. A pane with no view still has a host, and its size is worked out by a view that has drawn, so every terminal keeps the size of its pane. The window attaches to a terminal over the channel, loads its view from a snapshot of the runtime's screen, and then follows the output, resize and clear events in the order they are sent. The size and Clear are the runtime's: the window asks (`pty.resize`, `pty.clear`) and the view changes only when the answer comes back. When a program asks the terminal something (its cursor position, what kind of terminal it is), the runtime's screen answers, since a view is not always there; the view is told to stay silent on those so the shell does not get two answers (`leaveAnswersToTheRuntime`). That includes a program asking for a color: the host pushes the terminal's colors to the runtime, as it pushes the scrollback setting, and the screen answers from them. Focus is the exception: only a view knows whether it has it, so only a view reports it. `src/conformance/` is the test that holds the two engine halves to the same result.

## The three contracts

The app is three layers: the window, the workspace (the arrangement, in the main process) and the terminal runtime (the shells and their screens, in a process of its own). Each pair talks only through a set of plain-data messages in `src/protocol/`, described below. The window talks to the workspace and to the runtime, so a window is built against the first and the third of these, and could be replaced without the workspace or the runtime changing. A type in `src/protocol/plain.ts` fails the type check for a message that is not plain data.

**Window and workspace** (`src/protocol/workspace.ts`). The window sends a `WorkspaceRequest` for each thing the user does and gets a `RequestReply`. The workspace publishes the whole `DrawnState` each time it changes, with a revision number, and the window draws it. The window stores nothing. Keyboard focus is the exception to "the window changes nothing": it moves focus at once and reports it, and keeps its own move on screen until the published `focusCount` has caught up.

**Workspace and runtime** (`src/protocol/runtime.ts`, with its messages in `runtimeMessages.ts`). The workspace opens and closes terminals and hears them start, exit, get a title, and have a line submitted. It can ask a terminal's directory and what runs in it. The host pushes two settings the same way: the scrollback length and the terminal's colors. Agent status does not cross this wall: hook scripts call the app, which asks the runtime only which terminal the calling process belongs to.

**Window and runtime, the terminal channel** (`TerminalRequest` and `TerminalEvent` in `runtimeMessages.ts`). It runs over a message port, so its payloads do not pass through the main process. The window attaches to a terminal and is sent `attached`, with the terminal's size and a snapshot, then `output`, with `resized` and `cleared` at their place in the output, and `end`. Nothing is sent to a view before its snapshot. Each attach carries a number that comes back in `attached`, because a message for a view the window has let go of can still be on its way when the next view is made. `write` and `resize` are addressed by terminal and work with no view attached.

**What the terminal engine does not do yet.** These are known and none is a bug to chase without reading this first.

- A snapshot does not carry the scroll region, origin mode, the saved cursor, the line-drawing character set or tab stops. Those five are the cases in `src/conformance/cases.ts` marked `known`, which run as expected failures. A full-screen program that is running when its terminal is brought back repaints itself; one that relies on a scroll region it set earlier can draw in the wrong place until it does.
- A snapshot does not carry the cursor's shape or hyperlinks either. No test shows it: the serialize add-on has no code for them, and a headless terminal gives a test no way to read them.
- Bringing back a terminal that has no view costs as much as its scrollback is long. In `scripts/measure/README.md` a switch to a terminal with no view takes about 20 ms at the median, and 544 and 836 ms at the 95th percentile, which are the terminals with a full 100,000 lines. The snapshot is written in one synchronous call, so the runtime does nothing else during it.
- A terminal brought back starts scrolled to the bottom with nothing selected.
- A color a program has set and then asks for is answered from the theme, not from what it set. The color itself is kept and shown.
- The window may not give back everything a disposed view held. One run in `scripts/measure/README.md` made and disposed about 480 views with eight alive throughout, and the window ended about 90 MB larger than it was early in the run. Whether that is held or only not yet returned was not found out.
- A shell that is still starting when the app is killed outright can be left behind for ever, as a `spawn-helper` process with no parent: node-pty's helper waits to open a terminal whose other end is gone. When the app stops the runtime in the usual way (`kill()` on the utility process, which is SIGTERM), the runtime ends its shells itself (`src/runtime/main.ts`), so this is left only for a kill the app cannot see. It was seen on a busy machine, where a helper takes longer to start.

## Commands

```sh
npm run dev          # vite dev server plus Electron
npm test             # the pure units, ~150ms
npm run e2e          # builds, then drives the real app
npm run test:e2e     # the same, then installs it
npm run install-app  # build and install Taco Shells Dev
npm run dist         # app and zip into dist.noindex/
npm run promote      # mark a version stable (see below)
npm run record:clip  # builds, then films a portfolio clip into dist.noindex/ (-- taco for the other scene)
npm run measure      # builds, then opens many terminals in the real app and reports memory, delays and lost graphics contexts
```

**`npm run e2e` builds first, on purpose.** Driving a stale `out/` passes while the change under test is not in it.

**`npm test` never launches anything.** The e2e suite covers only what the unit tests structurally cannot: that a chord reaches a registered command, that the command reaches the screen, and what the built bundle contains. Do not restate the model there — it is slow and buys nothing.

For the Rust app, from the repo root:

```sh
cargo test           # every crate; under about 10 s once compiled
cargo fmt --all      # CI fails on anything rustfmt would change
cargo clippy --workspace --all-targets -- -D warnings
```

**CI checks the Rust app on every pull request that touches it** (`.github/workflows/rust.yml`): `cargo fmt --check`, `clippy` with every warning an error, and `cargo test`, which fails if the run takes over 20 s once compiled. Nothing in `cargo test` opens a window, starts the app or sleeps on a timer. The toolchain is pinned in `rust-toolchain.toml`, and the profiles in the root `Cargo.toml`.

**Worktrees share compiled dependencies through sccache, on this machine only.** The main checkout holds an untracked `.cargo/config.toml` setting `build.rustc-wrapper = "sccache"`, and cargo reads it from every worktree under `.worktrees/`, so gpui is compiled once rather than once per worktree. A fresh clone has none and builds without it. Keep the repo free of a tracked `.cargo/config.toml`: it would sit at the same path.

**`npm run measure` is run by hand, never by CI.** It shows a strip of a window at the right edge of the rightmost display for about a minute, uses the e2e suite's debug port so the two cannot run together, and its numbers and how to read them are in `scripts/measure/README.md`.

## Two apps, and which one a build becomes

A build from the working tree installs as **Taco Shells Dev**, beside the released **Taco Shells** rather than over it, so the app in daily use is never disturbed by the one being developed.

|           | Taco Shells               | Taco Shells Dev               |
| --------- | ------------------------- | ----------------------------- |
| bundle id | `com.ldelvoye.tacoshells` | `com.ldelvoye.tacoshells.dev` |
| config    | `~/.taco-shells`          | `~/.taco-shells-dev`          |

Each config directory holds `settings.json`, `keybindings.json` and `focus-port`, plus `workspace.json`, the workspace snapshot the app restores from, and `updates/`, the downloaded releases waiting to be applied. It also holds `hooks/`, the app's copy of the hook scripts, and `agents.json`, what the app remembers about each agent's setup.

`TACO_SHELLS_UPDATE_FEED` turns the updater on in the dev channel, which otherwise never checks, and points it at a release JSON URL in place of GitHub's latest-release endpoint. The stable build ignores it and always follows GitHub. It exists for testing the updater against a local server.

**The version says nothing about which is which.** `0.2.2` is the same number whether or not it has been promoted, so the channel comes from `TACO_SHELLS_CHANNEL`, read in `scripts/identity.cjs`. **Development is the default** and stable has to be asked for: `install-app`, `npm run dev` and a bare `electron-builder` all produce the dev app, and only `npm run promote` sets the variable. The safety is in the direction of the default — anything that forgets gives you a dev build, never a replacement for the app in daily use.

**It is baked at build time, not read at startup.** `electron.vite.config.ts` defines `__CHANNEL__`, and `src/app/channel.ts` compares it once at module scope, so the bundler folds the comparison and drops the losing channel. The dev bundle holds only the dev name and directory, the stable bundle only the stable pair, and `e2e/bundle.test.ts` checks the dev half of that on every e2e run.

**The bundle ids differ, and that matters more than the names.** Two bundles claiming one id leaves LaunchServices picking between them.

`scripts/identity.cjs` is CommonJS because `electron-builder.js` loads as CommonJS and both it and the ESM scripts must read one set of identities. `src/app/channel.ts` states the names and config directories a second time for the running app, since a source compiled into the bundle cannot import the build's config. Those two are the only copies.

## Setting up an agent's hooks

The app writes an agent's hook entries into the agent's own settings file, from the Agents menu and from a one-time offer in the window. The offer waits until the agent is seen running in a terminal: on a submitted line, `src/runtime/foreground.ts` reads that terminal's foreground programs and matches them against the setup's `commands`. `src/agents/claude/setup.ts` is the source of truth for the entries Claude Code needs; the README does not list them. The scripts in `hooks/` ship as real files (`extraResources` in `electron-builder.js`) and are copied into the config directory's `hooks/` on every launch, so each app runs its own copy.

**Only the stable app writes to real settings.** A dev build shows the menu with its action greyed out, a line under it saying why, and never offers. It writes only when both `TACO_SHELLS_CONFIG_DIR` and `CLAUDE_CONFIG_DIR` are set, which is how the e2e suite drives the flow, and how to try it by hand without touching your own settings:

```sh
mkdir -p /tmp/ts-try/claude /tmp/ts-try/app && echo '{}' > /tmp/ts-try/claude/settings.json
TACO_SHELLS_CONFIG_DIR=/tmp/ts-try/app CLAUDE_CONFIG_DIR=/tmp/ts-try/claude "/Applications/Taco Shells Dev.app/Contents/MacOS/Taco Shells Dev"
```

**A dev build cannot recognise the stable app's entries.** It must not hold the stable config directory's name (`e2e/bundle.test.ts`), so entries pointing into `~/.taco-shells/hooks/` read there as "Set up by hand".

**The settings file is edited in place, one path at a time.** `hookSettings.ts` goes through `jsonc-parser`'s `modify` so the rest of the file keeps its bytes, and `flow.ts` writes with a plain `writeFileSync` because a rename would replace a `settings.json` that is a symlink into someone's dotfiles.

## Landing

Work lands through a pull request against `main`, rebase-merged so each commit keeps its own message; nothing is pushed to `main` directly. The push to `main` is what CI versions and releases as a prerelease (next section). Making a version stable is a separate, deliberate step: `npm run promote -- --publish` on the tag. If Taco Shells is quit, this replaces the installed app. If it is running, promote publishes without installing and asks the app to fetch the release; an **Update** menu appears in the menu bar once it is ready, and **Restart to Update** swaps the bundle and reopens with the workspace restored. Nothing restarts until that click, because a restart ends whatever is running in the terminals. A dry run has no release to hand over, so it still needs the app quit. The running check is `pgrep -f` on the bundle path: run `promote` from a command line that does not itself mention `/Applications/Taco Shells.app`, or it matches its own shell.

## Versions and releasing

`main` is not assumed releasable. Features land in small batches and their fixes arrive in later rounds, so **every push to main is versioned and released, and some of those versions are promoted to stable.**

```
v0.1.0   ← promoted
v0.2.0     the feature
v0.2.1     cleanup
v0.2.2     cleanup, then promoted
```

**The version comes from the commits.** `bumpFor` in `scripts/version.mjs` reads the messages since the last tag: a `feat:` is a minor, anything else a patch, a `!` or `BREAKING CHANGE` a major — **except below 1.0.0, where it moves the minor instead**, because reaching 1.0.0 should be a decision rather than something a `feat!:` does on your behalf. `version.test.mjs` pins all of it.

**The Rust app has no versions of its own yet.** Until it replaces this one, a commit that touches only the Rust app (`crates/`, `xtask/`, `smoke/`, the Cargo files, `rust-toolchain.toml`, `rust.yml`) picks no version and is left out of the notes: `main.yml` skips a push of only those, and `OUTSIDE_THE_RUST_APP` in `scripts/repo.mjs` leaves them out of the log. When it does replace this one, it carries on the same line of tags, because `install.sh` and this app's updater both read it.

**Tags are the source of truth. `package.json`'s version is not** — it sits at `0.0.0` and is read only as a floor before the first tag exists. This was chosen over a `chore(release):` commit per merge so main's history stays the commits someone actually wrote. The cost is that `electron-builder.js` overrides `bundleShortVersion`, `bundleVersion` and `artifactName`, which would otherwise all come from package.json.

**Promotion is a rebuild.** `npm run promote` refuses unless HEAD is exactly on a version tag with a clean tree, then rebuilds under the stable identity, installs it unless the app is running (see Landing), attaches `install.sh` and the zip to the release that already exists and clears its prerelease flag. The script is the first install: it is what the README's `curl` line fetches from `/releases/latest`, and it downloads the zip. The zip is also what the in-app updater downloads. `--publish` is what makes it touch GitHub; without it nothing is published.

**Promotion rewrites the release notes**, because the ones a prerelease carries say it has no download. They are generated from the commits since the last release, and `--notes-file=some.md` replaces that body outright for a release where the list of commits is not the story. Note the consequence: setting a release's notes by hand *before* promoting it achieves nothing, since promotion overwrites them. Pass the file, or edit afterwards.

**"Which release is stable" is a GitHub flag, not a file.** A prerelease is excluded from Latest and from `/releases/latest`, so that endpoint is the stable download link and needs nothing maintaining it.

**CI never packages a bundle.** `.github/workflows/main.yml` runs `npm ci`, `npm test`, `npm run e2e`: no electron-builder, so no Homebrew librsvg and no signing. That is what keeps a push cheap. The runner is macOS because the e2e suite launches a real Electron binary whose ptys run login shells, and `cwd.ts` shells out to `lsof`.

**So the packaged app's runtime script path is checked by hand**, since a path that works from `out/` can fail from inside `app.asar`: `npm run package`, then run `dist.noindex/mac-arm64/Taco Shells Dev.app/Contents/MacOS/Taco Shells Dev` with `TACO_SHELLS_CONFIG_DIR` set to an empty folder and see that a terminal opens. The script runs from inside `app.asar`, with nothing unpacked but `node-pty`.

## Traps

**`electron-builder.js` is the only name electron-builder will find.** Its search list is `electron-builder.{yml,yaml,json,json5,toml,js,cjs,ts}` — there is no `.config.` in any of them. Getting the name wrong **fails silently**: no error, the defaults are used, and the bundle goes to `dist/` under a default name while whatever sits in `dist.noindex/` is left untouched. Reading that stale bundle's Info.plist then reports the old values and looks exactly like a config change that did nothing. **Check the output for `loaded configuration file=` before believing any config change took effect.**

**The build output directory is `dist.noindex/`, and the suffix is load-bearing.** Spotlight skips a directory ending in `.noindex`, which is Xcode's convention. Without it the packaged bundle is indexed alongside the installed one and searching offers two results, one of which goes stale immediately. A `.metadata_never_index` marker was tried first and does not work: that mechanism applies to a volume, not a directory. Do not rename it to `dist`.

**`npm run build` does not touch the app you launch from Spotlight.** It refreshes `out/`, which is what `npm run dev` loads. Only `install-app` or a passing `test:e2e` refreshes the installed bundle. Testing a change against a stale bundle looks exactly like the feature never landed.

**`install-app` refuses while the app is running**, because ditto over a live bundle leaves it misbehaving until relaunch, and quitting it for you would take the user's open shells.

**Both builds are ad-hoc signed and not notarised.** `codesign --verify` complains and that is expected; do not go chasing it. It matters only for a copy downloaded in a *browser*, which macOS reports as damaged until the quarantine flag is cleared. `install.sh` avoids that because `curl` does not quarantine what it fetches, and the README covers the by-hand path. No dmg is shipped: one can only be downloaded in a browser.

**An e2e test must not assume the machine's shell.** The app opens whatever `shellPath()` resolves — the user's login shell — and CI runs on a machine whose login shell differs from yours and which has no shell dotfiles at all. Two tests assumed otherwise and passed locally for weeks before the first CI run failed them: one counted processes matching a hardcoded `zsh -l`, and one read a pane's directory out of the sidebar row, which only carries one because a *user's* prompt configuration publishes it as the window title. Stock macOS `/etc/zshrc` does not. Ask the shell to report what you need instead of reading it off the chrome.

**There is no screen-capture permission on this machine**, so an agent cannot see the window. Anything about how it *looks* has to go to a person. Everything else is a test: `e2e/` drives the real app, and that is where verification belongs.
