//! The settings typed into a field: what the text means, and what is sent when it changes.

use taco_settings::{SettingChange, Settings};

/// The settings typed rather than chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedSetting {
    Font,
    Scrollback,
}

#[derive(Debug, PartialEq)]
pub enum Typed {
    Change(SettingChange),
    Unchanged,
    /// Scrollback that is not a whole number, which is never sent.
    NotWhole,
}

impl TypedSetting {
    pub fn key(self) -> &'static str {
        let named = current_setting(self, &Settings::default());
        named.key()
    }
}

/// What a typed value comes to once Enter is pressed or the field is left.
pub fn typed(setting: TypedSetting, text: &str, settings: &Settings) -> Typed {
    let trimmed = text.trim();
    let change = match setting {
        TypedSetting::Font => SettingChange::FontFamily(trimmed.to_string()),
        TypedSetting::Scrollback => {
            let parsed = trimmed.parse::<u32>();
            let Ok(lines) = parsed else {
                return Typed::NotWhole;
            };
            SettingChange::Scrollback(lines)
        }
    };
    let current = current_setting(setting, settings);
    if change == current {
        return Typed::Unchanged;
    }
    Typed::Change(change)
}

fn current_setting(setting: TypedSetting, settings: &Settings) -> SettingChange {
    match setting {
        TypedSetting::Font => SettingChange::FontFamily(settings.font_family.clone()),
        TypedSetting::Scrollback => SettingChange::Scrollback(settings.scrollback),
    }
}

/// The text a field shows for the setting as the file has it.
pub fn published_text(setting: TypedSetting, settings: &Settings) -> String {
    match setting {
        TypedSetting::Font => settings.font_family.clone(),
        TypedSetting::Scrollback => settings.scrollback.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_is_sent_only_as_a_whole_number_and_nothing_is_sent_when_nothing_changed() {
        let settings = Settings::default();
        let padded_zeros = format!("00{}", settings.scrollback);
        let padded_font = format!("  {}  ", settings.font_family);
        let not_whole = typed(TypedSetting::Scrollback, "12.5", &settings);
        assert_eq!(not_whole, Typed::NotWhole);
        let more = typed(TypedSetting::Scrollback, " 5000 ", &settings);
        assert_eq!(more, Typed::Change(SettingChange::Scrollback(5000)));
        let same_lines = typed(TypedSetting::Scrollback, &padded_zeros, &settings);
        assert_eq!(same_lines, Typed::Unchanged);
        let same_font = typed(TypedSetting::Font, &padded_font, &settings);
        assert_eq!(same_font, Typed::Unchanged);
    }
}
