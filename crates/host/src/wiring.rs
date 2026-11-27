//! The real app: the runtime, the workspace and the services started for the window, and stopped in order at quit.

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, bounded, unbounded};
use futures::channel::mpsc;
use taco_agents::HARNESSES;
use taco_identity::{CURRENT, config_directory};
use taco_protocol::{ConfigFile, HarnessId, RuntimeCommand, RuntimeReport};
use taco_services::{
    AgentMenuAction, config_file_path, focus_port_path, plugin_directory_path, read_config_files,
};
use taco_settings::{Config, KeymapChange, SettingChange, resolve_config};
use taco_wire::launch::{ListEntry, ShellContext};
use taco_workspace::{FileStorage, Options};

use crate::ports::{ServicesPort, WindowReceiver, WindowSender};
use crate::runtime_client::{RuntimeClient, RuntimeEvent};
use crate::services_host::ServicesHost;
use crate::supervised::spawn_supervised;
use crate::terminal_settings::send_terminal_settings;
pub use crate::workspace_host::nothing_drawn;
use crate::workspace_host::{RuntimeCommands, WorkspaceHost, new_terminal_id};

/// A healthy save takes milliseconds; this only bounds one stuck behind a wedged runtime.
const SAVE_PATIENCE: Duration = Duration::from_millis(1500);
const EVENTS_STOPPED: &str = "Taco Shells hit an internal error and stopped hearing from its terminals: titles, folders and exits no longer update. Quit and reopen Taco Shells.";

/// Everything the window runs on, started.
#[derive(Clone)]
pub struct Hosts {
    pub runtime: RuntimeClient,
    pub workspace: WorkspaceHost,
    pub services: ServicesHost,
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
    let read = read_config_files(&directory);
    let first = resolve_config(&read.texts, system_is_dark);

    let program = RuntimeClient::program_beside_this_one();
    let program =
        program.map_err(|error| format!("The terminal runtime was not found: {error}"))?;
    let (events, received) = unbounded();
    let on_event = move |event| {
        let _ = events.send(event);
    };
    let context = ShellContext {
        app: CURRENT.agent_app_name.to_string(),
        port_file: focus_port_path(&directory),
        list_entries: plugin_entries(&directory),
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
        window.clone(),
    );
    let hosts = Hosts {
        runtime,
        workspace,
        services,
    };
    forward_runtime_events(received, hosts.clone(), window);
    // The files can change while everything starts; the window and the runtime start from the same config.
    let first_config = hosts.services.config();
    if first_config != first {
        send_terminal_settings(&hosts.runtime, &first_config);
    }
    Ok(Started {
        hosts,
        outputs,
        first_config,
        terminals: started.terminals,
    })
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

fn plugin_entries(directory: &Path) -> Vec<ListEntry> {
    let mut entries = Vec::new();
    for harness in HARNESSES {
        let Some(variable) = harness.plugin_variable() else {
            continue;
        };
        let mut given = entries.iter();
        let already = given.any(|entry: &ListEntry| entry.variable == variable);
        if already {
            continue;
        }
        entries.push(ListEntry {
            variable: variable.to_string(),
            entry: plugin_directory_path(directory),
        });
    }
    entries
}

/// Forwards on one thread so each report reaches the workspace, and the services, in arrival order.
fn forward_runtime_events(events: Receiver<RuntimeEvent>, hosts: Hosts, window: WindowSender) {
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
    let spawned = spawn_supervised("runtime events", EVENTS_STOPPED, window, forward);
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
