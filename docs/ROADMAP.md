# Features


# Bugfixes

- The terminal sometimes goes blank: text disappears and only background fills stay (diff highlight bands, the grey block behind a Claude tool call), and the only known fix is restarting the app. It happens rarely, with no known way to trigger it. Unconfirmed guess: xterm's WebGL glyph texture atlas going stale. Backgrounds and glyphs are drawn separately, so losing only the glyphs fits. `loadWebglRenderer` in `src/renderer/terminal/registry.ts` handles context loss, but nothing ever calls `term.clearTextureAtlas()`. VS Code calls it to recover from the same symptom. Before relying on that guess, check whether forcing a full redraw (resizing the window, toggling the sidebar) brings the text back.
