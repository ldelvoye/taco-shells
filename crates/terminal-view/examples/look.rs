//! Draws a terminal of sample output, in the light and the dark theme, to PNG files with no window on screen.
//!
//! `cargo run -p taco-terminal-view --features capture --example look -- <directory>`

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{AppContext as _, HeadlessAppContext, px, size};
use taco_ghostty::{Snapshot, Terminal};
use taco_protocol::{TerminalColors, TerminalEvent, TerminalId, TerminalRequest};
use taco_settings::{Appearance, Settings, palette_for};
use taco_terminal_view::Terminals;

const SAMPLE: &str = concat!(
    "\x1b[32m➜\x1b[0m  \x1b[36mtaco-shells\x1b[0m \x1b[34mgit:(\x1b[31mmain\x1b[34m)\x1b[0m ls\r\n",
    "Cargo.toml  \x1b[1;34mcrates\x1b[0m  \x1b[1;34mdocs\x1b[0m  \x1b[1;34mhooks\x1b[0m  README.md  \x1b[1;32minstall.sh\x1b[0m\r\n",
    "\x1b[32m➜\x1b[0m  \x1b[36mtaco-shells\x1b[0m \x1b[34mgit:(\x1b[31mmain\x1b[34m)\x1b[0m cargo test -q\r\n",
    "running 19 tests\r\n",
    "\x1b[32m...................\x1b[0m\r\n",
    "test result: \x1b[32mok\x1b[0m. 19 passed; \x1b[31m0 failed\x1b[0m\r\n",
    "\r\n",
    " \x1b[30mblack\x1b[0m \x1b[31mred\x1b[0m \x1b[32mgreen\x1b[0m \x1b[33myellow\x1b[0m \x1b[34mblue\x1b[0m \x1b[35mmagenta\x1b[0m \x1b[36mcyan\x1b[0m \x1b[37mwhite\x1b[0m\r\n",
    " \x1b[90mblack\x1b[0m \x1b[91mred\x1b[0m \x1b[92mgreen\x1b[0m \x1b[93myellow\x1b[0m \x1b[94mblue\x1b[0m \x1b[95mmagenta\x1b[0m \x1b[96mcyan\x1b[0m \x1b[97mwhite\x1b[0m\r\n",
    " \x1b[40m  \x1b[41m  \x1b[42m  \x1b[43m  \x1b[44m  \x1b[45m  \x1b[46m  \x1b[47m  \x1b[0m",
    " \x1b[100m  \x1b[101m  \x1b[102m  \x1b[103m  \x1b[104m  \x1b[105m  \x1b[106m  \x1b[107m  \x1b[0m\r\n",
    "\r\n",
    " \x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4munderline\x1b[0m \x1b[4:3;58:5:1mcurly\x1b[0m",
    " \x1b[9mstruck\x1b[0m \x1b[2mfaint\x1b[0m \x1b[7minverse\x1b[0m\r\n",
    " 日本語のテキスト and https://tacoshells.dev\r\n",
    "\r\n",
    "\x1b[32m➜\x1b[0m  \x1b[36mtaco-shells\x1b[0m \x1b[34mgit:(\x1b[31mmain\x1b[34m)\x1b[0m ",
);

fn main() {
    let directory = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let directory = PathBuf::from(directory);
    for (appearance, name) in [(Appearance::Light, "light"), (Appearance::Dark, "dark")] {
        let colors = palette_for(appearance).terminal;
        let path = directory.join(format!("terminal-{name}.png"));
        draw(&colors, &path);
        println!("saved {}", path.display());
    }
}

fn draw(colors: &TerminalColors, path: &Path) {
    let (ours, runtime) = UnixStream::pair().expect("a socket pair");
    stand_in_runtime(runtime, colors.clone());

    let platform = gpui_platform::current_platform(true);
    let text_system = platform.text_system();
    let mut cx = HeadlessAppContext::with_platform(
        text_system,
        Arc::new(()),
        gpui_platform::current_headless_renderer,
    );
    cx.allow_parking();
    let settings = Settings::default();
    let window = cx
        .open_window(size(px(900.), px(420.)), |_, cx| {
            cx.new(|cx| Terminals::new(ours, &settings, colors, cx).expect("terminals"))
        })
        .expect("a headless window");
    let id = sample_id();
    window
        .update(&mut cx, |terminals, window, cx| {
            terminals.show(std::slice::from_ref(&id), Some(&id), cx);
            terminals.focus(&id, window, cx);
        })
        .expect("the window");

    let any_window = window.into();
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(20));
        cx.run_until_parked();
        let _ = cx.update_window(any_window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }
    let image = cx.capture_screenshot(any_window).expect("a screenshot");
    image.save(path).expect("the PNG written");
}

fn sample_id() -> TerminalId {
    TerminalId("sample".to_string())
}

/// Answers the view's attach with a snapshot of the sample, as the runtime would.
fn stand_in_runtime(mut stream: UnixStream, colors: TerminalColors) {
    std::thread::spawn(move || {
        while let Ok(Some(request)) = taco_wire::read_frame::<TerminalRequest>(&mut stream) {
            let TerminalRequest::Attach {
                id,
                attachment,
                size,
                ..
            } = request
            else {
                continue;
            };
            let Some(size) = size else {
                continue;
            };
            let mut terminal = Terminal::new(size).expect("a terminal");
            terminal.set_colors(&colors).expect("colors");
            terminal.write(SAMPLE.as_bytes());
            let snapshot = Snapshot::take(&terminal).expect("a snapshot");
            let attached = TerminalEvent::Attached {
                id: id.clone(),
                attachment,
                size,
                screen: snapshot.screen().to_vec(),
            };
            let _ = taco_wire::write_frame(&mut stream, &attached);
            let parts: Vec<Vec<u8>> = snapshot.history().map(|part| part.to_vec()).collect();
            let count = parts.len();
            for (index, part) in parts.into_iter().enumerate() {
                let history = TerminalEvent::History {
                    id: id.clone(),
                    attachment,
                    part,
                    last: index + 1 == count,
                };
                let _ = taco_wire::write_frame(&mut stream, &history);
            }
        }
    });
}
