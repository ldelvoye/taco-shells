//! Ghostty's terminal library, libghostty-vt, for the runtime and the terminal view.

pub mod input;
pub mod render;
mod restore;
mod snapshot;
mod terminal;

#[allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    unsafe_op_in_unsafe_fn,
    clippy::all
)]
#[rustfmt::skip]
pub mod sys;

pub use restore::Step;
pub use snapshot::Snapshot;
pub use terminal::{Cursor, Error, Terminal};

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;
    use taco_protocol::{Rgb, TerminalColors, TerminalSize};

    const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

    fn build_info<T: Default>(key: sys::GhosttyBuildInfo) -> T {
        let mut value = T::default();
        let pointer = &mut value as *mut T as *mut c_void;
        let result = unsafe { sys::ghostty_build_info(key, pointer) };
        assert_eq!(result, sys::GhosttyResult_GHOSTTY_SUCCESS);
        value
    }

    fn encoded(terminal: &Terminal) -> Vec<u8> {
        let snapshot = Snapshot::take(terminal).unwrap();
        let mut bytes = snapshot.screen().to_vec();
        for part in snapshot.history() {
            bytes.extend_from_slice(part);
        }
        bytes
    }

    fn plain_text(terminal: &Terminal) -> String {
        let mut formatter: sys::GhosttyFormatter = ptr::null_mut();
        let made = unsafe {
            sys::ghostty_formatter_terminal_new(ptr::null(), &mut formatter, terminal.as_raw())
        };
        assert_eq!(made, sys::GhosttyResult_GHOSTTY_SUCCESS);
        let mut text: *mut u8 = ptr::null_mut();
        let mut len = 0;
        let formatted = unsafe {
            sys::ghostty_formatter_format_alloc(formatter, ptr::null(), &mut text, &mut len)
        };
        assert_eq!(formatted, sys::GhosttyResult_GHOSTTY_SUCCESS);
        let bytes = unsafe { std::slice::from_raw_parts(text, len) };
        let owned = String::from_utf8_lossy(bytes).into_owned();
        unsafe { sys::ghostty_free(ptr::null(), text, len) };
        unsafe { sys::ghostty_formatter_free(formatter) };
        owned
    }

    fn scrollback_rows(terminal: &Terminal) -> usize {
        let mut rows = 0usize;
        terminal
            .get(
                sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS,
                &mut rows,
            )
            .unwrap();
        rows
    }

    fn numbered_lines(count: usize) -> Vec<u8> {
        let mut lines = Vec::new();
        for n in 0..count {
            let color = 31 + n % 7;
            lines.extend_from_slice(
                format!("\x1b[{color}mline {n} of the earlier output\x1b[0m\r\n").as_bytes(),
            );
        }
        lines
    }

    #[test]
    fn a_terminal_restored_part_by_part_is_the_terminal_it_came_from() {
        let mut source = Terminal::new(SIZE).unwrap();
        source.set_scrollback(20_000).unwrap();
        let history = numbered_lines(15_000);
        source.write(&history);
        // A color a program set, the alternate screen over the history, and a sequence cut in half.
        source.write(b"\x1b]4;2;rgb:12/34/56\x07\x1b[?1049h\x1b[5;5Hfull screen\x1b[3");

        let snapshot = Snapshot::take(&source).unwrap();
        let parts: Vec<&[u8]> = snapshot.history().collect();
        assert!(
            parts.len() >= 2,
            "15,000 lines of history should take more than one part"
        );

        let mut restored = Terminal::restore(snapshot.screen()).unwrap();
        assert_eq!(restored.cursor().unwrap(), source.cursor().unwrap());
        assert_eq!(plain_text(&restored), plain_text(&source));

        let (last, earlier) = parts.split_last().unwrap();
        for part in earlier {
            restored.push_history(part, false);
            assert_eq!(restored.step_history().unwrap(), Step::Waiting);
        }
        restored.push_history(last, true);

        let mut applied = 0;
        loop {
            let step = restored.step_history().unwrap();
            if step == Step::Finished {
                break;
            }
            applied += 1;
            if applied == 1 {
                let live = b"1mred\x1b[0m and more";
                source.write(live);
                restored.write(live);
            }
        }
        assert!(applied > 1);
        assert_eq!(encoded(&restored), encoded(&source));
    }

    #[test]
    fn the_scrollback_is_a_line_limit_past_the_library_byte_cap() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        terminal.set_scrollback(5_000).unwrap();
        let history = numbered_lines(6_000);
        terminal.write(&history);
        // History is dropped a page at a time, so the limit is met to within a page.
        let kept = scrollback_rows(&terminal);
        assert!(kept >= 4_500, "kept {kept} lines");
    }

    #[test]
    fn clear_keeps_the_cursor_line_at_the_top_and_drops_the_scrollback() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        let history = numbered_lines(100);
        terminal.write(&history);
        terminal.write(b"one\r\ntwo\r\nprompt $ ");
        terminal.clear().unwrap();

        assert_eq!(scrollback_rows(&terminal), 0);
        let expected = Cursor {
            x: 9,
            y: 0,
            visible: true,
        };
        assert_eq!(terminal.cursor().unwrap(), expected);
        assert_eq!(plain_text(&terminal).trim_end(), "prompt $");
    }

    #[test]
    fn a_title_change_is_reported_once() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        assert_eq!(terminal.take_title().unwrap(), None);
        terminal.write(b"\x1b]2;claude: fixing the tests\x07");
        let title = terminal.take_title().unwrap();
        assert_eq!(title.as_deref(), Some("claude: fixing the tests"));
        assert_eq!(terminal.take_title().unwrap(), None);
    }

    #[test]
    fn color_queries_are_answered_from_the_theme_over_the_standard_palette() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        terminal.answer_queries().unwrap();
        let mut ansi = [Rgb { r: 0, g: 0, b: 0 }; 16];
        ansi[1] = Rgb {
            r: 0xf0,
            g: 0x64,
            b: 0x6c,
        };
        let colors = TerminalColors {
            foreground: Rgb {
                r: 0xd6,
                g: 0xd0,
                b: 0xc9,
            },
            background: Rgb {
                r: 0x19,
                g: 0x19,
                b: 0x18,
            },
            cursor: Rgb {
                r: 0xec,
                g: 0xe7,
                b: 0xe1,
            },
            cursor_text: Rgb {
                r: 0x19,
                g: 0x19,
                b: 0x18,
            },
            selection: Rgb {
                r: 0x3a,
                g: 0x38,
                b: 0x36,
            },
            ansi,
        };
        terminal.set_colors(&colors).unwrap();

        terminal.write(b"\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07\x1b]4;1;?\x07\x1b]4;200;?\x07");
        let reply_bytes = terminal.take_replies();
        let replies = String::from_utf8(reply_bytes).unwrap();
        assert!(replies.contains("10;rgb:d6d6/d0d0/c9c9"), "{replies:?}");
        assert!(replies.contains("11;rgb:1919/1919/1818"), "{replies:?}");
        assert!(replies.contains("12;rgb:ecec/e7e7/e1e1"), "{replies:?}");
        assert!(replies.contains("4;1;rgb:f0f0/6464/6c6c"), "{replies:?}");
        // Past the theme's sixteen the palette stays the standard cube.
        assert!(replies.contains("4;200;rgb:ffff/0000/d7d7"), "{replies:?}");
        assert!(terminal.take_replies().is_empty());
    }

    #[test]
    fn the_linked_library_is_the_pinned_release_build() {
        let optimize: sys::GhosttyOptimizeMode =
            build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_OPTIMIZE);
        assert_eq!(
            optimize,
            sys::GhosttyOptimizeMode_GHOSTTY_OPTIMIZE_RELEASE_FAST
        );

        let simd: bool = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_SIMD);
        assert!(simd);

        let major: usize = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_VERSION_MAJOR);
        let minor: usize = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_VERSION_MINOR);
        let patch: usize = build_info(sys::GhosttyBuildInfo_GHOSTTY_BUILD_INFO_VERSION_PATCH);
        assert_eq!((major, minor, patch), (1, 0, 2));
    }
}
