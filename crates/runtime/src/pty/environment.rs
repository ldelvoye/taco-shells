use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::process::Command;

use taco_protocol::{APP_VARIABLE, PORT_FILE_VARIABLE};

use super::ShellContext;

/// The plugin folders Claude Code loads for one session, joined with `:`.
const PLUGIN_DIRECTORIES_VARIABLE: &str = "CLAUDE_CODE_PLUGIN_DIRS";

/// The variables every shell starts with; the app's own replace any it inherited.
pub(super) fn apply(command: &mut Command, context: &ShellContext, agent_plugin: bool) {
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    // Most CLIs only emit OSC 8 hyperlinks for terminals they recognise by name.
    command.env("FORCE_HYPERLINK", "1");
    if std::env::var_os("LANG").is_none() {
        command.env("LANG", "en_US.UTF-8");
    }
    // Overrides what the runtime inherited: a Dev app opened from a stable app's terminal would otherwise pass on the stable app's.
    command.env(APP_VARIABLE, &context.app);
    command.env(PORT_FILE_VARIABLE, &context.port_file);
    let inherited_plugins = std::env::var_os(PLUGIN_DIRECTORIES_VARIABLE);
    let own_plugin = &context.plugin_directory;
    let plugins = plugin_directories(inherited_plugins.as_deref(), own_plugin, agent_plugin);
    match plugins {
        Some(plugins) => command.env(PLUGIN_DIRECTORIES_VARIABLE, plugins),
        None => command.env_remove(PLUGIN_DIRECTORIES_VARIABLE),
    };
}

/// The plugin folders a new shell gets: the inherited ones with the app's own added when on and taken out when off; none if none is left.
fn plugin_directories(inherited: Option<&OsStr>, own: &Path, on: bool) -> Option<OsString> {
    let own = own.as_os_str().as_bytes();
    let mut kept: Vec<&[u8]> = Vec::new();
    if let Some(inherited) = inherited {
        let entries = inherited.as_bytes().split(|byte| *byte == b':');
        for entry in entries {
            if !entry.is_empty() && entry != own {
                kept.push(entry);
            }
        }
    }
    if on {
        kept.push(own);
    }
    if kept.is_empty() {
        return None;
    }
    let joined = kept.join(&b':');
    Some(OsString::from_vec(joined))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_plugin_is_added_to_the_inherited_ones_when_on_and_only_it_is_taken_out_when_off() {
        let own = Path::new("/ours/claude-plugin");
        let cases: [(Option<&str>, bool, Option<&str>); 9] = [
            (None, false, None),
            (Some("/theirs"), false, Some("/theirs")),
            (None, true, Some("/ours/claude-plugin")),
            (Some(""), true, Some("/ours/claude-plugin")),
            (Some("/theirs"), true, Some("/theirs:/ours/claude-plugin")),
            (
                Some("/ours/claude-plugin:/theirs"),
                true,
                Some("/theirs:/ours/claude-plugin"),
            ),
            (Some("/ours/claude-plugin"), false, None),
            (Some("/a:/ours/claude-plugin:/b"), false, Some("/a:/b")),
            // Only the exact folder is ours.
            (
                Some("/ours/claude-plugin-old"),
                false,
                Some("/ours/claude-plugin-old"),
            ),
        ];
        for (inherited, on, expected) in cases {
            let inherited = inherited.map(OsStr::new);
            let given = plugin_directories(inherited, own, on);
            let expected = expected.map(OsString::from);
            assert_eq!(given, expected, "inherited {inherited:?}, on {on}");
        }
    }
}
