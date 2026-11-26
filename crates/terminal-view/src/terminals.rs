//! The terminals in the middle of the window: a view for each one kept, and the one on screen drawn.

use std::collections::{HashMap, HashSet};
use std::io;
use std::os::unix::net::UnixStream;

use futures::StreamExt as _;
use gpui::{
    App, Bounds, Context, IntoElement, ParentElement as _, Pixels, Render, Styled as _, Task,
    Window, canvas, div, fill, font, px,
};
use taco_protocol::{TerminalColors, TerminalEvent, TerminalId, TerminalRequest};
use taco_settings::Settings;

use crate::connection::{Requests, connect};
use crate::draw::{hsla, paint};
use crate::grid::{CellMetrics, font_family, grid_that_fits};
use crate::shown::{after_showing, viewed};
use crate::view::{Applied, Drawn, View, ViewConfig};

/// The width of `m` as a share of the font size, for a font that cannot be measured.
const FALLBACK_ADVANCE: f32 = 0.6;

/// The views and the terminal channel. The app makes one when it starts the runtime and places it in the middle.
pub struct Terminals {
    requests: Requests,
    views: HashMap<TerminalId, View>,
    open: HashSet<TerminalId>,
    /// Their shell exited while they are still in the arrangement: the runtime holds nothing more for them, so they get no view.
    ended: HashSet<TerminalId>,
    /// Their view failed since the arrangement last changed, and they are not attached again until it changes.
    failed: HashSet<TerminalId>,
    recency: Vec<TerminalId>,
    on_screen: Option<TerminalId>,
    /// Counts every attach sent, so each has a number of its own.
    attachments: u64,
    /// The size last asked of the runtime for each terminal, so the same one is not asked twice.
    asked: HashMap<TerminalId, taco_protocol::TerminalSize>,
    /// The middle's size in pixels, once it has been laid out.
    area: Option<(f32, f32)>,
    family: String,
    font_size: f32,
    cell: CellMetrics,
    config: ViewConfig,
    _events: Task<()>,
}

/// What one frame paints, worked out before painting.
struct Prepared {
    drawn: Drawn,
    cell: CellMetrics,
    family: String,
    font_size: f32,
}

impl Terminals {
    pub fn new(
        connection: UnixStream,
        settings: &Settings,
        colors: &TerminalColors,
        cx: &mut Context<Terminals>,
    ) -> io::Result<Terminals> {
        let (requests, mut events) = connect(connection)?;
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let received = this.update(cx, |terminals, cx| terminals.receive(event, cx));
                if received.is_err() {
                    break;
                }
            }
        });

        let installed = cx.text_system().all_font_names();
        let family = font_family(&settings.font_family, &installed);
        let font_size = settings.font_size as f32;
        let cell = measure(&family, font_size, cx);
        let config = ViewConfig {
            scrollback: settings.scrollback,
            colors: colors.clone(),
        };
        Ok(Terminals {
            requests,
            views: HashMap::new(),
            open: HashSet::new(),
            ended: HashSet::new(),
            failed: HashSet::new(),
            recency: Vec::new(),
            on_screen: None,
            attachments: 0,
            asked: HashMap::new(),
            area: None,
            family,
            font_size,
            cell,
            config,
            _events: task,
        })
    }

    /// The open terminals and the one in the middle. Sent on every change to the arrangement; cheap when nothing changed.
    pub fn show(
        &mut self,
        open: &[TerminalId],
        on_screen: Option<&TerminalId>,
        cx: &mut Context<Terminals>,
    ) {
        self.open = open.iter().cloned().collect();
        self.recency = after_showing(&self.recency, on_screen, &self.open);
        let wanted = viewed(&self.recency);

        // Views are let go before new ones are made.
        let held: Vec<TerminalId> = self.views.keys().cloned().collect();
        for id in held {
            if !wanted.contains(&id) {
                self.let_go(&id);
            }
        }
        let open_now = &self.open;
        self.ended.retain(|id| open_now.contains(id));
        self.asked.retain(|id, _| open_now.contains(id));
        self.failed.clear();

        for id in wanted {
            self.attach_if_missing(id);
        }
        self.on_screen = on_screen.cloned();
        cx.notify();
    }

    /// Asks the runtime to clear the terminal; the view clears when `Cleared` comes back at its place in the output.
    pub fn clear(&mut self, id: &TerminalId) {
        let request = TerminalRequest::Clear { id: id.clone() };
        self.requests.send(request);
    }

    fn attach_if_missing(&mut self, id: TerminalId) {
        let missing = !self.views.contains_key(&id)
            && !self.ended.contains(&id)
            && !self.failed.contains(&id);
        if missing {
            self.attach(id);
        }
    }

    fn attach(&mut self, id: TerminalId) {
        self.attachments += 1;
        let Ok(view) = View::new(self.attachments, self.config.clone()) else {
            self.failed.insert(id);
            return;
        };
        let mut size = None;
        if let Some((width, height)) = self.area {
            let fits = grid_that_fits(width, height, self.cell);
            self.asked.insert(id.clone(), fits);
            size = Some(fits);
        }
        let request = TerminalRequest::Attach {
            id: id.clone(),
            attachment: self.attachments,
            size,
            cell: Some(self.cell.size_px()),
        };
        self.views.insert(id, view);
        self.requests.send(request);
    }

    fn let_go(&mut self, id: &TerminalId) {
        self.views.remove(id);
        if self.open.contains(id) {
            self.requests
                .send(TerminalRequest::Detach { id: id.clone() });
        }
    }

    /// A view that could not apply what it was sent starts over from a fresh snapshot, once per change to the arrangement.
    fn start_over(&mut self, id: TerminalId) {
        self.let_go(&id);
        let first_failure = self.failed.insert(id.clone());
        let still_wanted = viewed(&self.recency).contains(&id);
        if first_failure && still_wanted {
            self.attach(id);
        }
    }

    fn receive(&mut self, event: TerminalEvent, cx: &mut Context<Terminals>) {
        let id = event_terminal(&event).clone();
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let applied = view.apply(event);
        let on_screen = self.on_screen.as_ref() == Some(&id);
        match applied {
            Ok(Applied::Ignored) => {}
            Ok(Applied::Changed) => {
                if on_screen {
                    cx.notify();
                }
            }
            Ok(Applied::Ended) => {
                self.views.remove(&id);
                self.ended.insert(id);
                cx.notify();
            }
            Err(_) => {
                self.start_over(id);
                cx.notify();
            }
        }
    }

    /// The frame for the terminal on screen, after the work every frame does first.
    fn before_paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> Option<Prepared> {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        self.area = Some((width, height));
        self.step_restored_history(window);
        self.ask_for_the_size_that_fits(width, height);
        self.draw_on_screen()
    }

    // A page of each view's restored history per frame, asking for frames until it is all in.
    fn step_restored_history(&mut self, window: &mut Window) {
        let mut history_left = false;
        let mut broken = Vec::new();
        for (id, view) in self.views.iter_mut() {
            match view.step_history() {
                Ok(true) => history_left = true,
                Ok(false) => {}
                Err(_) => broken.push(id.clone()),
            }
        }
        for id in broken {
            self.start_over(id);
        }
        if history_left {
            window.request_animation_frame();
        }
    }

    fn ask_for_the_size_that_fits(&mut self, width: f32, height: f32) {
        let Some(id) = self.on_screen.clone() else {
            return;
        };
        let fits = grid_that_fits(width, height, self.cell);
        let already_asked = self.asked.get(&id) == Some(&fits);
        if already_asked || !self.views.contains_key(&id) {
            return;
        }
        self.asked.insert(id.clone(), fits);
        let request = TerminalRequest::Resize {
            id,
            size: fits,
            cell: self.cell.size_px(),
        };
        self.requests.send(request);
    }

    fn draw_on_screen(&mut self) -> Option<Prepared> {
        let id = self.on_screen.clone()?;
        let selection = self.config.colors.selection;
        let view = self.views.get_mut(&id)?;
        let drawn = match view.draw(selection) {
            Ok(Some(drawn)) => drawn,
            Ok(None) => return None,
            Err(_) => {
                self.start_over(id);
                return None;
            }
        };
        Some(Prepared {
            drawn,
            cell: self.cell,
            family: self.family.clone(),
            font_size: self.font_size,
        })
    }
}

impl Render for Terminals {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Terminals>) -> impl IntoElement {
        let entity = cx.entity();
        // The theme's background until a view draws; a program can change its own, which the frame then paints.
        let background = hsla(self.config.colors.background);
        div().size_full().bg(background).child(
            canvas(
                move |bounds, window, cx| {
                    entity.update(cx, |terminals, _| terminals.before_paint(bounds, window))
                },
                move |bounds, prepared: Option<Prepared>, window, cx| {
                    let Some(prepared) = prepared else {
                        return;
                    };
                    window.paint_quad(fill(bounds, hsla(prepared.drawn.background)));
                    paint(
                        &prepared.drawn.painting,
                        bounds.origin,
                        prepared.cell,
                        &prepared.family,
                        px(prepared.font_size),
                        window,
                        cx,
                    );
                },
            )
            .size_full(),
        )
    }
}

fn event_terminal(event: &TerminalEvent) -> &TerminalId {
    match event {
        TerminalEvent::Attached { id, .. } => id,
        TerminalEvent::History { id, .. } => id,
        TerminalEvent::Output { id, .. } => id,
        TerminalEvent::Resized { id, .. } => id,
        TerminalEvent::Cleared { id } => id,
        TerminalEvent::End { id } => id,
    }
}

/// The cell `family` gives at `size`: the advance of `m` across, ascent and descent down.
fn measure(family: &str, size: f32, cx: &App) -> CellMetrics {
    let text_system = cx.text_system();
    let font_id = text_system.resolve_font(&font(family.to_string()));
    let font_size = px(size);
    let mut width = size * FALLBACK_ADVANCE;
    if let Ok(advance) = text_system.advance(font_id, font_size, 'm') {
        width = f32::from(advance.width);
    }
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    // font-kit reports the descent below the baseline as a negative number.
    let descent = f32::from(text_system.descent(font_id, font_size)).abs();
    let height = (ascent + descent).ceil();
    CellMetrics { width, height }
}
