//! The window, and the wiring that starts everything.

mod activity_log;
mod activity_panel;
mod addresses;
mod assets;
mod banners;
mod done_checks;
pub mod drive;
#[cfg(feature = "render")]
mod fixture;
mod key_route;
mod keys;
pub mod link;
mod look;
mod menu;
mod middle;
mod motion;
mod page;
mod panels;
mod parts;
mod platform;
mod popups;
#[cfg(feature = "render")]
pub mod render;
mod row_name;
mod settings_page;
mod shared_view;
mod shell;
mod shortcuts_page;
mod slot;
#[cfg(feature = "render")]
mod stand_in;
mod strip;
mod terminal_list;
mod text_field;
mod title_bar;
mod window;

use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt;

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Global, IntoElement, PromptLevel, Render,
    TitlebarOptions, Window, WindowAppearance, WindowBounds, WindowOptions, div, px, size,
};
use taco_host::wiring::{self, Hosts, Started, nothing_drawn, shut_down};
use taco_identity::CURRENT;
use taco_services::AgentSetupView;
use taco_settings::{Appearance, Config, palette_for};
use taco_terminal_view::Terminals;

#[cfg(feature = "render")]
pub use fixture::Fixture;

use crate::assets::{Assets, load_fonts};
use crate::banners::Notices;
use crate::link::Ports;
use crate::look::Look;
use crate::platform::TrafficLights;
use crate::window::{Shown, WindowView};

const WINDOW_WIDTH: f32 = 1100.;
const WINDOW_HEIGHT: f32 = 720.;
const MIN_WINDOW_WIDTH: f32 = 520.;
const MIN_WINDOW_HEIGHT: f32 = 320.;
/// Past the slowest shutdown: 1.5 s for the save, 2 s for the services, 4 s for the runtime, 1.5 s for the save it freed.
const SHUTDOWN_PATIENCE: Duration = Duration::from_secs(10);
const START_FAILED: &str = "Taco Shells could not start";
const QUIT: &str = "Quit";
const FAILURE_WINDOW_WIDTH: f32 = 480.;
const FAILURE_WINDOW_HEIGHT: f32 = 200.;

/// What is running behind the window, and whether quitting has begun.
struct Running {
    hosts: Hosts,
    stopping: bool,
}

impl Global for Running {}

/// The window a start failure is shown on: nothing but the alert.
struct Blank;

impl Render for Blank {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

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
                show_start_failure(problem, background, cx);
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

/// Says why the app cannot start, on a window of its own since gpui alerts only on a window, then quits. A hidden launch has no one to tell.
fn show_start_failure(problem: String, background: bool, cx: &mut App) {
    if background {
        cx.quit();
        return;
    }
    let window_size = size(px(FAILURE_WINDOW_WIDTH), px(FAILURE_WINDOW_HEIGHT));
    let bounds = Bounds::centered(None, window_size, cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    };
    let opened = cx.open_window(options, |_, cx| cx.new(|_| Blank));
    let Ok(window) = opened else {
        cx.quit();
        return;
    };
    cx.activate(true);
    let asked = window.update(cx, |_, window, cx| {
        window.prompt(
            PromptLevel::Critical,
            START_FAILED,
            Some(&problem),
            &[QUIT],
            cx,
        )
    });
    let Ok(answered) = asked else {
        cx.quit();
        return;
    };
    let quit_once_answered = cx.spawn(async move |cx| {
        let _ = answered.await;
        cx.update(|cx| cx.quit());
    });
    quit_once_answered.detach();
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
        title: Some(CURRENT.application_name.into()),
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
    let mut shown = shown_from(&first_config);
    let look = Look::new(first_config.appearance);
    let connected = Terminals::connect(terminals);
    let channel = match connected {
        Ok(channel) => Some(channel),
        Err(error) => {
            let problem = format!("The terminals could not be shown: {error}");
            shown.notices.problems.push(problem);
            None
        }
    };
    let window_hosts = hosts.clone();
    cx.set_global(Running {
        hosts,
        stopping: false,
    });
    let opened = cx.open_window(options, move |window, cx| {
        let hosts = window_hosts;
        let settings = &first_config.settings;
        let palette = palette_for(first_config.appearance);
        let mut terminals = None;
        if let Some(channel) = channel {
            let made = cx.new(|cx| Terminals::new(channel, settings, &palette.terminal, cx));
            terminals = Some(made);
        }
        let services = hosts.services.clone();
        let activated = hosts.services.clone();
        let terminal_settings = hosts.terminal_settings.clone();
        let config_changed = move |config: &Config| {
            let _ = terminal_settings.send(config.clone());
        };
        let ports = Ports {
            workspace: Rc::new(hosts.workspace),
            services: Rc::new(hosts.services),
            outputs,
            config_changed: Box::new(config_changed),
            terminals,
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
                    // The setups re-read the agents' config files, which gpui's thread must not wait on.
                    let services = activated.clone();
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
        default_bindings: config.default_bindings.clone(),
        unread_keybindings: config.unread_keybindings,
        settings: config.settings.clone(),
        unread: config.unread_settings.clone(),
    }
}

pub(crate) fn appearance_of(system: WindowAppearance) -> Appearance {
    match system {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}
