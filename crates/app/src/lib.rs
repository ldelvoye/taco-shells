//! The window, and the wiring that starts everything.

mod activity;
mod assets;
mod banners;
pub mod drive;
mod fixture;
pub mod link;
mod look;
mod panels;
mod platform;
mod popups;
#[cfg(feature = "render")]
pub mod render;
pub mod runtime_client;
pub mod services_host;
mod stand_in;
mod strip;
mod terminal_list;
mod text_field;
mod title_bar;
mod window;
pub mod workspace_host;

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions,
    WindowAppearance, WindowBounds, WindowOptions, actions, px, size,
};
use taco_services::APPLICATION_NAME;
use taco_settings::Appearance;

pub use fixture::Fixture;

use crate::assets::{Assets, load_fonts};
use crate::look::Look;
use crate::platform::TrafficLights;
use crate::stand_in::StandIn;
use crate::window::WindowView;

const WINDOW_WIDTH: f32 = 1100.;
const WINDOW_HEIGHT: f32 = 720.;
const MIN_WINDOW_WIDTH: f32 = 520.;
const MIN_WINDOW_HEIGHT: f32 = 320.;

actions!(taco_shells, [Quit]);

/// Opens the window on a fixture and runs until it closes.
pub fn run(fixture: Fixture) {
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
        let window = open_window(fixture, background, cx);
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

fn install_menu(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    let quit = MenuItem::action("Quit", Quit);
    let app_menu = Menu::new(APPLICATION_NAME).items([quit]);
    cx.set_menus([app_menu]);
    // A terminal manager with no window has nothing left to manage.
    let quit_when_closed = cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    });
    quit_when_closed.detach();
}

fn open_window(fixture: Fixture, background: bool, cx: &mut App) -> Option<AnyWindowHandle> {
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
    let opened = cx.open_window(options, |window, cx| {
        let appearance = appearance_of(window.appearance());
        let look = Look::new(appearance);
        let shown = fixture.shown();
        let ports = StandIn::start(&shown, fixture.logs());
        cx.new(|cx| {
            let follow =
                cx.observe_window_appearance(window, |view: &mut WindowView, window, cx| {
                    let appearance = appearance_of(window.appearance());
                    view.set_look(Look::new(appearance));
                    cx.notify();
                });
            follow.detach();
            let lights = TrafficLights::Native;
            WindowView::new(
                shown,
                look,
                lights,
                ports.workspace,
                ports.services,
                ports.outputs,
                cx,
            )
        })
    });
    match opened {
        Ok(window) => Some(window.into()),
        Err(error) => {
            eprintln!("the window did not open: {error:#}");
            cx.quit();
            None
        }
    }
}

pub(crate) fn appearance_of(system: WindowAppearance) -> Appearance {
    match system {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}
