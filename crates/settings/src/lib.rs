//! Settings, themes, key bindings and commands.

mod commands;
mod config;
mod jsonc;
mod keymap_edit;
mod keys;
mod settings;
mod theme;

pub use commands::CommandId;
pub use config::{
    Config, ConfigTexts, DEFAULT_KEYBINDINGS_TEXT, DEFAULT_SETTINGS_TEXT, KEYBINDINGS_FILE_NAME,
    SETTINGS_FILE_NAME, UnreadSettings, resolve_config,
};
pub use jsonc::{JSONC_PARSE_OPTIONS, JsoncResult, parse_jsonc, set_top_level};
pub use keymap_edit::{KeymapChange, edit_keymap};
pub use keys::{
    Binding, Chord, KeymapResolution, chords_for, command_for, differs_from_defaults,
    file_chord_text, format_chord, modifier_spelling, parse_chord, resolve_keymap,
};
pub use settings::{
    CursorStyle, FONT_SIZE_MAX, FONT_SIZE_MIN, SettingChange, Settings, SettingsLayerResult,
    ShellRowName, ThemeChoice, apply_settings_layer,
};
pub use theme::{Appearance, Palette, WindowColors, palette_for};
