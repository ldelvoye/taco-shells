# Taqueria

A work-in-progress VS Code-lite terminal manager for macOS.

I like VS Code's terminal manager. The sidebar list where splits show up as coupled rows, each with its own status, is what makes running two Claude Code sessions side by side actually legible, and nothing else I tried does it.

What I don't like is that it lives inside an editor. Popping the terminal out into its own window leaves the sidebar behind and still opens new terminals back on the first monitor, because an auxiliary window only hosts the editor area. Docking it instead costs me IDE space.

So I decided to strip VS Code down to just the terminal and run that as its own app: full screen, its own monitor, its own update cadence.

## Status

Early, and not usable yet. The build pipeline works end to end and produces a launchable signed app, but it is still an unmodified VSCodium — the strip has not started.

## How it is built

This repository contains no VS Code source. It is a patch set on top of VSCodium, which is itself a patch set on top of `microsoft/vscode`:

```
taqueria           clones VSCodium at the pinned tag
VSCodium           clones microsoft/vscode at its pinned commit
VSCodium           applies its patches (debrand, telemetry, Open VSX)
prepare_vscode.sh  applies patches/user/*.patch   <- ours
gulp + esbuild     Taqueria.app
```

`upstream.json` records which VSCodium release a build comes from and which VS Code version that implies. A scheduled workflow opens an issue when that pin falls behind; taking the update is always a deliberate choice, since nothing changes unless a build is run.

## Building

Not yet scripted. That is the next milestone.

## License

MIT. VS Code and VSCodium are both MIT licensed, and this is a derivative of both.
