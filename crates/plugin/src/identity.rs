use taco_identity::CURRENT;
use taco_protocol::APP_VARIABLE;

/// The plugin is named after the app it serves, which is also what its shells' `TACO_SHELLS_APP` holds.
pub const PLUGIN_NAME: &str = CURRENT.agent_app_name;
pub const SERVER_NAME: &str = "activity";
pub const ENTRYPOINT_VARIABLE: &str = "CLAUDE_CODE_ENTRYPOINT";

pub struct Surroundings {
    pub app: Option<String>,
    pub entrypoint: Option<String>,
}

impl Surroundings {
    pub fn from_environment() -> Surroundings {
        let app = std::env::var(APP_VARIABLE).ok();
        let entrypoint = std::env::var(ENTRYPOINT_VARIABLE).ok();
        Surroundings { app, entrypoint }
    }
}

pub fn tool_name_prefix() -> String {
    format!("mcp__plugin_{PLUGIN_NAME}_{SERVER_NAME}__")
}

pub fn serves(surroundings: &Surroundings) -> bool {
    let is_own_app = surroundings.app.as_deref() == Some(PLUGIN_NAME);
    if !is_own_app {
        return false;
    }

    // A headless run started inside a terminal (claude -p) inherits the variables but has no panel.
    !is_headless(surroundings)
}

pub fn is_headless(surroundings: &Surroundings) -> bool {
    match &surroundings.entrypoint {
        Some(entrypoint) => entrypoint.starts_with("sdk"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surroundings(app: Option<&str>, entrypoint: Option<&str>) -> Surroundings {
        Surroundings {
            app: app.map(String::from),
            entrypoint: entrypoint.map(String::from),
        }
    }

    #[test]
    fn serves_only_interactive_runs_inside_its_own_app() {
        let other_channel = match PLUGIN_NAME {
            "taco-shells" => "taco-shells-dev",
            _ => "taco-shells",
        };

        assert!(serves(&surroundings(Some(PLUGIN_NAME), None)));
        assert!(serves(&surroundings(Some(PLUGIN_NAME), Some("cli"))));
        assert!(!serves(&surroundings(Some(PLUGIN_NAME), Some("sdk-cli"))));
        assert!(!serves(&surroundings(Some(other_channel), Some("cli"))));
        assert!(!serves(&surroundings(None, Some("cli"))));
    }
}
