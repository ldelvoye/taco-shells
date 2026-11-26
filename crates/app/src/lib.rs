//! The window, and the wiring that starts everything.

mod activity;
mod assets;
mod banners;
pub mod drive;
#[cfg(feature = "render")]
mod fixture;
mod keys;
pub mod link;
mod look;
mod menu;
mod panels;
mod platform;
mod popups;
#[cfg(feature = "render")]
pub mod render;
pub mod runtime_client;
pub mod services_host;
#[cfg(feature = "render")]
mod stand_in;
mod strip;
mod terminal_list;
mod text_field;
mod title_bar;
mod window;
mod wiring;
pub mod workspace_host;

use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt;

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Global, TitlebarOptions, WindowAppearance,
    WindowBounds, WindowOptions, px, size,
};
use taco_services::{APPLICATION_NAME, AgentSetupView};
use taco_settings::{Appearance, Config};

#[cfg(feature = "render")]
pub use fixture::Fixture;

use crate::assets::{Assets, load_fonts};
use crate::banners::Notices;
use crate::link::Ports;
use crate::look::Look;
use crate::platform::TrafficLights;
use crate::window::{Shown, WindowView};
use crate::wiring::{Hosts, Started, nothing_drawn, shut_down};

const WINDOW_WIDTH: f32 = 1100.;
const WINDOW_HEIGHT: f32 = 720.;
const MIN_WINDOW_WIDTH: f32 = 520.;
const MIN_WINDOW_HEIGHT: f32 = 320.;
/// Past a runtime's slowest stop (two waits of two seconds) with room for the save and the services.
const SHUTDOWN_PATIENCE: Duration = Duration::from_secs(8);

/// What is running behind the window, and whether quitting has begun.
struct Running {
    hosts: Hosts,
    stopping: bool,
    _terminals: UnixStream,
}

impl Global for Running {}

/// Runs the app until it quits.
pub fn run() {
    let assets = Assets::for_this_build();
    let application = gpui_platform::application().with_assets(assets);
    let background = platform::is_background_window();
    application.run(move |cx: &mut App| {
        if background {
            platform::keep_out_of_the_dock(cx);
        }
        if let Err(error) = load_fonts(cx) {
            eprintln!("the window's fonts did not load: {error:#}");
        }
        install_menu(cx);
        text_field::bind_keys(cx);
        let system = appearance_of(cx.window_appearance());
        let system_is_dark = system == Appearance::Dark;
        let started = match wiring::start(system_is_dark) {
            Ok(started) => started,
            Err(problem) => {
                eprintln!("{problem}");
                cx.quit();
                return;
            }
        };
        let window = open_window(started, background, cx);
        if !background {
            cx.activate(true);
        }
        #[cfg(feature = "drive")]
        if let Some(window) = window {
            drive::listen(window, cx);
        }
        #[cfg(not(feature = "drive"))]
        let _ = window;
    });
}

/// The one way out: closes the window, stops everything off gpui's thread, then quits. Closing the window calls it again, which `stopping` makes a no-op.
pub(crate) fn quit(cx: &mut App) {
    let Some(running) = cx.try_global::<Running>() else {
        cx.quit();
        return;
    };
    if running.stopping {
        return;
    }
    let hosts = running.hosts.clone();
    cx.global_mut::<Running>().stopping = true;
    for window in cx.windows() {
        let _ = window.update(cx, |_, window, _| window.remove_window());
    }
    let executor = cx.background_executor();
    let stopped = executor.spawn(async move { shut_down(&hosts) });
    let given_up = executor.timer(SHUTDOWN_PATIENCE);
    // A wedged or panicked step must not leave the app running with no window.
    let quit_once_stopped = cx.spawn(async move |cx| {
        let stopped = stopped.fuse();
        let given_up = given_up.fuse();
        futures::pin_mut!(stopped, given_up);
        futures::select! {
            () = stopped => {}
            _ = given_up => eprintln!("quitting before everything had stopped"),
        }
        cx.update(|cx| cx.quit());
    });
    quit_once_stopped.detach();
}

fn install_menu(cx: &mut App) {
    menu::install(cx);
    // A terminal manager with no window has nothing left to manage.
    let quit_when_closed = cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            quit(cx);
        }
    });
    quit_when_closed.detach();
    // macOS quits a running app itself at log out, without `quit`: the save is still written, and the runtime stops when its connection closes.
    let save_at_quit = cx.on_app_quit(|cx| {
        if let Some(running) = cx.try_global::<Running>()
            && !running.stopping
        {
            running.hosts.workspace.freeze_and_flush();
        }
        async {}
    });
    save_at_quit.detach();
}

fn open_window(started: Started, background: bool, cx: &mut App) -> Option<AnyWindowHandle> {
    let window_size = size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT));
    let bounds = Bounds::centered(None, window_size, cx);
    let titlebar = TitlebarOptions {
        title: Some(APPLICATION_NAME.into()),
        appears_transparent: true,
        traffic_light_position: Some(platform::traffic_light_position()),
    };
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(titlebar),
        window_min_size: Some(size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT))),
        app_owns_titlebar_drag: true,
        focus: !background,
        show: !background,
        ..Default::default()
    };
    let Started {
        hosts,
        outputs,
        first_config,
        terminals,
    } = started;
    let shown = shown_from(&first_config);
    let look = Look::new(first_config.appearance);
    let window_hosts = hosts.clone();
    cx.set_global(Running {
        hosts,
        stopping: false,
        _terminals: terminals,
    });
    let opened = cx.open_window(options, move |window, cx| {
        let hosts = window_hosts;
        let services = hosts.services.clone();
        let activated = hosts.clone();
        let terminal_settings = hosts.terminal_settings.clone();
        let config_changed = move |config: &Config| {
            let _ = terminal_settings.send(config.clone());
        };
        let ports = Ports {
            workspace: Rc::new(hosts.workspace),
            services: Rc::new(hosts.services),
            outputs,
            config_changed: Box::new(config_changed),
        };
        let lights = TrafficLights::Native;
        cx.new(|cx| {
            // The look follows the theme setting, so the system's appearance goes to the services, whose new config redraws the window.
            let follow_appearance =
                cx.observe_window_appearance(window, move |_: &mut WindowView, window, _| {
                    let appearance = appearance_of(window.appearance());
                    services.set_system_dark(appearance == Appearance::Dark);
                });
            follow_appearance.detach();
            let follow_activation =
                cx.observe_window_activation(window, move |_: &mut WindowView, window, cx| {
                    if !window.is_window_active() {
                        return;
                    }
                    activated.workspace.app_activated();
                    // The setups re-read the agents' config files, which gpui's thread must not wait on.
                    let services = activated.services.clone();
                    let refreshed = cx.background_spawn(async move { services.app_activated() });
                    refreshed.detach();
                });
            follow_activation.detach();
            WindowView::new(shown, look, lights, ports, cx)
        })
    });
    match opened {
        Ok(window) => Some(window.into()),
        Err(error) => {
            eprintln!("the window did not open: {error:#}");
            quit(cx);
            None
        }
    }
}

/// The first frame: nothing published yet, and the config the services started with.
fn shown_from(config: &Config) -> Shown {
    let notices = Notices {
        config_problems: config.problems.clone(),
        ..Notices::default()
    };
    Shown {
        drawn: nothing_drawn(),
        notices,
        placeholder_screen: Vec::new(),
        setups: AgentSetupView::default(),
        bindings: config.bindings.clone(),
    }
}

pub(crate) fn appearance_of(system: WindowAppearance) -> Appearance {
    match system {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}
