# Working in this repo

Notes for coding agents, and for anyone curious about how the thing is built. The [README](README.md) is the user-facing half: what the app is and how to install it.

Nothing here is a style guide. It is the handful of mechanics that are not obvious from the code, and the traps that have already cost someone an afternoon.

## Layout

Rust throughout, in one Cargo workspace. The window is [gpui](https://github.com/zed-industries/zed/tree/main/crates/gpui); the terminal emulation is Ghostty's library, not our own.

```
crates/protocol/       every type that crosses between the window, the workspace, the services and the runtime
crates/identity/       what the programs and the tooling agree on: each channel's names, the programs in a bundle, the release names, the launch variables
crates/workspace/      the owner of the terminals' arrangement, their agent rows and sessions, the debounced save and Agent Activity; takes inputs and returns outbound messages, no IO beyond the storage it is given
crates/rules/          pure functions over an arrangement and the panels: each change takes the state and returns the one it becomes, or none
crates/agents/         what the app knows about each coding agent, one harness table per agent; under claude/, Claude Code's readers for hook signals, titles, questions and activity entries, the activity tool names, reduced_payload, and the finders for what an agent shares
crates/settings/       reading, checking and editing settings.json and keybindings.json (JSONC kept as written), each setting written once, the keymap, the themes
crates/services/       the background services: the config store and its guarded writer, the hook listener on the focus port (hook_listener/), Agent Activity setup (agent_setups/), the updater (updater/)
crates/plugin/         taco-shells-plugin, the program Claude Code runs as this app's plugin: the MCP server for note, now and share, the PreToolUse allow, the hooks, and its posts to the listener (http.rs, endpoint.rs); ancestors.rs is its one Mac-only module, and program_path() is where the program sits in the plugin folder
crates/runtime/        taco-shells-runtime: every terminal's shell and screen, in a process of its own
crates/host/           what runs behind the window: the runtime client, the workspace and services hosts, the panic supervision, the terminal settings sent to the runtime, the start and quit order
crates/wire/           the framing between the app and the runtime, and the runtime's launch arguments
crates/ghostty/        Ghostty's terminal library (libghostty-vt), fetched prebuilt and wrapped for the runtime and the terminal view
crates/terminal-view/  the terminals in the middle of the window: silent copies of the runtime's terminals, the one on screen drawn, keys, mouse and input method turned into writes
crates/app/            taco-shells, the gpui window: draws the state it is sent and turns clicks and keys into requests
crates/drive/          the messages the smoke tests send a running development build, and its replies
xtask/                 the build, install, release and promote commands, as cargo xtask; no dependencies beyond crates/identity
smoke/                 the tests that drive the real app, and the measure tool
resources/             update-helper.sh, which the updater runs to swap the bundle
branding/              the icon master and the script that renders it
install.sh             the first install, attached to every stable release
```

## The layers and their contracts

Two processes live as long as the app: the app itself (`taco-shells`) and the runtime (`taco-shells-runtime`), which holds every shell and screen. A third program, the plugin (`taco-shells-plugin`), is started by Claude Code, once for each hook and once as the tool server, and lives no longer than that. Inside the app process, the window (`crates/app`) draws on gpui's thread, and behind it the hosts (`crates/host`) run the workspace and the services on threads of their own.

**The app and the runtime exchange only protocol messages.** The app starts the runtime and hands it two Unix socket pairs, at descriptors 3 and 4, through launch arguments that `crates/wire` writes and reads. Descriptor 3, the control connection, carries the workspace's commands (open, close, resize a terminal; the settings the shells need) and the runtime's reports (started, exited, a title, a submitted line). Descriptor 4, the terminal channel, goes to the window's views, so terminal output never passes through the workspace. Each message is one frame, a little-endian `u32` length then postcard, at most 64 MiB. Nothing listens on a path. A window attaches only terminals the runtime has reported started. An attach for any other id is answered with End, and the view does not ask again until the row leaves the arrangement.

**The window and the hosts meet at two traits** in `crates/host/src/ports.rs`: `WorkspacePort` (the window's requests to the workspace) and `ServicesPort` (its actions on the services: setup, updates, settings and key bindings), with everything coming back as one `WindowOutput` stream to the window's thread. Their data are protocol and settings types. The window owns no layout: the workspace publishes a `DrawnState` (rows, groups, panels, agent rows) and the window draws it. Every change the user makes, a drag, a rename or a close, goes out as a `WorkspaceRequest` and comes back as the next state. Keyboard focus is the one exception: a focus move is drawn at once, ahead of the workspace, and dropped once a published state has applied it.

**The services reach the workspace through the workspace host's inputs**, not through protocol messages. A hook post becomes a `Hook` input; the two questions a session start and a stop ask the app (`SessionContext`, `StopNudge`) are inputs that carry a reply channel, answered on the workspace's thread.

**Agents and the app.** Agents in this app's terminals reach it through the plugin's hooks, which post to the listener on the focus port with the token from `focus-token`. The plugin's hook lists its own ancestors, from Claude Code up to six processes, and sends them as `pids`; the host asks the runtime which of its terminals has a shell among them, the first match wins, and a post that matches no terminal is dropped (a question is answered as not one of the app's terminals). The posts' shapes are the typed contract in `crates/protocol/src/hooks.rs`.

**The only wall is each crate's `Cargo.toml`.** Nothing checks imports beyond it, so a dependency added there is a decision about which layer owns something. The directions that matter:
- `protocol` and `identity` are at the bottom and depend on nothing of ours. `wire` depends on nothing either.
- `agents` sits below `workspace` and `services`. `services` depends on `plugin`, whose folder it writes.
- `host` depends on neither `agents`, `runtime` nor `ghostty`: it starts the runtime as a program and speaks to it through `wire`.
- `app` never depends on `runtime`, `wire` or `ghostty`; it reaches the runtime through `host`, and a terminal's drawing through `terminal-view`.
- `xtask` depends only on `identity`, and `smoke` only on `identity`, `drive` and `protocol`.

## What must not change by accident

- **The stored files.** `workspace.json`, each terminal's `activity/<id>.jsonl` and its `<id>.state.json` are protocol types written as they are, so renaming a variant or a field is a format change. The golden tests in `crates/workspace` hold each one: `a_save_this_build_wrote_reads_back_whole_and_writes_the_same_text` (`save.rs`), `a_log_this_build_wrote_reads_back_whole_and_writes_the_same_lines` (`log.rs`) and `a_state_file_this_build_wrote_reads_back_whole…` (`board.rs`). A pull request that changes one changes its golden text on purpose and says so.
- **The hook wire.** `crates/services/tests/hook_contract.rs` pins what the plugin posts and what the listener reads. Any field added to the contract must be optional with a default, because a shell opened before an update keeps running the plugin it started with.
- **The app-to-runtime wire** needs no versioning: both programs come from one bundle, and the runtime ends with the app.

## Adding things

- **A panel:** a variant in `Panel` and an entry in `Panel::ALL` (`crates/protocol/src/panels.rs`). The window's matches on `Panel` fail to compile until it is drawn.
- **A setting:** a field and its default in `Settings` and a variant in `SettingChange` (`crates/settings/src/settings.rs`). `SettingChange::all` destructures `Settings`, so a field it leaves out does not compile. Then its key in `SettingChange::key`, its parse in `read_setting`, its store in `Settings::apply`, its default in `crates/settings/defaults/settings.jsonc`, and its row on the settings page. `every_setting_reads_back_what_it_writes` pins the key, the parse and the store to one another.
- **A command:** a variant in `CommandId` (`crates/settings/src/commands.rs`), its default chord in `crates/settings/defaults/keybindings.jsonc` if it has one, and its class in the exhaustive `match` in `crates/app/src/key_route.rs`, which fails to compile until it is classed.
- **A hook route:** its path in `taco_protocol::routes`, its body type in `crates/protocol/src/hooks.rs`, the plugin's post and the listener's handling, and a case in `hook_contract.rs`.
- **A request from the window to the workspace:** a variant in `WorkspaceRequest` (`crates/protocol/src/workspace.rs`); the workspace's match on it fails to compile until it is handled.
- **A second coding agent:** a `Harness` in `crates/agents` (`harness.rs`: its readers for signals, titles, resume lines, questions, entries, version, hook fields and start source, and an optional `HarnessSetup`), and its entry in `HARNESSES` (`crates/agents/src/lib.rs`). Claude Code's is `CLAUDE` in `claude/mod.rs`.

## The host

`crates/host` keeps the window's thread from waiting on anything that can stall:
- **The runtime's commands** go through a bounded queue of 4096. A runtime that leaves it full for 5 s has stopped reading, so it is killed and the window says "The terminals stopped responding and were closed." Once the runtime is gone, a question to it gets `None`.
- **Quitting** sends `Stop`, then SIGTERM, then SIGKILL, about 4 s at worst, and shows no banner. The final save runs on its own thread, with 1.5 s of patience, before the services and then the runtime stop.
- **Hook posts and questions have deadlines**, named in `taco_protocol::deadlines` (`crates/protocol/src/hooks.rs`), where a compile-time assert keeps them in order. A post waits 2 s for the runtime to place it.
- **A panic** in the workspace thread, the hook route or the runtime-events forwarder is caught (`supervised.rs`, `workspace_host.rs`) and shown as a banner. After a workspace panic, callers waiting on it are answered without a save.
- **The four terminal settings** (scrollback, colors, cursor, the agent plugin) go to the runtime from the host, not from the window.

Only the runtime's commands are bounded. The workspace's inputs, the window's stream and the runtime's events are not.

## The runtime

The runtime owns every terminal's screen: a Ghostty terminal in its one core thread, fed by the shell's output, which also answers a program's questions. A view never answers them. A view attaches to get a snapshot (the screen part, then history in parts), then follows the output, resizes and clears in the order sent. The control connection closing, a `Stop` command or SIGTERM ends every shell with SIGHUP, and the runtime exits: 0 after a stop, 2 for bad arguments, 101 for a panic.

Two rules keep it working. Only the core thread opens ptys and starts shells, since close-on-exec is set after a pty's descriptors are created. And each shell may have at most 1 MB of output read and not yet passed on, after which its reads stop, so one flooding terminal slows only itself.

## The terminal view

Each kept terminal has a `View`: a Ghostty terminal of its own that never answers a program, fed only by the terminal channel. A view sends `Attach` with a number of its own and ignores everything until the `Attached` carrying that number arrives; the snapshot is drawable at once and its history fills in one page per frame. After that, `Output`, `Resized`, `Cleared` and `CursorDefaults` are applied in the order they arrive, so each lands at its place in the output. The size is the runtime's: the view asks with `Resize` and changes nothing until `Resized` comes back.

Views go to the terminal on screen, then the most recently shown others, eight in all, and at most one attach per terminal is outstanding, which bounds the snapshots the runtime holds. The decisions live in `Views` and `View`, which tests drive with no window; `Terminals` only wires them to gpui.

## The window

The focus move drawn ahead of the workspace is `link::Ahead`, dropped once a published state has applied it (`drawn_with`).

Keys go through `key_route::route`, one pure function that decides whether a shortcut being set, the name field, a page, an opened thing or a command gets the key. Every `CommandId` is classed there in one exhaustive match, so a new command does not compile until someone decides what it does over a page. Links go through `addresses::may_open`, one check for a link clicked in a terminal and a shared one. It accepts only an allowed scheme, a host with no user part, and no space, control or direction character.

The window keeps its view state in `window/`, one file per part, each holding that part's state struct and its methods on `WindowView`:
- `settings_state.rs` (`SettingsState`): the typed fields, and what was written or refused.
- `shortcuts_state.rs` (`ShortcutsState`): the search, and the shortcut being set.
- `activity_state.rs` (`ActivityState`): the log, what is unfolded, and the thing opened in the middle.
- `pages.rs` (`PagesState`).
- `keyboard.rs` (`KeyboardState`): the window's focus, and the flags a part sets to ask for the keyboard, which `show_terminals` reads on the next frame.
- `arrangement.rs` (`ArrangementState`): drags, resizes, the slot and the popup.

A part asks for the keyboard only through those flags. Routing (`key_down`, `show_terminals`) and drawing (`layout.rs`, `render`) read several parts by nature.

Small parts drawn in several places have one copy each:
- `parts.rs`: element ids, the bordered button, the close button and the problem card.
- `page_parts.rs`: the pages' frame, header, groups, foot and key chips.
- `page.rs`: the stack of pages and how each move changes it.
- `settings_page/controls.rs`: the segmented choice, the switch and the stepper.
- `typed_setting.rs`: what a typed field's text means.
- `activity_panel/step_lines.rs` and `setup_line.rs`: the step text and the setup card.

## The workspace and Agent Activity

Agent Activity groups an agent's steps into stretches, each named by the `seq` of its first step. Only the agent's note closes a stretch and takes its steps. A new turn (a new or missing `prompt_id`) or a new session id ends one with no note, and a compaction keeps it, since the session id is the same. A subagent's step that arrives late joins its own turn's last stretch.

Each terminal keeps `activity/<id>.jsonl` and `<id>.state.json`, and the save is `workspace.json`. All three are mode 0600 and replaced by writing a temporary file and renaming it. A quit or a runtime death freezes the workspace, which keeps every log for the next launch; closing a terminal deletes its log and state file.

The three files hold protocol types written as they are. The golden tests in `log.rs`, `board.rs` and `save.rs` must still read back, so a pull request that changes one changes its golden text on purpose and says so.

## Agent Activity setup and the updater

The plugin is loaded per terminal: while `agentActivity` is on, every new shell gets `<config dir>/claude-plugin` added to the plugin variable of each harness that has one (`CLAUDE_CODE_PLUGIN_DIRS` for Claude Code), and nothing is installed into Claude Code. The host hands the runtime each variable and folder at launch (`--list-entry=<variable>=<path>`) and switches them with `ListEntry`, so the runtime itself names no agent's variable. The app writes that folder itself, refreshed by byte compare and replaced by rename, never in place, and never deletes it. The plugin acts only where `TACO_SHELLS_APP` names its own build and the run is not headless, so it is silent everywhere else. Every post to the listener carries the token from `focus-token`, beside `focus-port`. Any field added to the hook contract must be optional with a default, since a shell opened before an update keeps its old plugin.

Set up is the only time the app edits the agent's `settings.json`, and a development build makes that edit too, on the click. It removes only this build's own old hook entries and, when the user's settings set `CLAUDE_CODE_PLUGIN_DIRS` (a settings `env` block replaces the shell's value), appends this build's folder to that one string and records it; switching off restores the line only while it is still exactly what the app wrote. To try it without touching your own settings, start the Dev app with `CLAUDE_CONFIG_DIR` pointing at a scratch folder. Any change touching Claude Code's environment needs one headless run with the real setting sources, reading the init message's plugins list; a trial value can be layered with `--settings <scratch file>`.

The updater accepts a release only if its size matches, its sha256 matches GitHub's digest (a release without one is accepted on size and logged), and its `.sig` verifies with `/usr/bin/ssh-keygen -Y verify` against `taco_identity::RELEASE_KEYS`. A signature refusal is the one failed check the window shows, once per version; every other failure is logged only. A dev build checks a feed only when `TACO_SHELLS_UPDATE_FEED` is set, and also accepts the key at `TACO_SHELLS_UPDATE_KEY`; a stable build holds neither name.

## Commands

From the repo root:

```sh
cargo test                     # every crate; under about 10 s once compiled, and CI fails past 20 s
cargo fmt --all                # CI fails on anything rustfmt would change
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings   # the code only tooling turns on: render, drive, capture, bindings
cargo xtask run                # build the programs and start the app from target/
cargo xtask bundle             # the .app into target/bundle.noindex/; --release, --stable, --zip, --config-dir <absolute path> (dev only)
cargo xtask install            # release build of Taco Shells Dev, into /Applications
cargo xtask smoke              # the tests that drive the real app, against a hidden development bundle
cargo xtask rehearse-update --until-ready   # a Rust build updating itself to a newer one, on a throwaway config
cargo xtask measure            # memory and delays in the real app, by hand, never in CI
cargo xtask release --dry-run  # the prerelease the next push to main would cut
cargo xtask ghostty-bindings   # regenerate the Ghostty bindings after moving its pin
```

`cargo xtask promote` and `cargo xtask try-stable` are Lucas's alone (below).

**A bundle** holds `taco-shells`, `taco-shells-runtime` and `taco-shells-plugin` side by side in `Contents/MacOS/`, with the icon and `update-helper.sh` in `Contents/Resources/`. All three programs come from one `cargo build`, so they share a channel. `CFBundleShortVersionString` is always a plain `X.Y.Z`, because the updater reads it: the tag the build is on, or the last tag before it. `CFBundleVersion` holds what `git describe` says.

**`cargo xtask smoke` drives the real app**, launched once from a development bundle built with the `drive` feature, on a fresh config under `target/smoke.noindex/`. The app runs hidden (`TACO_SHELLS_BACKGROUND_WINDOW=1`: no Dock icon, no focus, a window never shown). Only a development build honours that variable; a stable build always shows its window. The tests talk to it over a Unix socket in `$TMPDIR`; a socket path must stay under 104 bytes, so not in a deep scratch folder. A `drive` build takes input from that socket and writes images where it is told, so it is never installed and never stable. The look images land in `target/smoke.noindex/` as `look-light.png` and `look-dark.png`. Run smoke from a checkout of the exact commit it should vouch for, never in a worktree that is switching branches, and never two at once on one build folder.

**Agents look at the window without a screen** in two ways: smoke's images, and `cargo run -p taco-app --features render --example render -- <fixture> <light|dark> <out.png> [width:N] [panel:N] [click:X,Y] [key:K] [hold:X,Y:X,Y] [wait:MS] …`, which draws a made-up state from `fixture.rs` into a PNG. The render stand-in drives the real `taco_workspace::Workspace`, kept in memory, and every fixture is built only through the inputs the host sends: requests, runtime reports and hooks, with hook payloads trimmed by `taco_agents::reduced_payload` as the plugin trims them live. Nothing is seeded into the workspace's files, so a render shows what the app would show; a state no input reaches is not added through a back door, the fixture changes instead. No fixture yet draws a pull request pill for a draft, a merged or a closed pull request, so whoever next changes that pill adds one.

The same arguments give the same bytes. A change that should not alter the look is checked by building the renderer before and after, rendering every fixture in both themes plus the gestures with both binaries back to back, and comparing with `cmp`. Times follow the clock, so render each pair back to back with a fixed `TZ`, or a run near midnight turns earlier times into dates in one binary and not the other. A render taken mid-animation is not byte-stable: end a `hold:` drag of a row with `wait:600`, and check a mid-slide segmented card by eye. For one terminal, `cargo run -p taco-terminal-view --features capture --example look -- <dir> [bytes file]` draws it in both themes.

**Worktrees share compiled dependencies through sccache, on this machine only.** `.worktrees/.cargo/config.toml` in the main checkout sets `build.rustc-wrapper = "sccache"`, and cargo reads config from every parent directory, so every worktree picks it up beside the tracked `.cargo/config.toml`. A fresh clone has none and builds without it.

## Two apps, and which one a build becomes

A build from the working tree installs as **Taco Shells Dev**, beside the released **Taco Shells** rather than over it, so the app in daily use is never disturbed by the one being developed.

|                | Taco Shells               | Taco Shells Dev               |
| -------------- | ------------------------- | ----------------------------- |
| bundle id      | `com.ldelvoye.tacoshells` | `com.ldelvoye.tacoshells.dev` |
| config         | `~/.taco-shells`          | `~/.taco-shells-dev`          |
| plugin, `TACO_SHELLS_APP` | `taco-shells`  | `taco-shells-dev`             |

The config folder holds `settings.json`, `keybindings.json`, `workspace.json`, `activity/`, the plugin folder `claude-plugin/`, `claude-plugin-dirs.json` (the record Set up writes when it changes the user's `CLAUDE_CODE_PLUGIN_DIRS` line), `focus-port` and `focus-token`, and `updates/`, the downloaded release waiting to be applied.

**The channel is chosen at compile time, and development is the default.** `TACO_SHELLS_CHANNEL=stable` is read by `crates/identity`'s build script and nothing else; `cargo xtask bundle --stable` sets it and every other build removes it, so a variable left in a shell cannot change what is built. It is not a cargo feature, so `--all-features` can never produce the stable app. The names live once, in `crates/identity`; CI fails on a config folder name written anywhere else.

**`TACO_SHELLS_CONFIG_DIR`** overrides the config folder at launch in every build. A development build can also carry one compiled in (`bundle --config-dir`), which the update rehearsal needs because the update script reopens the app with `open`, which passes no environment.

## Landing

Work lands through a pull request against `main`, rebase-merged so each commit keeps its own message; nothing is pushed to `main` directly. A push to `main` runs the check and then `cargo xtask release`, which cuts a prerelease with no download (next section). Making a version stable is a separate, deliberate step that only Lucas takes.

## Versions and releasing

`main` is not assumed releasable. Features land in small batches and their fixes arrive in later rounds, so **every push to main is versioned and released as a prerelease, and some of those versions are promoted to stable.**

**The version comes from the commits** since the last `v*` tag: a `feat:` is a minor, anything else a patch, a `!` or a `BREAKING CHANGE:` footer a major, **except below 1.0.0, where it moves the minor instead**, because reaching 1.0.0 should be a decision rather than something a `feat!:` does. `xtask/src/bump.rs` holds the rules and their tests. Tags are the source of truth; the Cargo workspace's `version` is never read.

**Promotion is a rebuild, and it is Lucas's.** `cargo xtask promote` refuses unless HEAD is exactly on a version tag with a clean tree. It runs the tests and smoke, builds the stable bundle and zip, and checks the bundle with `taco-smoke --stable` (the stable names, every release key, no dev name or extra-key variable). Without `--publish` it installs the bundle, and refuses to start while the stable app is running. With `--publish` it installs too, unless the app is running, which is then left to update itself from the release; and it signs the zip with `ssh-keygen -Y sign` and the key in `~/.config/taco-shells-release/`, verifies the signature against the app's own keys before anything changes, attaches `install.sh`, the zip and its `.sig`, marks the release Latest, and asks a running app to look for it. `--notes-file` replaces the generated notes. **No agent runs `promote` in any form, a dry run included (it installs over `/Applications/Taco Shells.app`), and no agent reads the key folder.**

**The release key is unlocked by its passphrase, typed at each promotion**: `ssh-keygen` asks for it on the terminal running `promote --publish`, and nothing stays unlocked between promotions. Loading the key into ssh-agent through the Keychain would remove the prompt, and was not chosen: any process running as Lucas, an agent's included, could then ask the agent to sign a release.

**`cargo xtask try-stable`** opens the stable build, shown, on copies of `~/.taco-shells` and Claude Code's `settings.json`, then deletes the bundle. It is for Lucas to look over a build before promoting it; no agent runs it.

**"Which release is stable" is a GitHub flag, not a file.** A prerelease is excluded from Latest and from `/releases/latest`, so that endpoint is the stable download link: `install.sh` reads it, and so does the app's updater.

**Signing keys rotate through `RELEASE_KEYS`**, which holds at most two. A new key comes in with a release signed by the old one that carries both; the next release is signed with the new one. A lost key means every user reinstalls through `install.sh`.

## Moving the pins

**gpui** has no crates.io release newer than 0.2.2, so `gpui` and `gpui_platform` pin one zed revision. To move it, change both `rev`s together, set `rust-toolchain.toml` to the channel zed's own `rust-toolchain.toml` names at that revision, copy zed's `[patch.crates-io]` `async-task` revision from the same commit, run clippy and the tests, and compare `look` images in both themes against the previous ones. `gpui_platform` needs `font-kit` (without it nothing draws, silently when headless) and `runtime_shaders` (so no Xcode `metal` compiler is needed). Never draw inside an update of the view a window is rooted on; go through `AnyWindowHandle`. The pinned zed source is the only gpui reference that matches.

**Ghostty**: set `VERSION` and `SHA256` in `crates/ghostty/build.rs` (the hash is the one in the release's `SHA256SUMS`), run `cargo xtask ghostty-bindings`, check the snapshot record constants in `crates/ghostty/src/snapshot.rs` against `snapshot.ksy` in the new source tarball, and run `cargo test -p taco-ghostty`, whose version test fails if the bindings were not regenerated. `TACO_GHOSTTY_DIR` points the build at a library built by hand.

## Rules for agents

- **Every launch of the app by an agent is hidden** (`TACO_SHELLS_BACKGROUND_WINDOW=1`). Nothing goes on Lucas's screen; anything about how the window looks goes to a person, or to the images above.
- **Never install over, or write into, the apps and folders in daily use**: `/Applications/Taco Shells.app`, `/Applications/Taco Shells Dev.app`, `~/.taco-shells`, `~/.taco-shells-dev`, `~/.claude`. `cargo xtask install`, the rehearsal's click-through, `promote` and `try-stable` are Lucas's.
- **Kill only processes you started, by pid.** Never `pkill -f` or `killall`: several sessions share this Mac.
- **No "N times faster" against the Electron app's figures.** `cargo xtask measure` ends a delay when the change reaches the view's state, not a shown frame.

## Traps

**`.noindex` folders are load-bearing.** Spotlight skips a folder ending in `.noindex`, so built bundles under `target/bundle.noindex/` are not offered as second copies of the app. Do not rename them.

**Both builds are ad hoc signed and not notarised.** `codesign --verify` complains, and that is expected. It matters only for a copy downloaded in a browser, which macOS reports as damaged until the quarantine flag is cleared; `install.sh` avoids that because `curl` does not quarantine what it fetches. A release's own signature is the `.sig`, checked by the updater, not the code signature.

**A GUI app gets launchd's `PATH`, with no Homebrew.** Anything the app runs is named by absolute path (`/usr/bin/ssh-keygen`, `/usr/bin/ditto`), and a shell it starts gets the user's login environment.

**The app is built with unwinding panics, and the host relies on it.** A panic in the workspace thread, the hook route or the runtime-events forwarder is caught and shown as a banner. With `panic = "abort"` in any profile, each of those becomes a crash with nothing shown.

**`ssh-keygen` asks the SSH agent whenever `SSH_AUTH_SOCK` is set**, even for a key file it is given, and that can wait for an approval forever. Every test that signs or generates a key runs it with a cleared environment, closed stdin and a deadline; only the real `promote --publish` keeps the environment, for the passphrase prompt.

**An agent's own Claude Code variables leak into what it runs.** Smoke runs the plugin with an empty environment for that reason; anything that starts the plugin or `claude` from an agent's shell must decide which variables it passes.

## Known limits

Known and not bugs to chase without reading this first.

**Runtime.**
- Closing a terminal, or quitting, sends only SIGHUP: a job that ignores it (with `nohup`, `disown` or in `tmux`) keeps running with no terminal.
- A snapshot waiting to go to a view that is not reading the terminal channel is not counted against any budget. The window attaches eight views at most, so it stays bounded.
- A shell's writer thread blocks for good if a process that outlived the shell holds the pty and stops reading; the thread and its pty descriptor stay until that process goes.
- A screen of about 3 million styled cells (3000 by 1000) is too large for one frame, so a view of it never attaches; the runtime logs it. Real windows are about 100,000 cells.
- The plugin folder cannot be put in `CLAUDE_CODE_PLUGIN_DIRS` if its path holds a `:`, as a home folder named with one would.
- A shell startup file that sets `CLAUDE_CODE_PLUGIN_DIRS` outright, rather than appending to it, takes the plugin away from that user's terminals without a word.

**Terminal view.**
- Only the left button and the vertical wheel are reported to a program tracking the mouse.
- A Cmd-click opens a link, but hovering with Cmd held does not underline it.

**Window.**
- A shifted symbol is read back by a US-layout table, so on another layout a chord typed with a symbol may resolve to the wrong binding. US only, by Lucas's ruling.
- An opened image is capped by its file size (20 MB), not its pixel size, so a huge SVG can hold hundreds of megabytes while shown.
- A markdown document opens as plain text.
- A shared text's tabs become spaces at stops of four columns counted in characters, not display cells, so a tab after wide characters lands a stop early.
- The activity log builds an element for every loaded line on each redraw, with no windowing, and "Show older" grows what is loaded without bound.

**Workspace and Agent Activity.**
- The artifact payload (`tool_response.url`, `title`) is read from session transcripts and is unconfirmed on a live hook until its first real use.
- An agent that creates fifty markdown files adds fifty things to Shared and fifty Shared lines to the log; nothing groups the app's own finds below Shared's cap of 100.
- `title_of` is narrower than a letter-and-mark pattern; Claude Code's glyphs are unaffected.
- After a failed restore (an unreadable save, copied aside), no activity log is deleted until the next successful restore or a launch with `restoreOnLaunch` off.
- A terminal has one open stretch, so two agent sessions running at once in one terminal mix their steps, and either one's note takes both.
- A state file from a newer build reads as an empty board and is replaced at the next write, so going back to an older build drops Shared for the terminals open at the time.

**Services and the plugin.**
- The listener reads one request at a time, each with a 1 s read deadline, so a local process that keeps opening connections and sending nothing can delay or drop hook posts while it lasts; a question's answer is written on its own thread and never holds the loop.
- A question's 1 s answer window starts when the listener reads its head, so under a busy listener a nudge can be written after the hook's 1.5 s and the app records an ask the agent never saw.
- The Stop nudge relies on Claude Code honouring the block: a nudged stop sends none of its posts.
- A shell opened while Agent Activity was on keeps loading the plugin after it is switched off: its agents keep the tools and their notes still reach the panel; only the panel text and the nudge stop.
- In a tmux or screen pane started inside a terminal, the plugin lists its tools but the listener cannot place the pane, so nothing it writes reaches a panel.
- Only the user and managed settings files are read for `CLAUDE_CODE_PLUGIN_DIRS`; a project file or a managed drop-in that sets it is caught only by the "didn't load this app's plugin" line, which can also flash briefly after a switch flip or a start slower than 5 s.
- The plugin-dirs record is one file per config folder, and a malformed one reads as none; an entry left while the switch was already off stays.
- Editing the agent's settings: a file starting with a BOM is refused; removal reflows a one-line file; a read-only file keeps its mode through the replace.
- Adding a key to a one-line `settings.json` spreads it over several lines.
- A hand edit saved by an editor that empties the file and then rewrites it can still be lost if the settings page writes between the two.
- A config file turned into a symlink after launch is not followed until the next launch.
- The Unix-only pieces of `services` (file modes, symlinks in tests) are not gathered behind one platform module; the plugin's are, in `ancestors.rs`.

**Build and tests.**
- The driven binary is built with gpui's test support, so the shipped stable binary is never driven; `taco-smoke --stable` checks what it can of it without launching it.
- `cargo xtask measure` ends each delay when the change reaches the view's state, not when a frame shows it, and its memory leaves out a shown window's drawing, since the window is never shown.
- `install.sh` does not verify the release's signature: the script comes from the same release it would check.
