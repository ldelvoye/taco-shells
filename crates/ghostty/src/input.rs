//! What the person types, clicks, scrolls, selects and pastes, encoded the way the program in the terminal asked for.

use std::ffi::{c_char, c_void};
use std::mem;
use std::ptr;

use crate::sys;
use crate::terminal::{Error, Terminal, check};

const MODE_FOCUS_EVENTS: u16 = 1004;
const MODE_ALTERNATE_SCROLL: u16 = 1007;
const MODE_BRACKETED_PASTE: u16 = 2004;
/// Enough for any key or mouse sequence; a longer one is retried at the size the library asks for.
const SEQUENCE_BYTES: usize = 128;
/// What bracketed paste adds around the text: `ESC [200~` and `ESC [201~`.
const BRACKETS_BYTES: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A letter, digit or punctuation key, by the character it types without Shift.
    Char(char),
    Enter,
    Tab,
    Backspace,
    Escape,
    Space,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    /// F1 to F12.
    Function(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub super_key: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress<'a> {
    pub key: Key,
    pub mods: Mods,
    /// What the key types before Ctrl or Alt change it; empty for keys that type nothing.
    pub text: &'a str,
    pub repeat: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    Motion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
}

/// The drawn grid in pixels, for turning a pointer position into a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    pub width_px: u32,
    pub height_px: u32,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseInput {
    pub action: MouseAction,
    /// None for a motion with no button held.
    pub button: Option<MouseButton>,
    pub mods: Mods,
    /// Pixels from the grid's top-left corner.
    pub x: f32,
    pub y: f32,
    pub any_pressed: bool,
    pub grid: Grid,
}

/// A cell in the viewport, column and row from the top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// Positive scrolls toward the newest output.
    Lines(isize),
    Top,
    Bottom,
}

/// Turns key presses into what the program expects, under its current keyboard modes.
pub struct KeyEncoder {
    encoder: sys::GhosttyKeyEncoder,
    event: sys::GhosttyKeyEvent,
}

/// Turns pointer events into mouse reports, for a program that tracks the mouse.
pub struct MouseEncoder {
    encoder: sys::GhosttyMouseEncoder,
    event: sys::GhosttyMouseEvent,
}

impl KeyEncoder {
    pub fn new() -> Result<KeyEncoder, Error> {
        let mut encoder: sys::GhosttyKeyEncoder = ptr::null_mut();
        let created = unsafe { sys::ghostty_key_encoder_new(ptr::null(), &mut encoder) };
        check(created, "create a key encoder")?;
        let mut event: sys::GhosttyKeyEvent = ptr::null_mut();
        let created = unsafe { sys::ghostty_key_event_new(ptr::null(), &mut event) };
        if let Err(error) = check(created, "create a key event") {
            unsafe { sys::ghostty_key_encoder_free(encoder) };
            return Err(error);
        }
        Ok(KeyEncoder { encoder, event })
    }

    /// The bytes for `press`, empty when the key sends nothing.
    pub fn encode(&mut self, terminal: &Terminal, press: &KeyPress) -> Result<Vec<u8>, Error> {
        let from_terminal =
            unsafe { sys::ghostty_key_encoder_set_from_terminal(self.encoder, terminal.as_raw()) };
        check(from_terminal, "read the terminal's keyboard modes")?;
        // Option types its own characters (Option-e is é), as on every Mac; it is never Meta.
        let option_as_alt: sys::GhosttyOptionAsAlt =
            sys::GhosttyOptionAsAlt_GHOSTTY_OPTION_AS_ALT_FALSE;
        let pointer = &option_as_alt as *const sys::GhosttyOptionAsAlt as *const c_void;
        let set = unsafe {
            sys::ghostty_key_encoder_set(
                self.encoder,
                sys::GhosttyKeyEncoderOption_GHOSTTY_KEY_ENCODER_OPT_MACOS_OPTION_AS_ALT,
                pointer,
            )
        };
        check(set, "set Option as not Alt")?;

        let mut action = sys::GhosttyKeyAction_GHOSTTY_KEY_ACTION_PRESS;
        if press.repeat {
            action = sys::GhosttyKeyAction_GHOSTTY_KEY_ACTION_REPEAT;
        }
        let key = sys_key(press.key);
        let mods = sys_mods(press.mods);
        let unshifted = unshifted_of(press.key);
        // The event borrows the text rather than copying it; `press` outlives the encode below.
        let text = sys::GhosttyString {
            ptr: press.text.as_ptr(),
            len: press.text.len(),
        };
        unsafe {
            check(
                sys::ghostty_key_event_set_action(self.event, action),
                "set a key's action",
            )?;
            check(sys::ghostty_key_event_set_key(self.event, key), "set a key")?;
            check(
                sys::ghostty_key_event_set_mods(self.event, mods),
                "set a key's modifiers",
            )?;
            check(
                sys::ghostty_key_event_set_utf8(self.event, text),
                "set a key's text",
            )?;
            check(
                sys::ghostty_key_event_set_unshifted_codepoint(self.event, unshifted),
                "set a key's unshifted character",
            )?;
        }

        let encoder = self.encoder;
        let event = self.event;
        let encoded = encode_into(|buf, len, written| unsafe {
            sys::ghostty_key_encoder_encode(encoder, event, buf, len, written)
        });
        // Clear the borrowed text so the event never holds a pointer past this call.
        let cleared = sys::GhosttyString {
            ptr: ptr::null(),
            len: 0,
        };
        unsafe { sys::ghostty_key_event_set_utf8(self.event, cleared) };
        encoded.map_err(|code| Error {
            code,
            call: "encode a key",
        })
    }
}

impl Drop for KeyEncoder {
    fn drop(&mut self) {
        unsafe { sys::ghostty_key_event_free(self.event) };
        unsafe { sys::ghostty_key_encoder_free(self.encoder) };
    }
}

impl MouseEncoder {
    pub fn new() -> Result<MouseEncoder, Error> {
        let mut encoder: sys::GhosttyMouseEncoder = ptr::null_mut();
        let created = unsafe { sys::ghostty_mouse_encoder_new(ptr::null(), &mut encoder) };
        check(created, "create a mouse encoder")?;
        let mut event: sys::GhosttyMouseEvent = ptr::null_mut();
        let created = unsafe { sys::ghostty_mouse_event_new(ptr::null(), &mut event) };
        if let Err(error) = check(created, "create a mouse event") {
            unsafe { sys::ghostty_mouse_encoder_free(encoder) };
            return Err(error);
        }
        Ok(MouseEncoder { encoder, event })
    }

    /// The mouse report for `input`, empty when the program does not track this kind of event.
    pub fn encode(&mut self, terminal: &Terminal, input: &MouseInput) -> Result<Vec<u8>, Error> {
        let from_terminal = unsafe {
            sys::ghostty_mouse_encoder_set_from_terminal(self.encoder, terminal.as_raw())
        };
        check(from_terminal, "read the terminal's mouse modes")?;

        let mut size: sys::GhosttyMouseEncoderSize = unsafe { mem::zeroed() };
        // A sized struct: the library reads `size` to know which version of it it is given.
        size.size = mem::size_of::<sys::GhosttyMouseEncoderSize>();
        size.screen_width = input.grid.width_px;
        size.screen_height = input.grid.height_px;
        size.cell_width = input.grid.cell_width_px.max(1);
        size.cell_height = input.grid.cell_height_px.max(1);
        self.set(
            sys::GhosttyMouseEncoderOption_GHOSTTY_MOUSE_ENCODER_OPT_SIZE,
            &size,
        )?;
        self.set(
            sys::GhosttyMouseEncoderOption_GHOSTTY_MOUSE_ENCODER_OPT_ANY_BUTTON_PRESSED,
            &input.any_pressed,
        )?;
        // Motion inside the cell already reported sends nothing.
        let track_last_cell = true;
        self.set(
            sys::GhosttyMouseEncoderOption_GHOSTTY_MOUSE_ENCODER_OPT_TRACK_LAST_CELL,
            &track_last_cell,
        )?;

        let action = match input.action {
            MouseAction::Press => sys::GhosttyMouseAction_GHOSTTY_MOUSE_ACTION_PRESS,
            MouseAction::Release => sys::GhosttyMouseAction_GHOSTTY_MOUSE_ACTION_RELEASE,
            MouseAction::Motion => sys::GhosttyMouseAction_GHOSTTY_MOUSE_ACTION_MOTION,
        };
        let position = sys::GhosttyMousePosition {
            x: input.x,
            y: input.y,
        };
        let mods = sys_mods(input.mods);
        unsafe {
            check(
                sys::ghostty_mouse_event_set_action(self.event, action),
                "set a mouse action",
            )?;
            check(
                sys::ghostty_mouse_event_set_position(self.event, position),
                "set a mouse position",
            )?;
            check(
                sys::ghostty_mouse_event_set_mods(self.event, mods),
                "set a mouse event's modifiers",
            )?;
        }
        match input.button {
            Some(button) => {
                let raw = sys_button(button);
                let set = unsafe { sys::ghostty_mouse_event_set_button(self.event, raw) };
                check(set, "set a mouse button")?;
            }
            None => {
                let cleared = unsafe { sys::ghostty_mouse_event_clear_button(self.event) };
                check(cleared, "clear a mouse button")?;
            }
        }

        let encoder = self.encoder;
        let event = self.event;
        let encoded = encode_into(|buf, len, written| unsafe {
            sys::ghostty_mouse_encoder_encode(encoder, event, buf, len, written)
        });
        encoded.map_err(|code| Error {
            code,
            call: "encode a mouse event",
        })
    }

    fn set<T>(&mut self, option: sys::GhosttyMouseEncoderOption, value: &T) -> Result<(), Error> {
        let pointer = value as *const T as *const c_void;
        let set = unsafe { sys::ghostty_mouse_encoder_set(self.encoder, option, pointer) };
        check(set, "set a mouse encoder option")
    }
}

impl Drop for MouseEncoder {
    fn drop(&mut self) {
        unsafe { sys::ghostty_mouse_event_free(self.event) };
        unsafe { sys::ghostty_mouse_encoder_free(self.encoder) };
    }
}

impl Terminal {
    /// The report for focus gained or lost, when the program asked for them (mode 1004).
    pub fn focus_report(&self, focused: bool) -> Result<Option<Vec<u8>>, Error> {
        let wanted = self.dec_mode(MODE_FOCUS_EVENTS)?;
        if !wanted {
            return Ok(None);
        }
        let mut event = sys::GhosttyFocusEvent_GHOSTTY_FOCUS_LOST;
        if focused {
            event = sys::GhosttyFocusEvent_GHOSTTY_FOCUS_GAINED;
        }
        let encoded = encode_into(|buf, len, written| unsafe {
            sys::ghostty_focus_encode(event, buf, len, written)
        });
        let bytes = encoded.map_err(|code| Error {
            code,
            call: "encode a focus report",
        })?;
        Ok(Some(bytes))
    }

    /// `text` as a paste: bracketed when the program asked for it (mode 2004), so a multi-line paste is not run line by line.
    pub fn paste_bytes(&self, text: &str) -> Result<Vec<u8>, Error> {
        let bracketed = self.dec_mode(MODE_BRACKETED_PASTE)?;
        // The library may rewrite the text in place, so it gets a copy.
        let mut data = text.as_bytes().to_vec();
        let data_pointer = data.as_mut_ptr() as *mut c_char;
        let data_len = data.len();
        let mut out = vec![0u8; data_len + BRACKETS_BYTES];
        let mut written = 0usize;
        let mut encoded = unsafe {
            sys::ghostty_paste_encode(
                data_pointer,
                data_len,
                bracketed,
                out.as_mut_ptr(),
                out.len(),
                &mut written,
            )
        };
        if encoded == sys::GhosttyResult_GHOSTTY_OUT_OF_SPACE {
            out.resize(written, 0);
            encoded = unsafe {
                sys::ghostty_paste_encode(
                    data_pointer,
                    data_len,
                    bracketed,
                    out.as_mut_ptr(),
                    out.len(),
                    &mut written,
                )
            };
        }
        check(encoded, "encode a paste")?;
        out.truncate(written);
        Ok(out)
    }

    /// Whether the program tracks the mouse, so clicks and the wheel go to it instead of selecting and scrolling.
    pub fn mouse_tracking(&self) -> Result<bool, Error> {
        let mut tracking = false;
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING,
            &mut tracking,
        )?;
        Ok(tracking)
    }

    /// Whether the wheel should send arrow keys: the alternate screen is up and the program asked for it (mode 1007).
    pub fn wheel_sends_arrows(&self) -> Result<bool, Error> {
        let mut screen: sys::GhosttyTerminalScreen = 0;
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN,
            &mut screen,
        )?;
        let alternate = screen == sys::GhosttyTerminalScreen_GHOSTTY_TERMINAL_SCREEN_ALTERNATE;
        if !alternate {
            return Ok(false);
        }
        self.dec_mode(MODE_ALTERNATE_SCROLL)
    }

    pub fn scroll(&mut self, scroll: Scroll) -> Result<(), Error> {
        let mut behavior: sys::GhosttyTerminalScrollViewport = unsafe { mem::zeroed() };
        match scroll {
            Scroll::Lines(delta) => {
                behavior.tag =
                    sys::GhosttyTerminalScrollViewportTag_GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA;
                behavior.value.delta = delta;
            }
            Scroll::Top => {
                behavior.tag =
                    sys::GhosttyTerminalScrollViewportTag_GHOSTTY_TERMINAL_SCROLL_VIEWPORT_TOP;
            }
            Scroll::Bottom => {
                behavior.tag =
                    sys::GhosttyTerminalScrollViewportTag_GHOSTTY_TERMINAL_SCROLL_VIEWPORT_BOTTOM;
            }
        }
        let scrolled = unsafe { sys::ghostty_terminal_scroll_viewport(self.as_raw(), behavior) };
        check(scrolled, "scroll the viewport")
    }

    /// Selects from one cell to another, both included, in the order the text flows.
    pub fn select(&mut self, from: CellPoint, to: CellPoint) -> Result<(), Error> {
        let start = self.grid_ref(from)?;
        let end = self.grid_ref(to)?;
        let selection = sys::GhosttySelection {
            start,
            end,
            rectangle: false,
        };
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SELECTION,
            &selection,
        )
    }

    /// Selects the word under `at`, as a double click does.
    pub fn select_word(&mut self, at: CellPoint) -> Result<(), Error> {
        let mut options: sys::GhosttyTerminalSelectWordOptions = unsafe { mem::zeroed() };
        options.size = mem::size_of::<sys::GhosttyTerminalSelectWordOptions>();
        options.ref_ = self.grid_ref(at)?;
        let mut selection: sys::GhosttySelection = unsafe { mem::zeroed() };
        let selected =
            unsafe { sys::ghostty_terminal_select_word(self.as_raw(), &options, &mut selection) };
        if selected == sys::GhosttyResult_GHOSTTY_NO_VALUE {
            return self.clear_selection();
        }
        check(selected, "select a word")?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SELECTION,
            &selection,
        )
    }

    /// Selects the line under `at`, as a triple click does.
    pub fn select_line(&mut self, at: CellPoint) -> Result<(), Error> {
        let mut options: sys::GhosttyTerminalSelectLineOptions = unsafe { mem::zeroed() };
        options.size = mem::size_of::<sys::GhosttyTerminalSelectLineOptions>();
        options.ref_ = self.grid_ref(at)?;
        let mut selection: sys::GhosttySelection = unsafe { mem::zeroed() };
        let selected =
            unsafe { sys::ghostty_terminal_select_line(self.as_raw(), &options, &mut selection) };
        if selected == sys::GhosttyResult_GHOSTTY_NO_VALUE {
            return self.clear_selection();
        }
        check(selected, "select a line")?;
        self.set(
            sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SELECTION,
            &selection,
        )
    }

    pub fn clear_selection(&mut self) -> Result<(), Error> {
        let cleared = unsafe {
            sys::ghostty_terminal_set(
                self.as_raw(),
                sys::GhosttyTerminalOption_GHOSTTY_TERMINAL_OPT_SELECTION,
                ptr::null(),
            )
        };
        check(cleared, "clear the selection")
    }

    /// The selected text as it is copied: lines a program wrote long are joined again and trailing blanks dropped.
    pub fn selection_text(&self) -> Result<Option<String>, Error> {
        let mut selection: sys::GhosttySelection = unsafe { mem::zeroed() };
        let pointer = &mut selection as *mut sys::GhosttySelection as *mut c_void;
        let read = unsafe {
            sys::ghostty_terminal_get(
                self.as_raw(),
                sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_SELECTION,
                pointer,
            )
        };
        if read == sys::GhosttyResult_GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        check(read, "read the selection")?;

        let mut formatter: sys::GhosttyFormatter = ptr::null_mut();
        let made = unsafe {
            sys::ghostty_formatter_terminal_new(ptr::null(), &mut formatter, self.as_raw())
        };
        check(made, "create a formatter")?;
        let text = format_selection(formatter, &selection);
        unsafe { sys::ghostty_formatter_free(formatter) };
        text.map(Some)
    }

    /// The hyperlink a program attached to the cell at `at` (OSC 8), if any.
    pub fn hyperlink_at(&self, at: CellPoint) -> Result<Option<String>, Error> {
        let grid_ref = self.grid_ref(at)?;
        let mut needed = 0usize;
        let sized = unsafe {
            sys::ghostty_grid_ref_hyperlink_uri(&grid_ref, ptr::null_mut(), 0, &mut needed)
        };
        if sized == sys::GhosttyResult_GHOSTTY_SUCCESS {
            return Ok(None);
        }
        if sized != sys::GhosttyResult_GHOSTTY_OUT_OF_SPACE {
            check(sized, "measure a hyperlink")?;
        }
        let mut uri = vec![0u8; needed];
        let mut written = 0usize;
        let read = unsafe {
            sys::ghostty_grid_ref_hyperlink_uri(
                &grid_ref,
                uri.as_mut_ptr(),
                uri.len(),
                &mut written,
            )
        };
        check(read, "read a hyperlink")?;
        uri.truncate(written);
        let text = String::from_utf8_lossy(&uri);
        Ok(Some(text.into_owned()))
    }

    fn dec_mode(&self, number: u16) -> Result<bool, Error> {
        // A DEC private mode is the number with the top bit clear; ANSI modes set it.
        let mut config = sys::GhosttyTerminalModeConfig {
            mode: number & 0x7fff,
            value: false,
        };
        self.get(
            sys::GhosttyTerminalData_GHOSTTY_TERMINAL_DATA_MODE,
            &mut config,
        )?;
        Ok(config.value)
    }

    fn grid_ref(&self, at: CellPoint) -> Result<sys::GhosttyGridRef, Error> {
        let mut point: sys::GhosttyPoint = unsafe { mem::zeroed() };
        point.tag = sys::GhosttyPointTag_GHOSTTY_POINT_TAG_VIEWPORT;
        point.value.coordinate = sys::GhosttyPointCoordinate {
            x: at.x,
            y: u32::from(at.y),
        };
        let mut grid_ref: sys::GhosttyGridRef = unsafe { mem::zeroed() };
        let found = unsafe { sys::ghostty_terminal_grid_ref(self.as_raw(), point, &mut grid_ref) };
        check(found, "find a cell")?;
        Ok(grid_ref)
    }
}

fn format_selection(
    formatter: sys::GhosttyFormatter,
    selection: &sys::GhosttySelection,
) -> Result<String, Error> {
    let unwrap = true;
    let trim = true;
    set_formatter(
        formatter,
        sys::GhosttyFormatterOption_GHOSTTY_FORMATTER_OPT_UNWRAP,
        &unwrap,
    )?;
    set_formatter(
        formatter,
        sys::GhosttyFormatterOption_GHOSTTY_FORMATTER_OPT_TRIM,
        &trim,
    )?;
    set_formatter(
        formatter,
        sys::GhosttyFormatterOption_GHOSTTY_FORMATTER_OPT_SELECTION,
        selection,
    )?;

    let mut bytes: *mut u8 = ptr::null_mut();
    let mut len = 0usize;
    let formatted = unsafe {
        sys::ghostty_formatter_format_alloc(formatter, ptr::null(), &mut bytes, &mut len)
    };
    check(formatted, "format the selection")?;
    let slice = unsafe { std::slice::from_raw_parts(bytes, len) };
    let text = String::from_utf8_lossy(slice).into_owned();
    unsafe { sys::ghostty_free(ptr::null(), bytes, len) };
    Ok(text)
}

fn set_formatter<T>(
    formatter: sys::GhosttyFormatter,
    option: sys::GhosttyFormatterOption,
    value: &T,
) -> Result<(), Error> {
    let pointer = value as *const T as *const c_void;
    let set = unsafe { sys::ghostty_formatter_set(formatter, option, pointer) };
    check(set, "set a formatter option")
}

/// Runs an encoder that writes into a caller's buffer, growing the buffer once if the encoder says it is too small.
fn encode_into(
    mut encode: impl FnMut(*mut u8, usize, *mut usize) -> sys::GhosttyResult,
) -> Result<Vec<u8>, sys::GhosttyResult> {
    let mut out = vec![0u8; SEQUENCE_BYTES];
    let mut written = 0usize;
    let mut result = encode(out.as_mut_ptr(), out.len(), &mut written);
    if result == sys::GhosttyResult_GHOSTTY_OUT_OF_SPACE {
        out.resize(written, 0);
        result = encode(out.as_mut_ptr(), out.len(), &mut written);
    }
    if result != sys::GhosttyResult_GHOSTTY_SUCCESS {
        return Err(result);
    }
    out.truncate(written);
    Ok(out)
}

fn sys_mods(mods: Mods) -> sys::GhosttyMods {
    let mut bits: sys::GhosttyMods = 0;
    if mods.shift {
        bits |= sys::GHOSTTY_MODS_SHIFT as sys::GhosttyMods;
    }
    if mods.ctrl {
        bits |= sys::GHOSTTY_MODS_CTRL as sys::GhosttyMods;
    }
    if mods.alt {
        bits |= sys::GHOSTTY_MODS_ALT as sys::GhosttyMods;
    }
    if mods.super_key {
        bits |= sys::GHOSTTY_MODS_SUPER as sys::GhosttyMods;
    }
    bits
}

fn sys_button(button: MouseButton) -> sys::GhosttyMouseButton {
    match button {
        MouseButton::Left => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_LEFT,
        MouseButton::Middle => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_MIDDLE,
        MouseButton::Right => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_RIGHT,
        MouseButton::WheelUp => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_FOUR,
        MouseButton::WheelDown => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_FIVE,
        MouseButton::WheelLeft => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_SIX,
        MouseButton::WheelRight => sys::GhosttyMouseButton_GHOSTTY_MOUSE_BUTTON_SEVEN,
    }
}

/// The character a key types without Shift, which the Kitty keyboard protocol reports; 0 for keys that type none.
fn unshifted_of(key: Key) -> u32 {
    match key {
        Key::Char(character) => u32::from(character),
        Key::Space => u32::from(' '),
        _ => 0,
    }
}

fn sys_key(key: Key) -> sys::GhosttyKey {
    match key {
        Key::Char(character) => char_key(character),
        Key::Enter => sys::GhosttyKey_GHOSTTY_KEY_ENTER,
        Key::Tab => sys::GhosttyKey_GHOSTTY_KEY_TAB,
        Key::Backspace => sys::GhosttyKey_GHOSTTY_KEY_BACKSPACE,
        Key::Escape => sys::GhosttyKey_GHOSTTY_KEY_ESCAPE,
        Key::Space => sys::GhosttyKey_GHOSTTY_KEY_SPACE,
        Key::Delete => sys::GhosttyKey_GHOSTTY_KEY_DELETE,
        Key::Insert => sys::GhosttyKey_GHOSTTY_KEY_INSERT,
        Key::Home => sys::GhosttyKey_GHOSTTY_KEY_HOME,
        Key::End => sys::GhosttyKey_GHOSTTY_KEY_END,
        Key::PageUp => sys::GhosttyKey_GHOSTTY_KEY_PAGE_UP,
        Key::PageDown => sys::GhosttyKey_GHOSTTY_KEY_PAGE_DOWN,
        Key::Up => sys::GhosttyKey_GHOSTTY_KEY_ARROW_UP,
        Key::Down => sys::GhosttyKey_GHOSTTY_KEY_ARROW_DOWN,
        Key::Left => sys::GhosttyKey_GHOSTTY_KEY_ARROW_LEFT,
        Key::Right => sys::GhosttyKey_GHOSTTY_KEY_ARROW_RIGHT,
        Key::Function(number) => function_key(number),
    }
}

fn function_key(number: u8) -> sys::GhosttyKey {
    match number {
        1 => sys::GhosttyKey_GHOSTTY_KEY_F1,
        2 => sys::GhosttyKey_GHOSTTY_KEY_F2,
        3 => sys::GhosttyKey_GHOSTTY_KEY_F3,
        4 => sys::GhosttyKey_GHOSTTY_KEY_F4,
        5 => sys::GhosttyKey_GHOSTTY_KEY_F5,
        6 => sys::GhosttyKey_GHOSTTY_KEY_F6,
        7 => sys::GhosttyKey_GHOSTTY_KEY_F7,
        8 => sys::GhosttyKey_GHOSTTY_KEY_F8,
        9 => sys::GhosttyKey_GHOSTTY_KEY_F9,
        10 => sys::GhosttyKey_GHOSTTY_KEY_F10,
        11 => sys::GhosttyKey_GHOSTTY_KEY_F11,
        12 => sys::GhosttyKey_GHOSTTY_KEY_F12,
        _ => sys::GhosttyKey_GHOSTTY_KEY_UNIDENTIFIED,
    }
}

/// The physical key for a character on a US layout. Anything else is unidentified, and the encoder goes by its text.
fn char_key(character: char) -> sys::GhosttyKey {
    let lower = character.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        let offset = u32::from(lower) - u32::from('a');
        return sys::GhosttyKey_GHOSTTY_KEY_A + offset;
    }
    if lower.is_ascii_digit() {
        let offset = u32::from(lower) - u32::from('0');
        return sys::GhosttyKey_GHOSTTY_KEY_DIGIT_0 + offset;
    }
    match lower {
        '`' => sys::GhosttyKey_GHOSTTY_KEY_BACKQUOTE,
        '\\' => sys::GhosttyKey_GHOSTTY_KEY_BACKSLASH,
        '[' => sys::GhosttyKey_GHOSTTY_KEY_BRACKET_LEFT,
        ']' => sys::GhosttyKey_GHOSTTY_KEY_BRACKET_RIGHT,
        ',' => sys::GhosttyKey_GHOSTTY_KEY_COMMA,
        '=' => sys::GhosttyKey_GHOSTTY_KEY_EQUAL,
        '-' => sys::GhosttyKey_GHOSTTY_KEY_MINUS,
        '.' => sys::GhosttyKey_GHOSTTY_KEY_PERIOD,
        '\'' => sys::GhosttyKey_GHOSTTY_KEY_QUOTE,
        ';' => sys::GhosttyKey_GHOSTTY_KEY_SEMICOLON,
        '/' => sys::GhosttyKey_GHOSTTY_KEY_SLASH,
        _ => sys::GhosttyKey_GHOSTTY_KEY_UNIDENTIFIED,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{RenderState, Row};
    use taco_protocol::TerminalSize;

    const SIZE: TerminalSize = TerminalSize { cols: 20, rows: 4 };
    const GRID: Grid = Grid {
        width_px: 200,
        height_px: 80,
        cell_width_px: 10,
        cell_height_px: 20,
    };

    fn press(key: Key, mods: Mods, text: &str) -> KeyPress<'_> {
        KeyPress {
            key,
            mods,
            text,
            repeat: false,
        }
    }

    fn first_row(terminal: &Terminal) -> Row {
        let mut state = RenderState::new().unwrap();
        let mut frame = state.update(terminal).unwrap();
        let mut row = Row::new();
        frame.next_row(&mut row, false).unwrap();
        row
    }

    #[test]
    fn keys_follow_our_choices_and_the_programs_modes() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        let mut keys = KeyEncoder::new().unwrap();
        let ctrl = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };

        // Option types its own character rather than sending Esc first.
        let option_e = keys
            .encode(&terminal, &press(Key::Char('e'), alt, "é"))
            .unwrap();
        assert_eq!(option_e, "é".as_bytes());
        let ctrl_c = keys
            .encode(&terminal, &press(Key::Char('c'), ctrl, "c"))
            .unwrap();
        assert_eq!(ctrl_c, b"\x03");
        let enter = keys
            .encode(&terminal, &press(Key::Enter, Mods::default(), ""))
            .unwrap();
        assert_eq!(enter, b"\r");

        // Arrows follow the program's cursor-key mode.
        let up = press(Key::Up, Mods::default(), "");
        assert_eq!(keys.encode(&terminal, &up).unwrap(), b"\x1b[A");
        terminal.write(b"\x1b[?1h");
        assert_eq!(keys.encode(&terminal, &up).unwrap(), b"\x1bOA");
    }

    #[test]
    fn paste_focus_and_the_wheel_follow_what_the_program_asked_for() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        let plain = terminal.paste_bytes("one\ntwo").unwrap();
        assert!(!plain.starts_with(b"\x1b[200~"));
        assert_eq!(terminal.focus_report(true).unwrap(), None);
        assert!(!terminal.wheel_sends_arrows().unwrap());

        terminal.write(b"\x1b[?2004h\x1b[?1004h\x1b[?1049h\x1b[?1007h");
        let bracketed = terminal.paste_bytes("one\ntwo").unwrap();
        assert!(bracketed.starts_with(b"\x1b[200~") && bracketed.ends_with(b"\x1b[201~"));
        assert_eq!(
            terminal.focus_report(true).unwrap(),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            terminal.focus_report(false).unwrap(),
            Some(b"\x1b[O".to_vec())
        );
        assert!(terminal.wheel_sends_arrows().unwrap());
    }

    #[test]
    fn a_click_is_reported_at_its_cell_only_to_a_program_tracking_the_mouse() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        let mut mouse = MouseEncoder::new().unwrap();
        // The third column of the second row.
        let click = MouseInput {
            action: MouseAction::Press,
            button: Some(MouseButton::Left),
            mods: Mods::default(),
            x: 25.0,
            y: 30.0,
            any_pressed: true,
            grid: GRID,
        };
        assert!(!terminal.mouse_tracking().unwrap());
        assert!(mouse.encode(&terminal, &click).unwrap().is_empty());

        terminal.write(b"\x1b[?1000h\x1b[?1006h");
        assert!(terminal.mouse_tracking().unwrap());
        assert_eq!(mouse.encode(&terminal, &click).unwrap(), b"\x1b[<0;3;2M");
    }

    #[test]
    fn a_selection_is_drawn_copied_and_cleared() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        terminal.write(b"hello world");

        let start = CellPoint { x: 1, y: 0 };
        let end = CellPoint { x: 4, y: 0 };
        terminal.select(start, end).unwrap();
        assert_eq!(terminal.selection_text().unwrap().as_deref(), Some("ello"));
        let row = first_row(&terminal);
        let selected: Vec<usize> = row
            .cells
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.selected)
            .map(|(column, _)| column)
            .collect();
        assert_eq!(selected, vec![1, 2, 3, 4]);

        terminal.select_word(CellPoint { x: 8, y: 0 }).unwrap();
        assert_eq!(terminal.selection_text().unwrap().as_deref(), Some("world"));

        terminal.clear_selection().unwrap();
        assert_eq!(terminal.selection_text().unwrap(), None);
    }

    #[test]
    fn a_hyperlink_is_found_under_its_cell() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        terminal.write(b"a\x1b]8;;https://example.com/page\x07link\x1b]8;;\x07");
        let link = terminal.hyperlink_at(CellPoint { x: 2, y: 0 }).unwrap();
        assert_eq!(link.as_deref(), Some("https://example.com/page"));
        assert_eq!(
            terminal.hyperlink_at(CellPoint { x: 0, y: 0 }).unwrap(),
            None
        );
    }

    #[test]
    fn scrolling_back_shows_older_lines() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        for line in 0..100 {
            terminal.write(format!("line {line}\r\n").as_bytes());
        }
        terminal.scroll(Scroll::Lines(-10)).unwrap();
        assert_eq!(first_row(&terminal).text, "line 87");
        terminal.scroll(Scroll::Bottom).unwrap();
        assert_eq!(first_row(&terminal).text, "line 97");
    }
}
