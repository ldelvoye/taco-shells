use serde_json::{Map, Value};

const THEME_REQUIREMENT: &str = "must be \"system\", \"light\" or \"dark\"";
const FONT_FAMILY_REQUIREMENT: &str = "must be a non-empty string";
const FONT_SIZE_REQUIREMENT: &str = "must be a number between 1 and 72";
const SCROLLBACK_REQUIREMENT: &str = "must be a whole number of lines, 0 or more";
const SCROLL_SENSITIVITY_REQUIREMENT: &str = "must be a number greater than 0";
const BOOLEAN_REQUIREMENT: &str = "must be true or false";
const CURSOR_STYLE_REQUIREMENT: &str = "must be \"block\", \"underline\" or \"bar\"";
const SHELL_ROW_NAME_REQUIREMENT: &str = "must be \"folder\" or \"path\"";
const CHECK_LABEL: &str = "settings.json";
const SETTING_KEYS: [&str; 10] = [
    "theme",
    "fontFamily",
    "fontSize",
    "scrollback",
    "scrollSensitivity",
    "cursorBlink",
    "cursorStyle",
    "restoreOnLaunch",
    "manageAgentHooks",
    "shellRowName",
];

pub const FONT_SIZE_MIN: f64 = 1.0;
pub const FONT_SIZE_MAX: f64 = 72.0;
const DEFAULT_FONT_FAMILY: &str = "Menlo, Monaco, 'Courier New', monospace";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellRowName {
    Folder,
    Path,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingChange {
    Theme(ThemeChoice),
    FontFamily(String),
    FontSize(f64),
    Scrollback(u32),
    ScrollSensitivity(f64),
    CursorBlink(bool),
    CursorStyle(CursorStyle),
    RestoreOnLaunch(bool),
    ManageAgentHooks(bool),
    ShellRowName(ShellRowName),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub theme: ThemeChoice,
    pub font_family: String,
    pub font_size: f64,
    pub scrollback: u32,
    pub scroll_sensitivity: f64,
    pub cursor_blink: bool,
    pub cursor_style: CursorStyle,
    pub restore_on_launch: bool,
    pub manage_agent_hooks: bool,
    pub shell_row_name: ShellRowName,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsLayerResult {
    pub settings: Settings,
    pub problems: Vec<String>,
    pub rejected: Vec<&'static str>,
}

enum Rejection {
    UnknownKey,
    Invalid(&'static str),
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeChoice::System,
            font_family: DEFAULT_FONT_FAMILY.to_string(),
            font_size: 12.0,
            scrollback: 100_000,
            scroll_sensitivity: 1.0,
            cursor_blink: true,
            cursor_style: CursorStyle::Block,
            restore_on_launch: true,
            manage_agent_hooks: true,
            shell_row_name: ShellRowName::Folder,
        }
    }
}

// A whole number is written as one ("fontSize": 20), the way a person writes it, not as 20.0.
fn number_value(number: f64) -> Value {
    let whole = number as i64;
    let round_trips = whole as f64 == number;
    if round_trips {
        return Value::from(whole);
    }
    Value::from(number)
}

impl SettingChange {
    pub fn key(&self) -> &'static str {
        match self {
            SettingChange::Theme(_) => "theme",
            SettingChange::FontFamily(_) => "fontFamily",
            SettingChange::FontSize(_) => "fontSize",
            SettingChange::Scrollback(_) => "scrollback",
            SettingChange::ScrollSensitivity(_) => "scrollSensitivity",
            SettingChange::CursorBlink(_) => "cursorBlink",
            SettingChange::CursorStyle(_) => "cursorStyle",
            SettingChange::RestoreOnLaunch(_) => "restoreOnLaunch",
            SettingChange::ManageAgentHooks(_) => "manageAgentHooks",
            SettingChange::ShellRowName(_) => "shellRowName",
        }
    }

    pub fn value(&self) -> Value {
        match self {
            SettingChange::Theme(choice) => {
                let text = match choice {
                    ThemeChoice::System => "system",
                    ThemeChoice::Light => "light",
                    ThemeChoice::Dark => "dark",
                };
                Value::from(text)
            }
            SettingChange::FontFamily(family) => Value::from(family.as_str()),
            SettingChange::FontSize(size) => number_value(*size),
            SettingChange::Scrollback(lines) => Value::from(*lines),
            SettingChange::ScrollSensitivity(sensitivity) => number_value(*sensitivity),
            SettingChange::CursorBlink(on) => Value::from(*on),
            SettingChange::CursorStyle(style) => {
                let text = match style {
                    CursorStyle::Block => "block",
                    CursorStyle::Underline => "underline",
                    CursorStyle::Bar => "bar",
                };
                Value::from(text)
            }
            SettingChange::RestoreOnLaunch(on) => Value::from(*on),
            SettingChange::ManageAgentHooks(on) => Value::from(*on),
            SettingChange::ShellRowName(name) => {
                let text = match name {
                    ShellRowName::Folder => "folder",
                    ShellRowName::Path => "path",
                };
                Value::from(text)
            }
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let key = self.key().to_string();
        let value = self.value();
        let mut layer = Map::new();
        layer.insert(key, value);
        let base = Settings::default();
        let result = apply_settings_layer(&base, &layer, CHECK_LABEL);
        let first_problem = result.problems.into_iter().next();
        match first_problem {
            Some(problem) => Err(problem),
            None => Ok(()),
        }
    }
}

fn setting_name(key: &str) -> Option<&'static str> {
    let mut names = SETTING_KEYS.into_iter();
    names.find(|name| *name == key)
}

fn read_theme(value: &Value) -> Option<ThemeChoice> {
    let text = value.as_str()?;
    match text {
        "system" => Some(ThemeChoice::System),
        "light" => Some(ThemeChoice::Light),
        "dark" => Some(ThemeChoice::Dark),
        _ => None,
    }
}

fn read_cursor_style(value: &Value) -> Option<CursorStyle> {
    let text = value.as_str()?;
    match text {
        "block" => Some(CursorStyle::Block),
        "underline" => Some(CursorStyle::Underline),
        "bar" => Some(CursorStyle::Bar),
        _ => None,
    }
}

fn read_shell_row_name(value: &Value) -> Option<ShellRowName> {
    let text = value.as_str()?;
    match text {
        "folder" => Some(ShellRowName::Folder),
        "path" => Some(ShellRowName::Path),
        _ => None,
    }
}

fn read_non_empty_string(value: &Value) -> Option<String> {
    let text = value.as_str()?;
    if text.is_empty() {
        return None;
    }
    Some(text.to_string())
}

fn read_font_size(value: &Value) -> Option<f64> {
    let number = value.as_f64()?;
    if number.is_finite() && (FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&number) {
        return Some(number);
    }
    None
}

fn read_scrollback(value: &Value) -> Option<u32> {
    let number = value.as_f64()?;
    let is_whole = number.is_finite() && number.fract() == 0.0;
    let in_range = (0.0..=f64::from(u32::MAX)).contains(&number);
    if is_whole && in_range {
        return Some(number as u32);
    }
    None
}

fn read_positive_number(value: &Value) -> Option<f64> {
    let number = value.as_f64()?;
    if number.is_finite() && number > 0.0 {
        return Some(number);
    }
    None
}

fn assign<T>(slot: &mut T, parsed: Option<T>, requirement: &'static str) -> Result<(), Rejection> {
    match parsed {
        Some(parsed_value) => {
            *slot = parsed_value;
            Ok(())
        }
        None => Err(Rejection::Invalid(requirement)),
    }
}

fn apply_setting(
    settings: &mut Settings,
    name: &'static str,
    value: &Value,
) -> Result<(), Rejection> {
    match name {
        "theme" => assign(&mut settings.theme, read_theme(value), THEME_REQUIREMENT),
        "fontFamily" => assign(
            &mut settings.font_family,
            read_non_empty_string(value),
            FONT_FAMILY_REQUIREMENT,
        ),
        "fontSize" => assign(
            &mut settings.font_size,
            read_font_size(value),
            FONT_SIZE_REQUIREMENT,
        ),
        "scrollback" => assign(
            &mut settings.scrollback,
            read_scrollback(value),
            SCROLLBACK_REQUIREMENT,
        ),
        "scrollSensitivity" => assign(
            &mut settings.scroll_sensitivity,
            read_positive_number(value),
            SCROLL_SENSITIVITY_REQUIREMENT,
        ),
        "cursorBlink" => assign(
            &mut settings.cursor_blink,
            value.as_bool(),
            BOOLEAN_REQUIREMENT,
        ),
        "cursorStyle" => assign(
            &mut settings.cursor_style,
            read_cursor_style(value),
            CURSOR_STYLE_REQUIREMENT,
        ),
        "restoreOnLaunch" => assign(
            &mut settings.restore_on_launch,
            value.as_bool(),
            BOOLEAN_REQUIREMENT,
        ),
        "manageAgentHooks" => assign(
            &mut settings.manage_agent_hooks,
            value.as_bool(),
            BOOLEAN_REQUIREMENT,
        ),
        "shellRowName" => assign(
            &mut settings.shell_row_name,
            read_shell_row_name(value),
            SHELL_ROW_NAME_REQUIREMENT,
        ),
        _ => Err(Rejection::UnknownKey),
    }
}

pub fn apply_settings_layer(
    base: &Settings,
    layer: &Map<String, Value>,
    label: &str,
) -> SettingsLayerResult {
    let mut settings = base.clone();
    let mut problems = Vec::new();
    let mut rejected = Vec::new();

    for (key, value) in layer {
        let unknown = format!("{label}: \"{key}\" is not a setting");
        let Some(name) = setting_name(key) else {
            problems.push(unknown);
            continue;
        };
        let outcome = apply_setting(&mut settings, name, value);
        match outcome {
            Ok(()) => {}
            Err(Rejection::UnknownKey) => problems.push(unknown),
            Err(Rejection::Invalid(requirement)) => {
                problems.push(format!("{label}: \"{key}\" {requirement}"));
                rejected.push(name);
            }
        }
    }

    SettingsLayerResult {
        settings,
        problems,
        rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layer_from(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }

    #[test]
    fn good_keys_apply_and_the_rest_stay_at_the_base() {
        let base = Settings::default();
        let layer = layer_from(json!({ "fontSize": 72, "cursorStyle": "bar", "scrollback": 0 }));

        let result = apply_settings_layer(&base, &layer, "settings.json");

        assert!(result.problems.is_empty());
        assert_eq!(result.settings.font_size, 72.0);
        assert_eq!(result.settings.cursor_style, CursorStyle::Bar);
        assert_eq!(result.settings.scrollback, 0);
        assert_eq!(result.settings.font_family, base.font_family);
        assert_eq!(result.settings.theme, base.theme);

        let layer = layer_from(json!({
            "fontSize": 0,
            "scrollback": -1,
            "scrollSensitivity": 0,
            "theme": "blue",
            "cursorStyle": "beam",
            "cursorBlink": "yes",
            "restoreOnLaunch": false,
            "colour": 1
        }));

        let result = apply_settings_layer(&base, &layer, "settings.json");

        let reported: Vec<&str> = result
            .problems
            .iter()
            .filter_map(|problem| problem.split('"').nth(1))
            .collect();
        let expected = [
            "fontSize",
            "scrollback",
            "scrollSensitivity",
            "theme",
            "cursorStyle",
            "cursorBlink",
            "colour",
        ];
        assert_eq!(reported, expected);
        assert_eq!(
            result.problems[0],
            "settings.json: \"fontSize\" must be a number between 1 and 72"
        );
        assert!(!result.settings.restore_on_launch);
        assert_eq!(result.settings.font_size, base.font_size);
        assert_eq!(result.settings.scrollback, base.scrollback);

        let path = layer_from(json!({ "shellRowName": "path" }));
        let result = apply_settings_layer(&base, &path, "settings.json");
        assert!(result.problems.is_empty());
        assert_eq!(result.settings.shell_row_name, ShellRowName::Path);

        let tree = layer_from(json!({ "shellRowName": "tree", "fontSize": 0 }));
        let result = apply_settings_layer(&base, &tree, "settings.json");
        assert_eq!(
            result.problems[0],
            "settings.json: \"shellRowName\" must be \"folder\" or \"path\""
        );
        assert_eq!(result.rejected, ["shellRowName", "fontSize"]);

        for (key, bad) in [
            ("fontSize", json!(73)),
            ("scrollback", json!(1.5)),
            ("scrollback", json!(4_294_967_296_u64)),
        ] {
            let single = layer_from(json!({ key: bad }));
            let outcome = apply_settings_layer(&base, &single, "settings.json");
            assert_eq!(outcome.problems.len(), 1, "{key}");
            assert_eq!(outcome.settings, base);
        }
    }

    #[test]
    fn check_refuses_what_the_reader_refuses_and_values_read_back() {
        let too_small = SettingChange::FontSize(0.0).check();
        let too_big = SettingChange::FontSize(73.0).check();
        let line = "settings.json: \"fontSize\" must be a number between 1 and 72";
        assert_eq!(too_small, Err(line.to_string()));
        assert_eq!(too_big, Err(line.to_string()));

        assert_eq!(SettingChange::FontSize(72.0).check(), Ok(()));

        let whole = SettingChange::FontSize(20.0).value();
        assert_eq!(whole.to_string(), "20");
        let fraction = SettingChange::FontSize(12.5).value();
        assert_eq!(fraction.to_string(), "12.5");
        let beyond_whole_numbers = SettingChange::ScrollSensitivity(1e300).value();
        assert_eq!(beyond_whole_numbers.as_f64(), Some(1e300));
        assert!(SettingChange::FontFamily(String::new()).check().is_err());

        let base = Settings::default();
        let cases = [
            SettingChange::Theme(ThemeChoice::Light),
            SettingChange::FontFamily("Iosevka".to_string()),
            SettingChange::FontSize(15.5),
            SettingChange::Scrollback(0),
            SettingChange::ScrollSensitivity(2.5),
            SettingChange::CursorBlink(false),
            SettingChange::CursorStyle(CursorStyle::Bar),
            SettingChange::RestoreOnLaunch(false),
            SettingChange::ManageAgentHooks(false),
            SettingChange::ShellRowName(ShellRowName::Path),
        ];
        for change in cases {
            let layer = layer_from(json!({ change.key(): change.value() }));
            let result = apply_settings_layer(&base, &layer, "settings.json");
            assert!(result.problems.is_empty(), "{}", change.key());

            let expected = match &change {
                SettingChange::Theme(choice) => Settings {
                    theme: *choice,
                    ..base.clone()
                },
                SettingChange::FontFamily(family) => Settings {
                    font_family: family.clone(),
                    ..base.clone()
                },
                SettingChange::FontSize(size) => Settings {
                    font_size: *size,
                    ..base.clone()
                },
                SettingChange::Scrollback(lines) => Settings {
                    scrollback: *lines,
                    ..base.clone()
                },
                SettingChange::ScrollSensitivity(sensitivity) => Settings {
                    scroll_sensitivity: *sensitivity,
                    ..base.clone()
                },
                SettingChange::CursorBlink(on) => Settings {
                    cursor_blink: *on,
                    ..base.clone()
                },
                SettingChange::CursorStyle(style) => Settings {
                    cursor_style: *style,
                    ..base.clone()
                },
                SettingChange::RestoreOnLaunch(on) => Settings {
                    restore_on_launch: *on,
                    ..base.clone()
                },
                SettingChange::ManageAgentHooks(on) => Settings {
                    manage_agent_hooks: *on,
                    ..base.clone()
                },
                SettingChange::ShellRowName(name) => Settings {
                    shell_row_name: *name,
                    ..base.clone()
                },
            };
            assert_eq!(result.settings, expected, "{}", change.key());
            assert_ne!(result.settings, base, "{}", change.key());
        }
    }
}
