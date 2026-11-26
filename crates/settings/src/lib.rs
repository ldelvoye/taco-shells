//! Settings, themes, key bindings and commands.

mod commands;
mod config;
mod jsonc;
mod keys;
mod settings;
mod theme;

pub use commands::{CommandId, RETIRED_COMMAND_IDS, is_retired};
pub use config::{
    Config, ConfigTexts, DEFAULT_KEYBINDINGS_TEXT, DEFAULT_SETTINGS_TEXT, resolve_config,
};
pub use jsonc::{JSONC_PARSE_OPTIONS, JsoncResult, parse_jsonc};
pub use keys::{
    Binding, Chord, KeymapResolution, command_for, format_chord, parse_chord, resolve_keymap,
};
pub use settings::{CursorStyle, Settings, SettingsLayerResult, ThemeChoice, apply_settings_layer};
pub use theme::{Appearance, Palette, WindowColors, palette_for};
