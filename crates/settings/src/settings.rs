use serde_json::{Map, Value};

const THEME_REQUIREMENT: &str = "must be \"system\", \"light\" or \"dark\"";
const FONT_FAMILY_REQUIREMENT: &str = "must be a non-empty string";
const FONT_SIZE_REQUIREMENT: &str = "must be a number between 1 and 72";
const SCROLLBACK_REQUIREMENT: &str = "must be a whole number of lines, 0 or more";
const SCROLL_SENSITIVITY_REQUIREMENT: &str = "must be a number greater than 0";
const BOOLEAN_REQUIREMENT: &str = "must be true or false";
const CURSOR_STYLE_REQUIREMENT: &str = "must be \"block\", \"underline\" or \"bar\"";

const FONT_SIZE_MIN: f64 = 1.0;
const FONT_SIZE_MAX: f64 = 72.0;
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsLayerResult {
    pub settings: Settings,
    pub problems: Vec<String>,
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
        }
    }
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

fn apply_setting(settings: &mut Settings, key: &str, value: &Value) -> Result<(), Rejection> {
    match key {
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

    for (key, value) in layer {
        let outcome = apply_setting(&mut settings, key, value);
        match outcome {
            Ok(()) => {}
            Err(Rejection::UnknownKey) => {
                problems.push(format!("{label}: \"{key}\" is not a setting"));
            }
            Err(Rejection::Invalid(requirement)) => {
                problems.push(format!("{label}: \"{key}\" {requirement}"));
            }
        }
    }

    SettingsLayerResult { settings, problems }
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
}
