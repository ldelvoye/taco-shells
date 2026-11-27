//! What every new terminal starts with, from the config, and its sending to the runtime.

use taco_protocol::{CursorShape, RuntimeCommand};
use taco_settings::{Config, CursorStyle, palette_for};

use crate::runtime_client::RuntimeClient;

/// Queues the four settings for the runtime, which apply to terminals opened from then on.
pub fn send_terminal_settings(runtime: &RuntimeClient, config: &Config) {
    let commands = terminal_settings_of(config);
    for command in commands {
        runtime.send(command);
    }
}

/// The scrollback, the colors, the cursor and whether Agent Activity is on, which every new terminal starts with, from the config.
fn terminal_settings_of(config: &Config) -> [RuntimeCommand; 4] {
    let settings = &config.settings;
    let palette = palette_for(config.appearance);
    let shape = match settings.cursor_style {
        CursorStyle::Block => CursorShape::Block,
        CursorStyle::Bar => CursorShape::Bar,
        CursorStyle::Underline => CursorShape::Underline,
    };
    let scrollback = RuntimeCommand::Scrollback {
        lines: settings.scrollback,
    };
    let colors = RuntimeCommand::Colors(palette.terminal);
    let cursor = RuntimeCommand::CursorDefaults {
        shape,
        blink: settings.cursor_blink,
    };
    let agent_plugin = RuntimeCommand::AgentPlugin {
        on: settings.agent_activity,
    };
    [scrollback, colors, cursor, agent_plugin]
}

#[cfg(test)]
mod tests {
    use taco_protocol::{CursorShape, RuntimeCommand};
    use taco_settings::{Appearance, ConfigTexts, CursorStyle, palette_for, resolve_config};

    use super::terminal_settings_of;

    #[test]
    fn each_cursor_style_is_its_shape_and_each_setting_comes_from_its_own_field_in_order() {
        let no_files = ConfigTexts {
            settings: None,
            keybindings: None,
        };
        let styles = [
            (CursorStyle::Block, CursorShape::Block),
            (CursorStyle::Bar, CursorShape::Bar),
            (CursorStyle::Underline, CursorShape::Underline),
        ];
        for (style, shape) in styles {
            let mut config = resolve_config(&no_files, true);
            config.settings.cursor_style = style;
            // Unlike the defaults, and the two switches unlike each other, so a setting sent from the wrong field shows.
            config.settings.scrollback = 4321;
            config.settings.cursor_blink = false;
            config.settings.agent_activity = true;
            let commands = terminal_settings_of(&config);
            let expected = [
                RuntimeCommand::Scrollback { lines: 4321 },
                RuntimeCommand::Colors(palette_for(Appearance::Dark).terminal),
                RuntimeCommand::CursorDefaults {
                    shape,
                    blink: false,
                },
                RuntimeCommand::AgentPlugin { on: true },
            ];
            assert_eq!(commands, expected, "{style:?}");
        }
    }
}
