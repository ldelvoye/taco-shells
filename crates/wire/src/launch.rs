//! How the app starts the runtime: the two connections it hands over, and what every shell is told about the app, as arguments.

use std::ffi::{OsStr, OsString};
use std::os::fd::RawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub const USAGE: &str = "usage: taco-shells-runtime --control-fd=<fd> --terminals-fd=<fd> --app=<name> --port-file=<path> --plugin-dir=<path>";

const CONTROL_FD: &str = "--control-fd";
const TERMINALS_FD: &str = "--terminals-fd";
const APP: &str = "--app";
const PORT_FILE: &str = "--port-file";
const PLUGIN_DIR: &str = "--plugin-dir";

/// What every shell is told about the app it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellContext {
    /// `taco-shells` or `taco-shells-dev`.
    pub app: String,
    /// The app's `focus-port` file.
    pub port_file: PathBuf,
    /// The app's agent plugin, which new shells load while Agent Activity is on.
    pub plugin_directory: PathBuf,
}

/// Everything the runtime is started with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    /// The app's control connection.
    pub control: RawFd,
    /// The window's terminal channel.
    pub terminals: RawFd,
    pub context: ShellContext,
}

/// The runtime's arguments for `launch`. Paths go as bytes, so any path works.
pub fn arguments(launch: &Launch) -> Vec<OsString> {
    let control = format!("{CONTROL_FD}={}", launch.control);
    let terminals = format!("{TERMINALS_FD}={}", launch.terminals);
    let app = format!("{APP}={}", launch.context.app);
    let port_file = path_argument(PORT_FILE, &launch.context.port_file);
    let plugin_directory = path_argument(PLUGIN_DIR, &launch.context.plugin_directory);
    vec![
        control.into(),
        terminals.into(),
        app.into(),
        port_file,
        plugin_directory,
    ]
}

/// All five arguments, or none if one is missing, empty or not known.
pub fn parsed(arguments: &[OsString]) -> Option<Launch> {
    let mut control = None;
    let mut terminals = None;
    let mut app = None;
    let mut port_file = None;
    let mut plugin_directory = None;
    for argument in arguments {
        let bytes = argument.as_bytes();
        let equals = bytes.iter().position(|byte| *byte == b'=')?;
        let name = std::str::from_utf8(&bytes[..equals]).ok()?;
        let value = OsStr::from_bytes(&bytes[equals + 1..]);
        if value.is_empty() {
            return None;
        }
        match name {
            CONTROL_FD => control = Some(fd_number(value)?),
            TERMINALS_FD => terminals = Some(fd_number(value)?),
            APP => {
                let text = value.to_str()?;
                app = Some(text.to_string());
            }
            PORT_FILE => port_file = Some(PathBuf::from(value)),
            PLUGIN_DIR => plugin_directory = Some(PathBuf::from(value)),
            _ => return None,
        }
    }
    let context = ShellContext {
        app: app?,
        port_file: port_file?,
        plugin_directory: plugin_directory?,
    };
    Some(Launch {
        control: control?,
        terminals: terminals?,
        context,
    })
}

fn path_argument(name: &str, path: &Path) -> OsString {
    let mut argument = OsString::from(format!("{name}="));
    argument.push(path);
    argument
}

fn fd_number(value: &OsStr) -> Option<RawFd> {
    let text = value.to_str()?;
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_runtime_reads_back_what_the_app_writes_whatever_bytes_its_paths_hold() {
        // Not UTF-8, and with an `=` of its own, as a home folder may be.
        let odd_folder = OsStr::from_bytes(b"/Users/caf\xe9/a=b");
        let launch = Launch {
            control: 3,
            terminals: 4,
            context: ShellContext {
                app: "taco-shells-dev".into(),
                port_file: PathBuf::from(odd_folder).join("focus-port"),
                plugin_directory: PathBuf::from(odd_folder).join("claude-plugin"),
            },
        };
        let written = arguments(&launch);
        assert_eq!(parsed(&written), Some(launch));
    }
}
