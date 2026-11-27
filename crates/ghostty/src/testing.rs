use std::ptr;

use crate::Terminal;
use crate::input::format_text;
use crate::sys;

/// The terminal's whole text, screen and scrollback, under the formatter's default options.
pub(crate) fn plain_text(terminal: &Terminal) -> String {
    let mut formatter: sys::GhosttyFormatter = ptr::null_mut();
    let made = unsafe {
        sys::ghostty_formatter_terminal_new(ptr::null(), &mut formatter, terminal.as_raw())
    };
    assert_eq!(made, sys::GhosttyResult_GHOSTTY_SUCCESS);
    let text = format_text(formatter, "format the terminal's text");
    unsafe { sys::ghostty_formatter_free(formatter) };
    text.unwrap()
}

/// `count` lines of colored output, each naming its number.
pub(crate) fn numbered_lines(count: usize) -> Vec<u8> {
    let mut lines = Vec::new();
    for n in 0..count {
        let color = 31 + n % 7;
        lines.extend_from_slice(
            format!("\x1b[{color}mline {n} of the earlier output\x1b[0m\r\n").as_bytes(),
        );
    }
    lines
}
