use std::ffi::c_void;
use std::fmt;
use std::ptr;

use taco_protocol::{Rgb, TerminalColors, TerminalSize};

use crate::restore::{Restore, Step};
use crate::sys;

/// The longest unfinished sequence a snapshot can carry: 65 MiB, the library's own limit for one.
const CONTINUATION_BYTES: usize = 65 * 1024 * 1024;
const PALETTE_SIZE: usize = 256;

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
        let terminal = Terminal {
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
        Ok(terminal)
    }

    pub fn as_raw(&self) -> sys::GhosttyTerminal {
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
        check(set, "set the reply callback")
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
        check(resized, "ghostty_terminal_resize")
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

    pub fn title(&self) -> Result<String, Error> {
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
