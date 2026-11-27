use serde_json::{Map, Value};

use crate::config::SETTINGS_FILE_NAME;

const FONT_FAMILY_REQUIREMENT: &str = "must be a non-empty string";
const FONT_SIZE_REQUIREMENT: &str = "must be a number between 1 and 72";
const SCROLLBACK_REQUIREMENT: &str = "must be a whole number of lines, 0 or more";
const SCROLL_SENSITIVITY_REQUIREMENT: &str = "must be a number greater than 0";
const BOOLEAN_REQUIREMENT: &str = "must be true or false";
// Keys an older version wrote: read without a problem line and ignored.
const RETIRED_SETTING_KEYS: [&str; 1] = ["manageAgentHooks"];

pub const FONT_SIZE_MIN: f64 = 1.0;
pub const FONT_SIZE_MAX: f64 = 72.0;
const DEFAULT_FONT_FAMILY: &str = "Menlo, Monaco, 'Courier New', monospace";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeChoice {
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn as_str(self) -> &'static str {
        match self {
            ThemeChoice::System => "system",
            ThemeChoice::Light => "light",
            ThemeChoice::Dark => "dark",
        }
    }

    pub fn from_text(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|choice| choice.as_str() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

impl CursorStyle {
    pub const ALL: [Self; 3] = [Self::Block, Self::Underline, Self::Bar];

    pub fn as_str(self) -> &'static str {
        match self {
            CursorStyle::Block => "block",
            CursorStyle::Underline => "underline",
            CursorStyle::Bar => "bar",
        }
    }

    pub fn from_text(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|style| style.as_str() == text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellRowName {
    Folder,
    Path,
}

impl ShellRowName {
    pub const ALL: [Self; 2] = [Self::Folder, Self::Path];

    pub fn as_str(self) -> &'static str {
        match self {
            ShellRowName::Folder => "folder",
            ShellRowName::Path => "path",
        }
    }

    pub fn from_text(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|name| name.as_str() == text)
    }
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
    AgentActivity(bool),
    AskForActivityNotes(bool),
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
    pub agent_activity: bool,
    pub ask_for_activity_notes: bool,
    pub shell_row_name: ShellRowName,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsLayerResult {
    pub settings: Settings,
    pub problems: Vec<String>,
    pub rejected: Vec<&'static str>,
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
            agent_activity: false,
            ask_for_activity_notes: true,
            shell_row_name: ShellRowName::Folder,
        }
    }
}

impl Settings {
    pub fn apply(&mut self, change: SettingChange) {
        match change {
            SettingChange::Theme(choice) => self.theme = choice,
            SettingChange::FontFamily(family) => self.font_family = family,
            SettingChange::FontSize(size) => self.font_size = size,
            SettingChange::Scrollback(lines) => self.scrollback = lines,
            SettingChange::ScrollSensitivity(sensitivity) => self.scroll_sensitivity = sensitivity,
            SettingChange::CursorBlink(on) => self.cursor_blink = on,
            SettingChange::CursorStyle(style) => self.cursor_style = style,
            SettingChange::RestoreOnLaunch(on) => self.restore_on_launch = on,
            SettingChange::AgentActivity(on) => self.agent_activity = on,
            SettingChange::AskForActivityNotes(on) => self.ask_for_activity_notes = on,
            SettingChange::ShellRowName(name) => self.shell_row_name = name,
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

fn choices_requirement(names: &[&str]) -> String {
    let mut quoted: Vec<String> = Vec::new();
    for name in names {
        quoted.push(format!("\"{name}\""));
    }
    let Some(last) = quoted.pop() else {
        return String::new();
    };
    if quoted.is_empty() {
        return format!("must be {last}");
    }
    let leading = quoted.join(", ");
    format!("must be {leading} or {last}")
}

fn theme_requirement() -> String {
    let names = ThemeChoice::ALL.map(ThemeChoice::as_str);
    choices_requirement(&names)
}

fn cursor_style_requirement() -> String {
    let names = CursorStyle::ALL.map(CursorStyle::as_str);
    choices_requirement(&names)
}

fn shell_row_name_requirement() -> String {
    let names = ShellRowName::ALL.map(ShellRowName::as_str);
    choices_requirement(&names)
}

impl SettingChange {
    pub fn all(settings: &Settings) -> Vec<SettingChange> {
        let Settings {
            theme,
            font_family,
            font_size,
            scrollback,
            scroll_sensitivity,
            cursor_blink,
            cursor_style,
            restore_on_launch,
            agent_activity,
            ask_for_activity_notes,
            shell_row_name,
        } = settings;
        vec![
            SettingChange::Theme(*theme),
            SettingChange::FontFamily(font_family.clone()),
            SettingChange::FontSize(*font_size),
            SettingChange::Scrollback(*scrollback),
            SettingChange::ScrollSensitivity(*scroll_sensitivity),
            SettingChange::CursorBlink(*cursor_blink),
            SettingChange::CursorStyle(*cursor_style),
            SettingChange::RestoreOnLaunch(*restore_on_launch),
            SettingChange::AgentActivity(*agent_activity),
            SettingChange::AskForActivityNotes(*ask_for_activity_notes),
            SettingChange::ShellRowName(*shell_row_name),
        ]
    }

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
            SettingChange::AgentActivity(_) => "agentActivity",
            SettingChange::AskForActivityNotes(_) => "askForActivityNotes",
            SettingChange::ShellRowName(_) => "shellRowName",
        }
    }

    pub fn value(&self) -> Value {
        match self {
            SettingChange::Theme(choice) => Value::from(choice.as_str()),
            SettingChange::FontFamily(family) => Value::from(family.as_str()),
            SettingChange::FontSize(size) => number_value(*size),
            SettingChange::Scrollback(lines) => Value::from(*lines),
            SettingChange::ScrollSensitivity(sensitivity) => number_value(*sensitivity),
            SettingChange::CursorBlink(on) => Value::from(*on),
            SettingChange::CursorStyle(style) => Value::from(style.as_str()),
            SettingChange::RestoreOnLaunch(on) => Value::from(*on),
            SettingChange::AgentActivity(on) => Value::from(*on),
            SettingChange::AskForActivityNotes(on) => Value::from(*on),
            SettingChange::ShellRowName(name) => Value::from(name.as_str()),
        }
    }

    pub fn check(&self) -> Result<(), String> {
        let key = self.key().to_string();
        let value = self.value();
        let mut layer = Map::new();
        layer.insert(key, value);
        let base = Settings::default();
        let result = apply_settings_layer(&base, &layer, SETTINGS_FILE_NAME);
        let first_problem = result.problems.into_iter().next();
        match first_problem {
            Some(problem) => Err(problem),
            None => Ok(()),
        }
    }
}

fn read_choice<T>(value: &Value, from_text: fn(&str) -> Option<T>) -> Option<T> {
    let text = value.as_str()?;
    from_text(text)
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

fn require<T>(parsed: Option<T>, requirement: &str) -> Result<T, String> {
    match parsed {
        Some(parsed_value) => Ok(parsed_value),
        None => Err(requirement.to_string()),
    }
}

fn read_setting(template: &SettingChange, value: &Value) -> Result<SettingChange, String> {
    match template {
        SettingChange::Theme(_) => {
            let parsed = read_choice(value, ThemeChoice::from_text);
            match parsed {
                Some(choice) => Ok(SettingChange::Theme(choice)),
                None => Err(theme_requirement()),
            }
        }
        SettingChange::FontFamily(_) => {
            let parsed = read_non_empty_string(value);
            let family = require(parsed, FONT_FAMILY_REQUIREMENT)?;
            Ok(SettingChange::FontFamily(family))
        }
        SettingChange::FontSize(_) => {
            let parsed = read_font_size(value);
            let size = require(parsed, FONT_SIZE_REQUIREMENT)?;
            Ok(SettingChange::FontSize(size))
        }
        SettingChange::Scrollback(_) => {
            let parsed = read_scrollback(value);
            let lines = require(parsed, SCROLLBACK_REQUIREMENT)?;
            Ok(SettingChange::Scrollback(lines))
        }
        SettingChange::ScrollSensitivity(_) => {
            let parsed = read_positive_number(value);
            let sensitivity = require(parsed, SCROLL_SENSITIVITY_REQUIREMENT)?;
            Ok(SettingChange::ScrollSensitivity(sensitivity))
        }
        SettingChange::CursorBlink(_) => {
            let parsed = value.as_bool();
            let on = require(parsed, BOOLEAN_REQUIREMENT)?;
            Ok(SettingChange::CursorBlink(on))
        }
        SettingChange::CursorStyle(_) => {
            let parsed = read_choice(value, CursorStyle::from_text);
            match parsed {
                Some(style) => Ok(SettingChange::CursorStyle(style)),
                None => Err(cursor_style_requirement()),
            }
        }
        SettingChange::RestoreOnLaunch(_) => {
            let parsed = value.as_bool();
            let on = require(parsed, BOOLEAN_REQUIREMENT)?;
            Ok(SettingChange::RestoreOnLaunch(on))
        }
        SettingChange::AgentActivity(_) => {
            let parsed = value.as_bool();
            let on = require(parsed, BOOLEAN_REQUIREMENT)?;
            Ok(SettingChange::AgentActivity(on))
        }
        SettingChange::AskForActivityNotes(_) => {
            let parsed = value.as_bool();
            let on = require(parsed, BOOLEAN_REQUIREMENT)?;
            Ok(SettingChange::AskForActivityNotes(on))
        }
        SettingChange::ShellRowName(_) => {
            let parsed = read_choice(value, ShellRowName::from_text);
            match parsed {
                Some(name) => Ok(SettingChange::ShellRowName(name)),
                None => Err(shell_row_name_requirement()),
            }
        }
    }
}

fn template_for<'a>(templates: &'a [SettingChange], key: &str) -> Option<&'a SettingChange> {
    templates.iter().find(|template| template.key() == key)
}

pub fn apply_settings_layer(
    base: &Settings,
    layer: &Map<String, Value>,
    label: &str,
) -> SettingsLayerResult {
    let mut settings = base.clone();
    let mut problems = Vec::new();
    let mut rejected = Vec::new();
    let defaults = Settings::default();
    let templates = SettingChange::all(&defaults);

    for (key, value) in layer {
        let is_retired = RETIRED_SETTING_KEYS.contains(&key.as_str());
        if is_retired {
            continue;
        }
        let Some(template) = template_for(&templates, key) else {
            problems.push(format!("{label}: \"{key}\" is not a setting"));
            continue;
        };
        let outcome = read_setting(template, value);
        match outcome {
            Ok(change) => settings.apply(change),
            Err(requirement) => {
                problems.push(format!("{label}: \"{key}\" {requirement}"));
                rejected.push(template.key());
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

        let retired = layer_from(json!({ "manageAgentHooks": false }));
        let result = apply_settings_layer(&base, &retired, "settings.json");
        assert!(result.problems.is_empty(), "{:?}", result.problems);
        assert!(result.rejected.is_empty());
        assert_eq!(result.settings, base);

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
            SettingChange::AgentActivity(true),
            SettingChange::AskForActivityNotes(false),
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
                SettingChange::AgentActivity(on) => Settings {
                    agent_activity: *on,
                    ..base.clone()
                },
                SettingChange::AskForActivityNotes(on) => Settings {
                    ask_for_activity_notes: *on,
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

    #[test]
    fn every_setting_reads_back_what_it_writes() {
        let defaults = Settings::default();
        let non_default = Settings {
            theme: ThemeChoice::Dark,
            font_family: "Iosevka".to_string(),
            font_size: 20.0,
            scrollback: 5000,
            scroll_sensitivity: 2.5,
            cursor_blink: false,
            cursor_style: CursorStyle::Bar,
            restore_on_launch: false,
            agent_activity: true,
            ask_for_activity_notes: false,
            shell_row_name: ShellRowName::Path,
        };
        let default_changes = SettingChange::all(&defaults);

        let changes = SettingChange::all(&non_default);
        for change in &changes {
            assert!(!default_changes.contains(change), "{}", change.key());

            let layer = layer_from(json!({ change.key(): change.value() }));
            let result = apply_settings_layer(&defaults, &layer, "settings.json");
            assert!(result.problems.is_empty(), "{}", change.key());

            let read_back = SettingChange::all(&result.settings);
            assert!(read_back.contains(change), "{}", change.key());
        }

        let mut keys: Vec<&str> = changes.iter().map(SettingChange::key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), changes.len());
    }
}
