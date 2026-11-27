use std::ffi::c_void;
use std::fmt;
use std::ptr;

use taco_protocol::{CursorShape, Rgb, TerminalColors, TerminalSize};

use crate::restore::{Restore, Step};
use crate::sys;

/// The longest unfinished sequence a snapshot can carry: 65 MiB, the library's own limit for one.
const CONTINUATION_BYTES: usize = 65 * 1024 * 1024;
pub(crate) const PALETTE_SIZE: usize = 256;

/// A call into the library that did not succeed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    pub code: sys::GhosttyResult,
    pub call: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub visible: bool,
}

/// What the library's callbacks leave for the owner to collect after a write.
#[derive(Default)]
struct Effects {
    replies: Vec<u8>,
    title_changed: bool,
    /// What a program's size reports (CSI 14, 16 and 18 t) answer: the library asks for them rather than keeping them.
    rows: u16,
    columns: u16,
    cell_width_px: u32,
    cell_height_px: u32,
}

/// One terminal's screen, scrollback and modes. Silent until `answer_queries` is called.
pub struct Terminal {
    raw: sys::GhosttyTerminal,
    /// Owned, from `Box::into_raw`. Reached only through this pointer, by us and by the library's callbacks alike.
    effects: *mut Effects,
    /// For a restored terminal, the history still to apply. Its decoder borrows `raw`, so it is dropped first.
    history: Option<Restore>,
}

// The library's terminal has no thread affinity; `&mut` keeps it to one thread at a time.
unsafe impl Send for Terminal {}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} failed with libghostty-vt result {}",
            self.call, self.code
        )
    }
}

impl std::error::Error for Error {}

pub(crate) fn check(code: sys::GhosttyResult, call: &'static str) -> Result<(), Error> {
    if code == sys::GhosttyResult_GHOSTTY_SUCCESS {
        return Ok(());
    }
    Err(Error { code, call })
}

impl Terminal {
    pub fn new(size: TerminalSize) -> Result<Terminal, Error> {
        let mut raw: sys::GhosttyTerminal = ptr::null_mut();
        let created =
            unsafe { sys::ghostty_terminal_new(ptr::null(), &mut raw, size.cols, size.rows) };
        check(created, "ghostty_terminal_new")?;
        let terminal = Terminal::adopt(raw, None)?;
        // Output can stop inside a sequence; without tracking, a snapshot taken there would fail.
        terminal.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_CONTINUATION_MAX_BYTES,
            &CONTINUATION_BYTES,
        )?;
        Ok(terminal)
    }

    /// A terminal drawable from a snapshot's screen part, with its history to follow through `push_history`.
    pub fn restore(screen: &[u8]) -> Result<Terminal, Error> {
        let (history, raw) = Restore::begin(screen)?;
        Terminal::adopt(raw, Some(history))
    }

    fn adopt(raw: sys::GhosttyTerminal, history: Option<Restore>) -> Result<Terminal, Error> {
        let boxed = Box::new(Effects::default());
        let effects = Box::into_raw(boxed);
        let mut terminal = Terminal {
            raw,
            effects,
            history,
        };

        let userdata = effects as *const c_void;
        let set = unsafe {
            sys::ghostty_terminal_set(
                raw,
                sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_USERDATA,
                userdata,
            )
        };
        check(set, "set userdata")?;
        let title_callback = on_title_changed as *const c_void;
        let set = unsafe {
            sys::ghostty_terminal_set(
                raw,
                sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
                title_callback,
            )
        };
        check(set, "set the title callback")?;

        let mut columns: u16 = 0;
        let mut rows: u16 = 0;
        terminal.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_COLS,
            &mut columns,
        )?;
        terminal.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_ROWS,
            &mut rows,
        )?;
        let effects = terminal.effects();
        effects.columns = columns;
        effects.rows = rows;
        Ok(terminal)
    }

    pub(crate) fn as_raw(&self) -> sys::GhosttyTerminal {
        self.raw
    }

    /// From now on, what the terminal answers a program's questions is kept for `take_replies`. Until then it is dropped.
    pub fn answer_queries(&mut self) -> Result<(), Error> {
        let reply_callback = on_write_pty as *const c_void;
        let set = unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_WRITE_PTY,
                reply_callback,
            )
        };
        check(set, "set the reply callback")?;
        let size_callback = on_size as *const c_void;
        let set = unsafe {
            sys::ghostty_terminal_set(
                self.raw,
                sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SIZE,
                size_callback,
            )
        };
        check(set, "set the size callback")
    }

    pub fn write(&mut self, data: &[u8]) {
        unsafe { sys::ghostty_terminal_vt_write(self.raw, data.as_ptr(), data.len()) };
    }

    /// A history part of a restored terminal's snapshot, in order; `last` marks the final one. Ignored once it has come.
    pub fn push_history(&mut self, part: &[u8], last: bool) {
        if let Some(history) = &mut self.history {
            history.push(part, last);
        }
    }

    /// Applies at most one history page, once every part is in. Live output may be written between steps.
    pub fn step_history(&mut self) -> Result<Step, Error> {
        let Some(history) = &mut self.history else {
            return Ok(Step::Finished);
        };
        let step = history.step();
        let done = !matches!(step, Ok(Step::Waiting) | Ok(Step::Applied));
        if done {
            self.history = None;
        }
        step
    }

    pub fn resize(
        &mut self,
        size: TerminalSize,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> Result<(), Error> {
        let resized = unsafe {
            sys::ghostty_terminal_resize(
                self.raw,
                size.cols,
                size.rows,
                cell_width_px,
                cell_height_px,
            )
        };
        check(resized, "ghostty_terminal_resize")?;
        let effects = self.effects();
        effects.rows = size.rows;
        effects.columns = size.cols;
        effects.cell_width_px = cell_width_px;
        effects.cell_height_px = cell_height_px;
        Ok(())
    }

    /// A limit in lines. The library's default byte limit is lifted: left in place it drops history long before the line limit.
    pub fn set_scrollback(&mut self, lines: usize) -> Result<(), Error> {
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES,
            &usize::MAX,
        )?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES,
            &lines,
        )
    }

    /// The cursor shown while no program has chosen one, or after it gives its choice back; a shape a program chose stays. A snapshot carries them.
    pub fn set_cursor_defaults(&mut self, shape: CursorShape, blink: bool) -> Result<(), Error> {
        let style = match shape {
            CursorShape::Block => sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_BLOCK,
            CursorShape::Bar => sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_BAR,
            CursorShape::Underline => sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_UNDERLINE,
        };
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_STYLE,
            &style,
        )?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_DEFAULT_CURSOR_BLINK,
            &blink,
        )
    }

    /// The default colors programs' color queries are answered with. The theme's sixteen replace the first sixteen of the library's 256.
    pub fn set_colors(&mut self, colors: &TerminalColors) -> Result<(), Error> {
        let mut palette = [sys::GhosttyColorRgb { r: 0, g: 0, b: 0 }; PALETTE_SIZE];
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_COLOR_PALETTE_DEFAULT,
            &mut palette,
        )?;
        for (index, color) in colors.ansi.iter().enumerate() {
            palette[index] = rgb(*color);
        }

        let foreground = rgb(colors.foreground);
        let background = rgb(colors.background);
        let cursor = rgb(colors.cursor);
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND,
            &foreground,
        )?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND,
            &background,
        )?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_COLOR_CURSOR,
            &cursor,
        )?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_COLOR_PALETTE,
            &palette,
        )
    }

    /// The cursor's line becomes the first and the scrollback is dropped. The runtime and the view each apply it at the same point in the output, so they stay alike.
    pub fn clear(&mut self) -> Result<(), Error> {
        let row = self.cursor()?.y;
        let mut sequence = String::new();
        if row > 0 {
            let scroll_and_follow = format!("\x1b[{row}S\x1b[{row}A");
            sequence.push_str(&scroll_and_follow);
        }
        sequence.push_str("\x1b[3J");
        self.write(sequence.as_bytes());
        Ok(())
    }

    /// What the terminal has answered since the last call, for the shell.
    pub fn take_replies(&mut self) -> Vec<u8> {
        let effects = self.effects();
        std::mem::take(&mut effects.replies)
    }

    /// The title, when a program has set one since the last call.
    pub fn take_title(&mut self) -> Result<Option<String>, Error> {
        let effects = self.effects();
        let changed = std::mem::take(&mut effects.title_changed);
        if !changed {
            return Ok(None);
        }
        let title = self.title()?;
        Ok(Some(title))
    }

    fn title(&self) -> Result<String, Error> {
        let mut title = sys::GhosttyString {
            ptr: ptr::null(),
            len: 0,
        };
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_TITLE,
            &mut title,
        )?;
        if title.ptr.is_null() {
            return Ok(String::new());
        }
        let bytes = unsafe { std::slice::from_raw_parts(title.ptr, title.len) };
        let text = String::from_utf8_lossy(bytes);
        Ok(text.into_owned())
    }

    pub fn cursor(&self) -> Result<Cursor, Error> {
        let mut cursor = Cursor {
            x: 0,
            y: 0,
            visible: true,
        };
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_CURSOR_X,
            &mut cursor.x,
        )?;
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_CURSOR_Y,
            &mut cursor.y,
        )?;
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE,
            &mut cursor.visible,
        )?;
        Ok(cursor)
    }

    pub(crate) fn set<T>(
        &self,
        option: sys::GhosttyTerminalOption,
        value: &T,
    ) -> Result<(), Error> {
        let pointer = value as *const T as *const c_void;
        let set = unsafe { sys::ghostty_terminal_set(self.raw, option, pointer) };
        check(set, "ghostty_terminal_set")
    }

    pub(crate) fn get<T>(&self, data: sys::GhosttyTerminalData, out: &mut T) -> Result<(), Error> {
        let pointer = out as *mut T as *mut c_void;
        let read = unsafe { sys::ghostty_terminal_get(self.raw, data, pointer) };
        check(read, "ghostty_terminal_get")
    }

    fn effects(&mut self) -> &mut Effects {
        unsafe { &mut *self.effects }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.history = None;
        unsafe { sys::ghostty_terminal_free(self.raw) };
        let effects = unsafe { Box::from_raw(self.effects) };
        drop(effects);
    }
}

fn rgb(color: Rgb) -> sys::GhosttyColorRgb {
    sys::GhosttyColorRgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

unsafe extern "C" fn on_write_pty(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    let effects = unsafe { &mut *(userdata as *mut Effects) };
    let reply = unsafe { std::slice::from_raw_parts(data, len) };
    effects.replies.extend_from_slice(reply);
}

unsafe extern "C" fn on_title_changed(_terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    let effects = unsafe { &mut *(userdata as *mut Effects) };
    effects.title_changed = true;
}

unsafe extern "C" fn on_size(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    out_size: *mut sys::GhosttySizeReportSize,
) -> bool {
    let effects = unsafe { &*(userdata as *const Effects) };
    let size = unsafe { &mut *out_size };
    size.rows = effects.rows;
    size.columns = effects.columns;
    size.cell_width = effects.cell_width_px;
    size.cell_height = effects.cell_height_px;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{numbered_lines, plain_text};

    const SIZE: TerminalSize = TerminalSize { cols: 80, rows: 24 };

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
}
