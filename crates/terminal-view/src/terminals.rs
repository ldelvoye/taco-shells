//! The terminals in the middle of the window: a view for each one kept, and the one on screen drawn.

use std::collections::{HashMap, HashSet};
use std::io;
use std::os::unix::net::UnixStream;

use futures::StreamExt as _;
use gpui::{
    App, Bounds, Context, EventEmitter, Hsla, IntoElement, ParentElement as _, Pixels, Render,
    Styled as _, Task, Window, canvas, div, fill, font, px,
};
use taco_protocol::{TerminalColors, TerminalEvent, TerminalId, TerminalRequest, TerminalSize};
use taco_settings::Settings;

use crate::connection::{Requests, connect};
use crate::draw::{Painting, hsla, lay_out, paint};
use crate::grid::{Cell, font_family, grid_that_fits};
use crate::shown::{after_showing, viewed};
use crate::view::{Applied, View, ViewConfig};

/// The width of `m` as a share of the font size, for a font that cannot be measured.
const FALLBACK_ADVANCE: f32 = 0.6;

pub enum TerminalsEvent {
    /// A click put keyboard focus in this terminal. Not sent for a focus the app asked for.
    Focused(TerminalId),
    /// A link was clicked with Cmd held. The app decides which schemes may open.
    LinkClicked(String),
}

/// The views and the terminal channel. The app makes one when it starts the runtime and places it in the middle.
pub struct Terminals {
    requests: Requests,
    views: HashMap<TerminalId, View>,
    open: HashSet<TerminalId>,
    /// Ended while still in the arrangement: the runtime has nothing left to show for them.
    ended: HashSet<TerminalId>,
    recency: Vec<TerminalId>,
    on_screen: Option<TerminalId>,
    /// Counts every attach sent, so each has a number of its own.
    attachments: u64,
    /// The size last asked of the runtime for each terminal, so the same one is not asked twice.
    asked: HashMap<TerminalId, TerminalSize>,
    /// The middle's size in pixels, once it has been laid out.
    area: Option<(f32, f32)>,
    family: String,
    font_size: f32,
    cell: Cell,
    config: ViewConfig,
    _events: Task<()>,
}

/// What one frame paints, worked out before painting.
struct Prepared {
    painting: Painting,
    background: Hsla,
    cell: Cell,
    family: String,
    font_size: f32,
}

impl EventEmitter<TerminalsEvent> for Terminals {}

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
        let mut shown: Vec<TerminalId> = Vec::new();
        if let Some(id) = on_screen {
            shown.push(id.clone());
        }
        self.recency = after_showing(&self.recency, &shown, &self.open);
        let wanted = viewed(&self.recency, &shown, &self.open);

        // Views are let go before new ones are made, so the window never holds more than it ends with.
        let held: Vec<TerminalId> = self.views.keys().cloned().collect();
        for id in held {
            if wanted.contains(&id) {
                continue;
            }
            self.views.remove(&id);
            if self.open.contains(&id) {
                self.requests.send(TerminalRequest::Detach { id });
            }
        }
        let open_now = &self.open;
        self.ended.retain(|id| open_now.contains(id));
        self.asked.retain(|id, _| open_now.contains(id));

        for id in wanted {
            let has_view = self.views.contains_key(&id);
            let has_ended = self.ended.contains(&id);
            if !has_view && !has_ended {
                self.attach(id);
            }
        }
        self.on_screen = on_screen.cloned();
        cx.notify();
    }

    /// Asks the runtime to clear the terminal; the view clears when `Cleared` comes back at its place in the output.
    pub fn clear(&mut self, id: &TerminalId) {
        let request = TerminalRequest::Clear { id: id.clone() };
        self.requests.send(request);
    }

    fn attach(&mut self, id: TerminalId) {
        self.attachments += 1;
        let Ok(view) = View::new(self.attachments, self.config.clone()) else {
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

    fn receive(&mut self, event: TerminalEvent, cx: &mut Context<Terminals>) {
        let id = event_terminal(&event).clone();
        let Some(view) = self.views.get_mut(&id) else {
            return;
        };
        let cell = Some(self.cell.size_px());
        let applied = view.apply(event, cell);
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
            // A view that could not apply what it was sent is let go; it attaches afresh when next shown.
            Err(_) => {
                self.views.remove(&id);
                self.requests.send(TerminalRequest::Detach { id });
            }
        }
    }

    /// Steps restored history, asks for the size the middle fits, and lays out the terminal on screen.
    fn prepare(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> Option<Prepared> {
        let width = f32::from(bounds.size.width);
        let height = f32::from(bounds.size.height);
        self.area = Some((width, height));

        let mut history_left = false;
        for view in self.views.values_mut() {
            let stepped = view.step_history();
            if let Ok(true) = stepped {
                history_left = true;
            }
        }
        if history_left {
            window.request_animation_frame();
        }

        let id = self.on_screen.clone()?;
        let fits = grid_that_fits(width, height, self.cell);
        let already_asked = self.asked.get(&id) == Some(&fits);
        if !already_asked && self.views.contains_key(&id) {
            self.asked.insert(id.clone(), fits);
            let request = TerminalRequest::Resize {
                id: id.clone(),
                size: fits,
                cell: self.cell.size_px(),
            };
            self.requests.send(request);
        }

        let selection = self.config.colors.selection;
        let view = self.views.get_mut(&id)?;
        let Ok(Some((frame, rows))) = view.read() else {
            return None;
        };
        let colors = frame.colors().clone();
        let cursor = frame.cursor().ok()?;
        let painting = lay_out(rows, cursor, &colors, selection);
        let _ = frame.clean();
        Some(Prepared {
            painting,
            background: hsla(colors.background),
            cell: self.cell,
            family: self.family.clone(),
            font_size: self.font_size,
        })
    }
}

impl Render for Terminals {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Terminals>) -> impl IntoElement {
        let entity = cx.entity();
        let background = hsla(self.config.colors.background);
        div().size_full().bg(background).child(
            canvas(
                move |bounds, window, cx| {
                    entity.update(cx, |terminals, _| terminals.prepare(bounds, window))
                },
                move |bounds, prepared: Option<Prepared>, window, cx| {
                    let Some(prepared) = prepared else {
                        return;
                    };
                    window.paint_quad(fill(bounds, prepared.background));
                    paint(
                        &prepared.painting,
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

/// The cell `family` at `size` gives: the advance of `m` across, ascent and descent down.
fn measure(family: &str, size: f32, cx: &App) -> Cell {
    let text_system = cx.text_system();
    let font_id = text_system.resolve_font(&font(family.to_string()));
    let font_size = px(size);
    let mut width = size * FALLBACK_ADVANCE;
    if let Ok(advance) = text_system.advance(font_id, font_size, 'm') {
        width = f32::from(advance.width);
    }
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    let descent = f32::from(text_system.descent(font_id, font_size));
    let height = (ascent + descent.abs()).ceil();
    Cell { width, height }
}
