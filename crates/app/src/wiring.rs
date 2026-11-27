//! The real app: the runtime, the workspace and the services started for the window, and stopped in order at quit.

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use futures::channel::mpsc;
use taco_protocol::{
    ConfigFile, CursorShape, DrawnState, HarnessId, RuntimeCommand, RuntimeReport,
};
use taco_rules::default_panels;
use taco_services::{
    AGENT_APP_NAME, AgentMenuAction, config_directory, config_file_path, focus_port_path,
    plugin_directory_path, read_config_texts,
};
use taco_settings::{
    Config, CursorStyle, KeymapChange, SettingChange, palette_for, resolve_config,
};
use taco_workspace::{FileStorage, Options};

use crate::link::{ServicesPort, WindowReceiver};
use crate::runtime_client::{RuntimeClient, RuntimeEvent, ShellContext};
use crate::services_host::ServicesHost;
use crate::workspace_host::{RuntimeCommands, WorkspaceHost, new_terminal_id};

/// A healthy save takes milliseconds; this only bounds one stuck behind a wedged runtime.
const SAVE_PATIENCE: Duration = Duration::from_millis(1500);

/// Everything the window runs on, started.
#[derive(Clone)]
pub struct Hosts {
    pub runtime: RuntimeClient,
    pub workspace: WorkspaceHost,
    pub services: ServicesHost,
    /// Where a new config goes to reach the runtime, in order, off the window's thread.
    pub terminal_settings: Sender<Config>,
}

pub struct Started {
    pub hosts: Hosts,
    pub outputs: WindowReceiver,
    pub first_config: Config,
    /// The runtime's terminals connection, for the terminal view.
    pub terminals: UnixStream,
}

impl RuntimeCommands for RuntimeClient {
    fn send(&self, command: RuntimeCommand) {
        RuntimeClient::send(self, command);
    }
}

impl ServicesPort for ServicesHost {
    fn agent_action(&self, harness: &HarnessId, action: AgentMenuAction) {
        ServicesHost::agent_action(self, harness, action);
    }

    fn dismiss_notice(&self, harness: &HarnessId) {
        ServicesHost::dismiss_notice(self, harness);
    }

    fn apply_update(&self) {
        ServicesHost::apply_update(self);
    }

    fn hooks_directory(&self) -> PathBuf {
        ServicesHost::hooks_directory(self)
    }

    fn write_setting(&self, change: SettingChange) {
        ServicesHost::write_setting(self, change);
    }

    fn write_keybinding(&self, change: KeymapChange) {
        ServicesHost::write_keybinding(self, change);
    }
}

/// The runtime, the workspace and the services, started in that order; the runtime has the terminal settings before the first shell.
pub fn start(system_is_dark: bool) -> Result<Started, String> {
    let Some(directory) = config_directory() else {
        return Err("Taco Shells has no home directory to keep its config in.".to_string());
    };
    let texts = read_config_texts(&directory);
    let first = resolve_config(&texts, system_is_dark);

    let program = RuntimeClient::program_beside_this_one();
    let program =
        program.map_err(|error| format!("The terminal runtime was not found: {error}"))?;
    let (events, received) = unbounded();
    let on_event = move |event| {
        let _ = events.send(event);
    };
    let context = ShellContext {
        app: AGENT_APP_NAME.to_string(),
        port_file: focus_port_path(&directory),
        plugin_directory: plugin_directory_path(&directory),
    };
    let started = RuntimeClient::start(&program, &context, on_event);
    let started =
        started.map_err(|error| format!("The terminal runtime did not start: {error}"))?;
    let runtime = started.client;
    send_terminal_settings(&runtime, &first);

    let (window, outputs) = mpsc::unbounded();
    let options = workspace_options(&directory, &first);
    let workspace = WorkspaceHost::start(options, runtime.clone(), window.clone());
    let services = ServicesHost::start(
        directory,
        system_is_dark,
        runtime.clone(),
        workspace.clone(),
        window,
    );
    let terminal_settings = terminal_settings_writer(runtime.clone());
    let hosts = Hosts {
        runtime,
        workspace,
        services,
        terminal_settings,
    };
    forward_runtime_events(received, hosts.clone());
    // The files can change while everything starts; the window and the runtime start from the same config.
    let first_config = hosts.services.config();
    if first_config != first {
        let _ = hosts.terminal_settings.send(first_config.clone());
    }
    Ok(Started {
        hosts,
        outputs,
        first_config,
        terminals: started.terminals,
    })
}

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

/// Saves, stops the services, then the runtime. Blocks for seconds at worst: never on gpui's thread.
pub fn shut_down(hosts: &Hosts) {
    let save = start_final_save(&hosts.workspace);
    let saved_first = saved_within(&save, SAVE_PATIENCE);
    if !saved_first {
        eprintln!("the final save had not finished; stopping the runtime anyway");
    }
    hosts.services.stop();
    hosts.runtime.stop();
    // Stopping the runtime frees a save stuck behind it; the app must not quit before that save lands.
    if !saved_first && !saved_within(&save, SAVE_PATIENCE) {
        eprintln!("the final save did not finish");
    }
}

// A save stuck behind a send to a wedged runtime would keep the runtime from ever being stopped, so it runs on a thread of its own.
fn start_final_save(workspace: &WorkspaceHost) -> Option<Receiver<()>> {
    let (done, finished) = bounded(1);
    let saving = workspace.clone();
    let save = move || {
        saving.freeze_and_flush();
        let _ = done.send(());
    };
    let spawned = thread::Builder::new().name("final save".into()).spawn(save);
    if spawned.is_err() {
        return None;
    }
    Some(finished)
}

fn saved_within(save: &Option<Receiver<()>>, patience: Duration) -> bool {
    let Some(finished) = save else {
        return false;
    };
    let waited = finished.recv_timeout(patience);
    waited.is_ok()
}

/// The state drawn before the workspace publishes its first.
pub fn nothing_drawn() -> DrawnState {
    DrawnState {
        revision: 0,
        arrangement: None,
        agent_rows: BTreeMap::new(),
        directories: BTreeMap::new(),
        refocus: 0,
        focus_count: 0,
        agents: BTreeMap::new(),
        default_agent_name: String::new(),
        panels: default_panels(),
        problem: None,
        started: BTreeSet::new(),
    }
}

fn workspace_options(directory: &Path, config: &Config) -> Options {
    let settings_path = config_file_path(directory, ConfigFile::Settings);
    let keybindings_path = config_file_path(directory, ConfigFile::Keybindings);
    let storage = FileStorage::new(directory.to_path_buf());
    Options {
        settings_path: settings_path.to_string_lossy().into_owned(),
        keybindings_path: keybindings_path.to_string_lossy().into_owned(),
        new_terminal_id: Box::new(new_terminal_id),
        storage: Box::new(storage),
        restore_on_launch: config.settings.restore_on_launch,
    }
}

// A write to the runtime blocks while it is not reading, which the window's thread must never wait on.
fn terminal_settings_writer(runtime: RuntimeClient) -> Sender<Config> {
    let (settings, received) = unbounded::<Config>();
    let write = move || {
        for config in received {
            send_terminal_settings(&runtime, &config);
        }
    };
    let spawned = thread::Builder::new()
        .name("terminal settings".into())
        .spawn(write);
    if let Err(error) = spawned {
        eprintln!("new terminal settings have no thread to reach the runtime through: {error}");
    }
    settings
}

/// Forwards on one thread so each report reaches the workspace, and the services, in arrival order.
fn forward_runtime_events(events: Receiver<RuntimeEvent>, hosts: Hosts) {
    let forward = move || {
        for event in events {
            match event {
                RuntimeEvent::Report(report) => {
                    // The workspace first: the services' refresh reads files, and the window waits on the workspace.
                    let for_services = report.clone();
                    hosts.workspace.runtime_report(report);
                    tell_services(&hosts.services, &for_services);
                }
                RuntimeEvent::Died { reason } => hosts.workspace.runtime_died(reason),
            }
        }
    };
    let spawned = thread::Builder::new()
        .name("runtime events".into())
        .spawn(forward);
    if let Err(error) = spawned {
        eprintln!("the runtime's reports have no thread to go through: {error}");
    }
}

fn tell_services(services: &ServicesHost, report: &RuntimeReport) {
    match report {
        RuntimeReport::Submitted { id, .. } => services.submitted(id.clone()),
        RuntimeReport::Exited { id, .. } => services.exited(id),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use taco_protocol::{CursorShape, RuntimeCommand};
    use taco_settings::{Appearance, ConfigTexts, CursorStyle, palette_for, resolve_config};

    use super::terminal_settings_of;

    #[test]
    fn the_runtime_starts_terminals_with_the_configs_cursor_theme_and_agent_plugin_switch() {
        let no_files = ConfigTexts {
            settings: None,
            keybindings: None,
        };
        let mut config = resolve_config(&no_files, true);
        config.settings.cursor_style = CursorStyle::Bar;
        config.settings.cursor_blink = false;
        config.settings.scrollback = 4321;
        config.settings.agent_activity = true;

        let commands = terminal_settings_of(&config);

        let dark = palette_for(Appearance::Dark);
        let expected = [
            RuntimeCommand::Scrollback { lines: 4321 },
            RuntimeCommand::Colors(dark.terminal),
            RuntimeCommand::CursorDefaults {
                shape: CursorShape::Bar,
                blink: false,
            },
            RuntimeCommand::AgentPlugin { on: true },
        ];
        assert_eq!(config.appearance, Appearance::Dark);
        assert_eq!(commands, expected);
    }
}
