use serde_json::{Map, Value};

use crate::jsonc::parse_jsonc;
use crate::keys::{Binding, resolve_keymap};
use crate::settings::{Settings, ThemeChoice, apply_settings_layer};
use crate::theme::Appearance;

pub const DEFAULT_SETTINGS_TEXT: &str = include_str!("../defaults/settings.jsonc");
pub const DEFAULT_KEYBINDINGS_TEXT: &str = include_str!("../defaults/keybindings.jsonc");

const SETTINGS_LABEL: &str = "settings.json";
const KEYBINDINGS_LABEL: &str = "keybindings.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigTexts {
    pub settings: Option<String>,
    pub keybindings: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnreadSettings {
    None,
    Keys(Vec<&'static str>),
    Whole,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub settings: Settings,
    pub appearance: Appearance,
    pub bindings: Vec<Binding>,
    pub default_bindings: Vec<Binding>,
    pub problems: Vec<String>,
    pub unread_settings: UnreadSettings,
    pub unread_keybindings: bool,
}

fn appearance_for(theme: ThemeChoice, system_is_dark: bool) -> Appearance {
    match theme {
        ThemeChoice::Light => Appearance::Light,
        ThemeChoice::Dark => Appearance::Dark,
        ThemeChoice::System => {
            if system_is_dark {
                Appearance::Dark
            } else {
                Appearance::Light
            }
        }
    }
}

pub fn resolve_config(texts: &ConfigTexts, system_is_dark: bool) -> Config {
    let mut problems: Vec<String> = Vec::new();

    let default_result = parse_jsonc(DEFAULT_KEYBINDINGS_TEXT, KEYBINDINGS_LABEL);
    problems.extend(default_result.problems);
    let default_keymap = match default_result.value {
        Some(map) => map,
        None => Map::new(),
    };

    let mut settings = Settings::default();
    let mut unread_settings = UnreadSettings::None;
    if let Some(settings_text) = &texts.settings {
        let user_result = parse_jsonc(settings_text, SETTINGS_LABEL);
        problems.extend(user_result.problems);

        match user_result.value {
            Some(user_settings) => {
                let layer = apply_settings_layer(&settings, &user_settings, SETTINGS_LABEL);
                settings = layer.settings;
                problems.extend(layer.problems);
                if !layer.rejected.is_empty() {
                    unread_settings = UnreadSettings::Keys(layer.rejected);
                }
            }
            None => unread_settings = UnreadSettings::Whole,
        }
    }

    let mut user_keymap: Option<Map<String, Value>> = None;
    let mut unread_keybindings = false;
    if let Some(keybindings_text) = &texts.keybindings {
        let user_result = parse_jsonc(keybindings_text, KEYBINDINGS_LABEL);
        problems.extend(user_result.problems);
        user_keymap = user_result.value;
        unread_keybindings = user_keymap.is_none();
    }

    let default_resolution = resolve_keymap(&default_keymap, None, KEYBINDINGS_LABEL);
    let resolution = resolve_keymap(&default_keymap, user_keymap.as_ref(), KEYBINDINGS_LABEL);
    problems.extend(resolution.problems);

    let appearance = appearance_for(settings.theme, system_is_dark);

    Config {
        settings,
        appearance,
        bindings: resolution.bindings,
        default_bindings: default_resolution.bindings,
        problems,
        unread_settings,
        unread_keybindings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandId, is_retired};
    use crate::keys::{command_for, parse_chord};

    fn texts(settings: Option<&str>) -> ConfigTexts {
        ConfigTexts {
            settings: settings.map(str::to_string),
            keybindings: None,
        }
    }

    fn bound_to(config: &Config, chord_text: &str) -> Option<CommandId> {
        let chord = parse_chord(chord_text).expect(chord_text);
        command_for(&config.bindings, &chord)
    }

    #[test]
    fn a_broken_settings_file_keeps_defaults_and_keymap_and_reports_itself() {
        let broken = ConfigTexts {
            settings: Some("{ \"fontSize\": }".to_string()),
            keybindings: Some("{ \"cmd+k\": null }".to_string()),
        };
        let config = resolve_config(&broken, false);

        assert_eq!(config.settings, Settings::default());
        assert_eq!(config.problems.len(), 1);
        assert!(config.problems[0].starts_with("settings.json: "));
        assert_eq!(bound_to(&config, "cmd+c"), Some(CommandId::TerminalCopy));
        assert_eq!(bound_to(&config, "cmd+k"), None);
    }

    #[test]
    fn unread_settings_names_the_refused_keys_or_the_whole_file() {
        let partly = texts(Some(
            "{ \"fontSize\": 0, \"colour\": 1, \"cursorStyle\": \"beam\", \"theme\": \"dark\" }",
        ));
        let config = resolve_config(&partly, false);
        let keys = UnreadSettings::Keys(vec!["fontSize", "cursorStyle"]);
        assert_eq!(config.unread_settings, keys);

        let broken = resolve_config(&texts(Some("{ broken")), false);
        assert_eq!(broken.unread_settings, UnreadSettings::Whole);

        let missing = resolve_config(&texts(None), false);
        assert_eq!(missing.unread_settings, UnreadSettings::None);

        let clean = resolve_config(&texts(Some("{ \"fontSize\": 14 }")), false);
        assert_eq!(clean.unread_settings, UnreadSettings::None);

        let keybindings = |text: &str| ConfigTexts {
            settings: None,
            keybindings: Some(text.to_string()),
        };
        let unparsed = resolve_config(&keybindings("{ broken"), false);
        assert!(unparsed.unread_keybindings);
        let one_bad_line = resolve_config(&keybindings("{ \"hyper+x\": \"terminal.new\" }"), false);
        assert!(!one_bad_line.unread_keybindings);
        let cleared = parse_chord("cmd+k").expect("cmd+k");
        let default_command = command_for(&unparsed.default_bindings, &cleared);
        assert_eq!(default_command, Some(CommandId::TerminalClear));
    }

    #[test]
    fn the_system_theme_follows_the_system_and_a_pinned_theme_ignores_it() {
        let system = texts(Some("{ \"theme\": \"system\" }"));
        assert_eq!(resolve_config(&system, true).appearance, Appearance::Dark);
        assert_eq!(resolve_config(&system, false).appearance, Appearance::Light);

        let light = texts(Some("{ \"theme\": \"light\" }"));
        assert_eq!(resolve_config(&light, true).appearance, Appearance::Light);
    }

    #[test]
    fn the_shipped_files_resolve_cleanly_and_match_the_built_in_defaults() {
        let shipped = texts(Some(DEFAULT_SETTINGS_TEXT));
        let config = resolve_config(&shipped, false);
        assert!(config.problems.is_empty(), "{:?}", config.problems);
        assert_eq!(config.settings, Settings::default());

        let parsed = parse_jsonc(DEFAULT_KEYBINDINGS_TEXT, KEYBINDINGS_LABEL);
        let keymap = parsed.value.expect("shipped keybindings parse");
        let command_ids: Vec<&str> = keymap.values().filter_map(Value::as_str).collect();
        let retired: Vec<&&str> = command_ids.iter().filter(|id| is_retired(id)).collect();
        assert!(retired.is_empty(), "{retired:?}");
    }
}
