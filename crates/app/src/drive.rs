//! The smoke tests' way into the running window: a Unix socket taking `taco_drive` requests, each answered on the
//! window's thread. Without the `drive` feature only `mark` exists, and it changes nothing.

use gpui::{ParentElement, SharedString};

#[cfg(feature = "drive")]
pub use enabled::listen;

/// Records where `element` is drawn under `name`, for `Bounds` requests. Returns `element` untouched without the feature.
pub fn mark<E: ParentElement>(element: E, name: impl Into<SharedString>) -> E {
    #[cfg(feature = "drive")]
    {
        let recorder = enabled::recorder(name.into());
        element.child(recorder)
    }
    #[cfg(not(feature = "drive"))]
    {
        let _ = name;
        element
    }
}

#[cfg(feature = "drive")]
mod enabled {
    use std::collections::HashMap;
    use std::io::{self, BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use std::thread;

    use futures::StreamExt;
    use futures::channel::mpsc::{UnboundedSender, unbounded};
    use gpui::{
        AnyElement, AnyWindowHandle, App, AsyncApp, Bounds, Global, IntoElement, Keystroke,
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
        PlatformInput, SharedString, Styled, Window, canvas, point, px,
    };
    use taco_drive::{Appearance, DriveReply, DriveRequest, Point, Rect, SOCKET_VARIABLE};
    use taco_protocol::TerminalId;

    use crate::appearance_of;
    use crate::look::Look;
    use crate::window::WindowView;

    const DRAG_STEPS: u16 = 8;
    const ON_FIXTURES: &str = "the window runs on fixtures, with no terminals";

    #[derive(Default)]
    struct BoundsTable {
        drawn: HashMap<SharedString, Bounds<Pixels>>,
    }

    impl Global for BoundsTable {}

    struct Job {
        request: DriveRequest,
        reply: mpsc::Sender<DriveReply>,
    }

    pub(super) fn recorder(name: SharedString) -> AnyElement {
        let record = move |bounds: Bounds<Pixels>, _: &mut Window, cx: &mut App| {
            if cx.has_global::<BoundsTable>() {
                let table = cx.global_mut::<BoundsTable>();
                table.drawn.insert(name, bounds);
            }
        };
        canvas(record, |_, _, _, _| {})
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element()
    }

    /// Listens on `TACO_SHELLS_DRIVE_SOCKET` when it is set; does nothing when it is not.
    pub fn listen(window: AnyWindowHandle, cx: &mut App) {
        let Some(socket) = std::env::var_os(SOCKET_VARIABLE) else {
            return;
        };
        let socket = PathBuf::from(socket);
        cx.set_global(BoundsTable::default());

        let (jobs, mut queue) = unbounded::<Job>();
        let serving = thread::Builder::new()
            .name("drive".into())
            .spawn(move || serve(&socket, jobs));
        if let Err(error) = serving {
            eprintln!("the driver did not start: {error}");
            return;
        }

        let answering = cx.spawn(async move |cx: &mut AsyncApp| {
            while let Some(job) = queue.next().await {
                let reply = cx.update(|cx| answer(window, job.request, cx));
                let _ = job.reply.send(reply);
            }
        });
        answering.detach();
    }

    fn serve(socket: &Path, jobs: UnboundedSender<Job>) {
        let _ = std::fs::remove_file(socket);
        let listener = match UnixListener::bind(socket) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!(
                    "the driver could not listen on {}: {error}",
                    socket.display()
                );
                return;
            }
        };
        for connection in listener.incoming() {
            let conversed = match connection {
                Ok(stream) => converse(stream, &jobs),
                Err(error) => Err(error),
            };
            if let Err(error) = conversed {
                eprintln!("a driver connection ended: {error}");
            }
            if jobs.is_closed() {
                return;
            }
        }
    }

    fn converse(stream: UnixStream, jobs: &UnboundedSender<Job>) -> io::Result<()> {
        let mut writer = stream.try_clone()?;
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let line = line?;
            let parsed = serde_json::from_str::<DriveRequest>(&line);
            let reply = match parsed {
                Ok(request) => on_the_window(request, jobs),
                Err(error) => DriveReply::Failed(format!("not a request: {error}")),
            };
            let text = serde_json::to_string(&reply).map_err(io::Error::other)?;
            writeln!(writer, "{text}")?;
        }
        Ok(())
    }

    fn on_the_window(request: DriveRequest, jobs: &UnboundedSender<Job>) -> DriveReply {
        let (reply, answered) = mpsc::channel();
        let job = Job { request, reply };
        if jobs.unbounded_send(job).is_err() {
            return DriveReply::Failed("the window is gone".to_string());
        }
        match answered.recv() {
            Ok(reply) => reply,
            Err(_) => DriveReply::Failed("the window is gone".to_string()),
        }
    }

    fn answer(window: AnyWindowHandle, request: DriveRequest, cx: &mut App) -> DriveReply {
        let outcome = match request {
            DriveRequest::Keys { keys } => press(window, &keys, cx),
            DriveRequest::Text { text } => type_text(window, text, cx),
            DriveRequest::Drag { from, to } => drag(window, from, to, cx),
            DriveRequest::Bounds { target } => bounds(window, &target, cx),
            DriveRequest::DrawnState => drawn_state(window, cx),
            DriveRequest::ScreenText { terminal } => screen_text(window, &terminal, cx),
            DriveRequest::Render { path, appearance } => render(window, &path, appearance, cx),
        };
        match outcome {
            Ok(reply) => reply,
            Err(reason) => DriveReply::Failed(reason),
        }
    }

    fn closed(error: impl std::fmt::Display) -> String {
        format!("the window is closed: {error}")
    }

    fn draw_now(window: &mut Window, cx: &mut App) {
        window.refresh();
        window.draw(cx).clear(cx);
    }

    fn press(window: AnyWindowHandle, keys: &[String], cx: &mut App) -> Result<DriveReply, String> {
        let os = cx.platform_info().os;
        for key in keys {
            let keystroke = Keystroke::parse_with_os(key, os)
                .map_err(|error| format!("{key} is not a keystroke: {error:?}"))?;
            window
                .update(cx, |_, window, cx| {
                    window.dispatch_keystroke(keystroke, cx);
                })
                .map_err(closed)?;
        }
        Ok(DriveReply::Done)
    }

    // Text arrives as a key no binding takes, so the window hands it to the focused input handler whole, as a paste
    // or a composed character does.
    fn type_text(
        window: AnyWindowHandle,
        text: String,
        cx: &mut App,
    ) -> Result<DriveReply, String> {
        let keystroke = Keystroke {
            modifiers: Modifiers::default(),
            key: text.clone(),
            key_char: Some(text),
        };
        window
            .update(cx, |_, window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            })
            .map_err(closed)?;
        Ok(DriveReply::Done)
    }

    fn drag(
        window: AnyWindowHandle,
        from: Point,
        to: Point,
        cx: &mut App,
    ) -> Result<DriveReply, String> {
        let start = point(px(from.x), px(from.y));
        let end = point(px(to.x), px(to.y));
        let mut events = vec![PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: start,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        })];
        for step in 1..=DRAG_STEPS {
            let fraction = f32::from(step) / f32::from(DRAG_STEPS);
            let x = from.x + (to.x - from.x) * fraction;
            let y = from.y + (to.y - from.y) * fraction;
            events.push(PlatformInput::MouseMove(MouseMoveEvent {
                position: point(px(x), px(y)),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }));
        }
        events.push(PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: end,
            modifiers: Modifiers::default(),
            click_count: 1,
        }));

        // Drawn after each event, as frames come between a real drag's events, so hover and drop targets are current.
        for event in events {
            window
                .update(cx, |_, window, cx| {
                    window.dispatch_event(event, cx);
                    draw_now(window, cx);
                })
                .map_err(closed)?;
        }
        Ok(DriveReply::Done)
    }

    // The table is emptied and the window drawn, so a thing that is no longer drawn has no entry.
    fn bounds(window: AnyWindowHandle, target: &str, cx: &mut App) -> Result<DriveReply, String> {
        cx.global_mut::<BoundsTable>().drawn.clear();
        window
            .update(cx, |_, window, cx| draw_now(window, cx))
            .map_err(closed)?;
        let table = cx.global::<BoundsTable>();
        let found = table.drawn.get(target);
        let rect = found.map(rect_of);
        Ok(DriveReply::Bounds(rect))
    }

    fn rect_of(bounds: &Bounds<Pixels>) -> Rect {
        let origin = Point {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
        };
        Rect {
            origin,
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
        }
    }

    // Drawn first, as a view attaches and takes in its output when the window draws.
    fn screen_text(
        window: AnyWindowHandle,
        terminal: &TerminalId,
        cx: &mut App,
    ) -> Result<DriveReply, String> {
        window
            .update(cx, |_, window, cx| draw_now(window, cx))
            .map_err(closed)?;
        let views = window
            .read::<WindowView, _, _>(cx, |view, cx| view.read(cx).terminals())
            .map_err(closed)?;
        let Some(views) = views else {
            return Err(ON_FIXTURES.to_string());
        };
        let lines = views.update(cx, |views, _| views.visible_lines(terminal));
        match lines {
            Some(lines) => Ok(DriveReply::ScreenText(lines)),
            None => Err(format!("terminal {} has no view on screen", terminal.0)),
        }
    }

    fn drawn_state(window: AnyWindowHandle, cx: &mut App) -> Result<DriveReply, String> {
        let state = window
            .read::<WindowView, _, _>(cx, |view, cx| view.read(cx).drawn())
            .map_err(closed)?;
        Ok(DriveReply::DrawnState(Box::new(state)))
    }

    // Drawn in the asked appearance, then given back the window's own, which follows the system.
    fn render(
        window: AnyWindowHandle,
        path: &str,
        appearance: Appearance,
        cx: &mut App,
    ) -> Result<DriveReply, String> {
        let Some(view) = window.downcast::<WindowView>() else {
            return Err("the window's root is not the window view".to_string());
        };
        let asked = settings_appearance(appearance);
        view.update(cx, |view, _, _| view.set_look(Look::new(asked)))
            .map_err(closed)?;
        let drawn = window
            .update(cx, |_, window, cx| {
                draw_now(window, cx);
                let image = window.render_to_image();
                let own = appearance_of(window.appearance());
                (image, own)
            })
            .map_err(closed)?;
        let (image, own) = drawn;
        view.update(cx, |view, _, cx| {
            view.set_look(Look::new(own));
            cx.notify();
        })
        .map_err(closed)?;

        let image = image.map_err(|error| format!("the window could not be drawn: {error}"))?;
        image
            .save(path)
            .map_err(|error| format!("could not write {path}: {error}"))?;
        Ok(DriveReply::Done)
    }

    fn settings_appearance(appearance: Appearance) -> taco_settings::Appearance {
        match appearance {
            Appearance::Light => taco_settings::Appearance::Light,
            Appearance::Dark => taco_settings::Appearance::Dark,
        }
    }
}
