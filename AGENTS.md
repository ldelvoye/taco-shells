# Working in this repo

Notes for coding agents, and for anyone curious about how the thing is built. The
[README](README.md) is the user-facing half: what the app is and how to run it.

Nothing here is a style guide. It is the handful of mechanics that are not
obvious from the code, and the traps that have already cost someone an afternoon.

## Layout

Electron, TypeScript throughout, React for the window chrome only. The terminal
itself is `@xterm/xterm` over `node-pty`; none of it is our own emulation.

```
src/main/       the Electron main process: windows, ptys, config, the menu
src/preload/    the one bridge, exposed on window.tacoShells
src/renderer/   React: the sidebar, panes, and the terminal views
src/shared/     types and pure logic both sides import
e2e/            drives the real app over the Chrome DevTools protocol
scripts/        build, install and release commands
```

## Commands

```sh
npm run dev          # vite dev server plus Electron
npm test             # the pure units, ~150ms
npm run e2e          # builds, then drives the real app
npm run test:e2e     # the same, then installs it
npm run install-app  # build and install Taco Shells Dev
npm run dist         # app and dmg into dist.noindex/
npm run promote      # mark a version stable (see below)
```

**`npm run e2e` builds first, on purpose.** Driving a stale `out/` passes while
the change under test is not in it.

**`npm test` never launches anything.** The e2e suite covers only what the unit
tests structurally cannot: that a chord reaches a registered command, and that
the command reaches the screen. Do not restate the model there — it is slow and
buys nothing.

## Two apps, and which one a build becomes

A build from the working tree installs as **Taco Shells Dev**, beside the
released **Taco Shells** rather than over it, so the app in daily use is never
disturbed by the one being developed.

|           | Taco Shells               | Taco Shells Dev               |
| --------- | ------------------------- | ----------------------------- |
| bundle id | `com.ldelvoye.tacoshells` | `com.ldelvoye.tacoshells.dev` |
| config    | `~/.taco-shells`          | `~/.taco-shells-dev`          |

**The version says nothing about which is which.** `0.2.2` is the same number
whether or not it has been promoted, so the channel comes from
`TACO_SHELLS_CHANNEL`, read in `scripts/identity.cjs`. **Development is the
default** and stable has to be asked for: `install-app`, `npm run dev` and a bare
`electron-builder` all produce the dev app, and only `npm run promote` sets the
variable. The safety is in the direction of the default — anything that forgets
gives you a dev build, never a replacement for the app in daily use.

**It is baked at build time, not read at startup.** `electron.vite.config.ts`
defines `__CHANNEL__`, so the comparison in `src/main/channel.ts` is
constant-folded and the losing branch is eliminated. A dev bundle contains
`.taco-shells-dev` and no `.taco-shells`; the stable bundle is the exact inverse.

**The bundle ids differ, and that matters more than the names.** Two bundles
claiming one id leaves LaunchServices picking between them.

`scripts/identity.cjs` is CommonJS because `electron-builder.js` loads as
CommonJS and both it and the ESM scripts must read one set of identities.
`src/main/channel.ts` states the names a second time for the running app, since a
source compiled into the bundle cannot import the build's config. Those two are
the only copies.

## Versions and releasing

`main` is not assumed releasable. Features land in small batches and their fixes
arrive in later rounds, so **every push to main is versioned and released, and
some of those versions are promoted to stable.**

```
v0.1.0   ← promoted
v0.2.0     the feature
v0.2.1     cleanup
v0.2.2     cleanup, then promoted
```

**The version comes from the commits.** `bumpFor` in `scripts/version.mjs` reads
the messages since the last tag: a `feat:` is a minor, anything else a patch, a
`!` or `BREAKING CHANGE` a major — **except below 1.0.0, where it moves the minor
instead**, because reaching 1.0.0 should be a decision rather than something a
`feat!:` does on your behalf. `version.test.mjs` pins all of it.

**Tags are the source of truth. `package.json`'s version is not** — it sits at
`0.0.0` and is read only as a floor before the first tag exists. This was chosen
over a `chore(release):` commit per merge so main's history stays the commits
someone actually wrote. The cost is that `electron-builder.js` overrides
`bundleShortVersion`, `bundleVersion` and `artifactName`, which would otherwise
all come from package.json.

**Promotion is a rebuild.** `npm run promote` refuses unless HEAD is exactly on a
version tag with a clean tree, then rebuilds under the stable identity, installs
it, attaches the dmg to the release that already exists and clears its prerelease
flag. `--publish` is what makes it touch GitHub; without it nothing is published.

**"Which release is stable" is a GitHub flag, not a file.** A prerelease is
excluded from Latest and from `/releases/latest`, so that endpoint is the stable
download link and needs nothing maintaining it.

**CI never packages a bundle.** `.github/workflows/main.yml` runs `npm ci`,
`npm test`, `npm run e2e` — no electron-builder, so no Homebrew librsvg and no
signing. That is what keeps a push cheap. The runner is macOS because the e2e
suite launches a real Electron binary whose ptys run login shells, and `cwd.ts`
shells out to `lsof`.

## Traps

**`electron-builder.js` is the only name electron-builder will find.** Its search
list is `electron-builder.{yml,yaml,json,json5,toml,js,cjs,ts}` — there is no
`.config.` in any of them. Getting the name wrong **fails silently**: no error,
the defaults are used, and the bundle goes to `dist/` under a default name while
whatever sits in `dist.noindex/` is left untouched. Reading that stale bundle's
Info.plist then reports the old values and looks exactly like a config change
that did nothing. **Check the output for `loaded configuration file=` before
believing any config change took effect.**

**The build output directory is `dist.noindex/`, and the suffix is
load-bearing.** Spotlight skips a directory ending in `.noindex`, which is
Xcode's convention. Without it the packaged bundle is indexed alongside the
installed one and searching offers two results, one of which goes stale
immediately. A `.metadata_never_index` marker was tried first and does not work:
that mechanism applies to a volume, not a directory. Do not rename it to `dist`.

**`npm run build` does not touch the app you launch from Spotlight.** It
refreshes `out/`, which is what `npm run dev` loads. Only `install-app` or a
passing `test:e2e` refreshes the installed bundle. Testing a change against a
stale bundle looks exactly like the feature never landed.

**`install-app` refuses while the app is running**, because ditto over a live
bundle leaves it misbehaving until relaunch, and quitting it for you would take
the user's open shells.

**Both builds are ad-hoc signed and not notarised.** `codesign --verify`
complains and that is expected; do not go chasing it. It matters only for a
*downloaded* copy, which macOS reports as damaged until the quarantine flag is
cleared — the README says how.

**An e2e test must not assume the machine's shell.** The app opens whatever
`shellPath()` resolves — the user's login shell — and CI runs on a machine whose
login shell differs from yours and which has no shell dotfiles at all. Two tests
assumed otherwise and passed locally for weeks before the first CI run failed
them: one counted processes matching a hardcoded `zsh -l`, and one read a pane's
directory out of the sidebar row, which only carries one because a *user's*
prompt configuration publishes it as the window title. Stock macOS `/etc/zshrc`
does not. Ask the shell to report what you need instead of reading it off the
chrome.

**There is no screen-capture permission on this machine**, so an agent cannot see
the window. Anything about how it *looks* has to go to a person. Everything else
is a test: `e2e/` drives the real app, and that is where verification belongs.
