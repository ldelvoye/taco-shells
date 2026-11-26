//! The window, and the wiring that starts everything.

mod assets;
mod banners;
mod fixture;
pub mod link;
mod look;
mod panels;
mod platform;
#[cfg(feature = "render")]
pub mod render;
mod stand_in;
mod strip;
mod terminal_list;
mod title_bar;
mod trial;
mod window;

use gpui::{
    App, AppContext, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions, WindowAppearance,
    WindowBounds, WindowOptions, actions, px, size,
};
use taco_services::APPLICATION_NAME;
use taco_settings::Appearance;

pub use fixture::Fixture;
pub use trial::{BannerLayout, PlusPlace, Trial};

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
    application.run(move |cx: &mut App| {
        if let Err(error) = load_fonts(cx) {
            eprintln!("the window's fonts did not load: {error:#}");
        }
        install_menu(cx);
        open_window(fixture, cx);
        cx.activate(true);
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

fn open_window(fixture: Fixture, cx: &mut App) {
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
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        let appearance = appearance_of(window.appearance());
        let look = Look::new(appearance);
        let shown = fixture.shown();
        let (port, outputs) = StandIn::start(shown.drawn.clone());
        cx.new(|cx| {
            let follow =
                cx.observe_window_appearance(window, |view: &mut WindowView, window, cx| {
                    let appearance = appearance_of(window.appearance());
                    view.set_look(Look::new(appearance));
                    cx.notify();
                });
            follow.detach();
            let trial = Trial::default();
            let lights = TrafficLights::Native;
            WindowView::new(shown, look, trial, lights, port, outputs, cx)
        })
    });
    if let Err(error) = opened {
        eprintln!("the window did not open: {error:#}");
        cx.quit();
    }
}

pub(crate) fn appearance_of(system: WindowAppearance) -> Appearance {
    match system {
        WindowAppearance::Light | WindowAppearance::VibrantLight => Appearance::Light,
        WindowAppearance::Dark | WindowAppearance::VibrantDark => Appearance::Dark,
    }
}
