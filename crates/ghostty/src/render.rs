//! What a terminal's viewport looks like, read for drawing: rows of cells with their text, colors and style.

use std::ffi::c_void;
use std::mem;
use std::ops::Range;
use std::ptr;

use taco_protocol::{Rgb, TerminalSize};

use crate::sys;
use crate::terminal::{Error, Terminal, check};

// The style flags are C macros, which the bindings do not carry.
const FLAG_BOLD: u16 = 1 << 0;
const FLAG_ITALIC: u16 = 1 << 1;
const FLAG_FAINT: u16 = 1 << 2;
const FLAG_BLINK: u16 = 1 << 3;
const FLAG_INVERSE: u16 = 1 << 4;
const FLAG_INVISIBLE: u16 = 1 << 5;
const FLAG_STRIKETHROUGH: u16 = 1 << 6;
const FLAG_OVERLINE: u16 = 1 << 7;
const UNDERLINE_MASK: u16 = 0x7 << sys::GHOSTTY_STYLE_UNDERLINE_SHIFT;
const PALETTE_SIZE: usize = 256;

/// How much of the viewport changed since the last `Frame::clean`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dirty {
    Clean,
    /// Some rows changed; `next_row` with `only_dirty` returns just those.
    Partial,
    /// Everything has to be drawn again, whatever each row's own flag says.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCursor {
    /// Column and row in the viewport, or none when the cursor is scrolled out of it.
    pub position: Option<(u16, u16)>,
    /// True when the cursor is on the second cell of a wide character. False when there is no position.
    pub wide_tail: bool,
    /// False when the program's modes hide the cursor.
    pub visible: bool,
    pub blinking: bool,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameColors {
    pub background: Rgb,
    pub foreground: Rgb,
    /// Only when a program has set one.
    pub cursor: Option<Rgb>,
    pub palette: [Rgb; PALETTE_SIZE],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Narrow,
    /// The first of a character's two cells.
    Wide,
    /// The second of a wide character's two cells. It has no text of its own.
    SpacerTail,
    /// The empty cell left at the end of a line by a wide character that continued on the next line.
    SpacerHead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Underline {
    None,
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The cell's text, as a range of its own row's `Row::text`. Empty for a blank cell.
    pub text: Range<usize>,
    pub width: Width,
    /// None means the default foreground.
    pub foreground: Option<Rgb>,
    /// None means the default background.
    pub background: Option<Rgb>,
    /// None means the text's own color.
    pub underline_color: Option<Rgb>,
    pub underline: Underline,
    pub bold: bool,
    pub italic: bool,
    pub faint: bool,
    pub blink: bool,
    /// The drawer swaps foreground and background; the colors above are as the program set them.
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
    pub overline: bool,
    pub selected: bool,
    pub hyperlink: bool,
}

/// An id that stays with a row as it scrolls, so work done for it can be kept while it is clean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RowId(pub [u64; 2]);

/// One viewport row, refilled in place by `Frame::next_row`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: RowId,
    pub y: u16,
    pub dirty: bool,
    /// Every cell's text, one after another; each `Cell::text` is a range of it.
    pub text: String,
    pub cells: Vec<Cell>,
}

/// A terminal's viewport, read for drawing. Keep one per terminal: it tracks what changed since the last frame.
pub struct RenderState {
    raw: sys::GhosttyRenderState,
    rows: sys::GhosttyRenderStateRowIterator,
    cells: sys::GhosttyRenderStateRowCells,
    /// Scratch space the library writes a cell's text into. Only its capacity is used; its length stays 0.
    utf8: Vec<u8>,
}

/// The viewport as of one update. Rows are read from it in order, top to bottom.
pub struct Frame<'a> {
    state: &'a mut RenderState,
    colors: FrameColors,
}

impl RenderState {
    pub fn new() -> Result<RenderState, Error> {
        let mut raw: sys::GhosttyRenderState = ptr::null_mut();
        let created = unsafe { sys::ghostty_render_state_new(ptr::null(), &mut raw) };
        check(created, "create a render state")?;

        let mut rows: sys::GhosttyRenderStateRowIterator = ptr::null_mut();
        let created = unsafe { sys::ghostty_render_state_row_iterator_new(ptr::null(), &mut rows) };
        if let Err(error) = check(created, "create a row iterator") {
            unsafe { sys::ghostty_render_state_free(raw) };
            return Err(error);
        }

        let mut cells: sys::GhosttyRenderStateRowCells = ptr::null_mut();
        let created = unsafe { sys::ghostty_render_state_row_cells_new(ptr::null(), &mut cells) };
        if let Err(error) = check(created, "create a row's cells") {
            unsafe { sys::ghostty_render_state_row_iterator_free(rows) };
            unsafe { sys::ghostty_render_state_free(raw) };
            return Err(error);
        }

        Ok(RenderState {
            raw,
            rows,
            cells,
            utf8: Vec::new(),
        })
    }

    /// The terminal's viewport as of now, for one frame.
    pub fn update(&mut self, terminal: &Terminal) -> Result<Frame<'_>, Error> {
        let updated = unsafe { sys::ghostty_render_state_update(self.raw, terminal.as_raw()) };
        check(updated, "update the render state")?;
        // The library fills the iterator we own with this update's rows.
        let mut iterator = self.rows;
        self.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
            &mut iterator,
        )?;
        let colors = self.read_colors()?;
        Ok(Frame {
            state: self,
            colors,
        })
    }

    fn read_colors(&self) -> Result<FrameColors, Error> {
        let mut raw: sys::GhosttyRenderStateColors = unsafe { mem::zeroed() };
        // A sized struct: the library reads `size` to know which version of it it is filling.
        raw.size = mem::size_of::<sys::GhosttyRenderStateColors>();
        self.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_COLORS,
            &mut raw,
        )?;

        let mut cursor = None;
        if raw.cursor_has_value {
            cursor = Some(from_sys(raw.cursor));
        }
        let palette = raw.palette.map(from_sys);
        Ok(FrameColors {
            background: from_sys(raw.background),
            foreground: from_sys(raw.foreground),
            cursor,
            palette,
        })
    }

    fn get<T>(&self, data: sys::GhosttyRenderStateData, out: &mut T) -> Result<(), Error> {
        let pointer = out as *mut T as *mut c_void;
        let read = unsafe { sys::ghostty_render_state_get(self.raw, data, pointer) };
        check(read, "read the render state")
    }

    // The row and cell reads return the library's result unchecked: some of them answer NO_VALUE or OUT_OF_SPACE.
    fn row_query<T>(
        &self,
        data: sys::GhosttyRenderStateRowData,
        out: &mut T,
    ) -> sys::GhosttyResult {
        let pointer = out as *mut T as *mut c_void;
        unsafe { sys::ghostty_render_state_row_get(self.rows, data, pointer) }
    }

    fn cell_query<T>(
        &self,
        data: sys::GhosttyRenderStateRowCellsData,
        out: &mut T,
    ) -> sys::GhosttyResult {
        let pointer = out as *mut T as *mut c_void;
        unsafe { sys::ghostty_render_state_row_cells_get(self.cells, data, pointer) }
    }

    /// The current cell's text, appended to `text`.
    fn read_text(&mut self, text: &mut String) -> Result<(), Error> {
        let mut buffer = sys::GhosttyBuffer {
            ptr: self.utf8.as_mut_ptr(),
            cap: self.utf8.capacity(),
            len: 0,
        };
        let mut read = self.cell_query(
            sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
            &mut buffer,
        );
        // Too small, the library reports the size it needs in `len`; grow to it and read once more.
        if read == sys::GhosttyResult_GHOSTTY_OUT_OF_SPACE {
            let needed = buffer.len;
            self.utf8.reserve(needed);
            buffer = sys::GhosttyBuffer {
                ptr: self.utf8.as_mut_ptr(),
                cap: self.utf8.capacity(),
                len: 0,
            };
            read = self.cell_query(
                sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                &mut buffer,
            );
        }
        check(read, "read a cell's text")?;
        let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
        let decoded = String::from_utf8_lossy(bytes);
        text.push_str(&decoded);
        Ok(())
    }

    /// The current cell's color of `data`, or none when the cell has no color of its own.
    fn cell_color(&self, data: sys::GhosttyRenderStateRowCellsData) -> Result<Option<Rgb>, Error> {
        let mut color = sys::GhosttyColorRgb { r: 0, g: 0, b: 0 };
        let read = self.cell_query(data, &mut color);
        if read == sys::GhosttyResult_GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        check(read, "read a cell's color")?;
        Ok(Some(from_sys(color)))
    }
}

impl Drop for RenderState {
    fn drop(&mut self) {
        unsafe { sys::ghostty_render_state_row_cells_free(self.cells) };
        unsafe { sys::ghostty_render_state_row_iterator_free(self.rows) };
        unsafe { sys::ghostty_render_state_free(self.raw) };
    }
}

impl Frame<'_> {
    pub fn dirty(&self) -> Result<Dirty, Error> {
        let mut dirty: sys::GhosttyRenderStateDirty = 0;
        self.state.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_DIRTY,
            &mut dirty,
        )?;
        match dirty {
            sys::GhosttyRenderStateDirty_GHOSTTY_RENDER_STATE_DIRTY_FALSE => Ok(Dirty::Clean),
            sys::GhosttyRenderStateDirty_GHOSTTY_RENDER_STATE_DIRTY_PARTIAL => Ok(Dirty::Partial),
            sys::GhosttyRenderStateDirty_GHOSTTY_RENDER_STATE_DIRTY_FULL => Ok(Dirty::Full),
            _ => Err(unknown("read the dirty state")),
        }
    }

    pub fn size(&self) -> Result<TerminalSize, Error> {
        let mut size = TerminalSize { cols: 0, rows: 0 };
        self.state.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_COLS,
            &mut size.cols,
        )?;
        self.state.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_ROWS,
            &mut size.rows,
        )?;
        Ok(size)
    }

    pub fn cursor(&self) -> Result<FrameCursor, Error> {
        let mut raw: sys::GhosttyRenderStateCursor = unsafe { mem::zeroed() };
        raw.size = mem::size_of::<sys::GhosttyRenderStateCursor>();
        self.state.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_CURSOR,
            &mut raw,
        )?;

        let mut position = None;
        let mut wide_tail = false;
        if raw.viewport_has_value {
            position = Some((raw.viewport_x, raw.viewport_y));
            wide_tail = raw.wide_tail;
        }
        let shape = match raw.visual_style {
            sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_BAR => CursorShape::Bar,
            sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_BLOCK => CursorShape::Block,
            sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_UNDERLINE => CursorShape::Underline,
            sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW => CursorShape::BlockHollow,
            _ => return Err(unknown("read the cursor's shape")),
        };
        Ok(FrameCursor {
            position,
            wide_tail,
            visible: raw.visible,
            blinking: raw.blinking,
            shape,
        })
    }

    pub fn colors(&self) -> &FrameColors {
        &self.colors
    }

    /// The next row, read into `row`, skipping unchanged rows with `only_dirty`. False when no rows remain.
    pub fn next_row(&mut self, row: &mut Row, only_dirty: bool) -> Result<bool, Error> {
        let state = &mut *self.state;
        let moved = advance(state.rows, only_dirty);
        if !moved {
            return Ok(false);
        }

        let mut viewport_y: i32 = 0;
        let read = state.row_query(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_VIEWPORT_Y,
            &mut viewport_y,
        );
        check(read, "read a row's position")?;
        // Only overscan rows sit above the viewport, and this reader never asks for overscan.
        let Ok(y) = u16::try_from(viewport_y) else {
            return Err(unknown("read a row's position"));
        };
        let mut id = sys::GhosttyRenderStateRowId { bits: [0, 0] };
        let read = state.row_query(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_ID,
            &mut id,
        );
        check(read, "read a row's id")?;
        let mut dirty = false;
        let read = state.row_query(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY,
            &mut dirty,
        );
        check(read, "read a row's dirty flag")?;
        let selection = row_selection(state)?;

        // The library fills the cells handle we own with this row's cells.
        let mut cells = state.cells;
        let read = state.row_query(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
            &mut cells,
        );
        check(read, "read a row's cells")?;

        row.id = RowId(id.bits);
        row.y = y;
        row.dirty = dirty;
        row.text.clear();
        row.cells.clear();
        let mut column: u16 = 0;
        while unsafe { sys::ghostty_render_state_row_cells_next(state.cells) } {
            let cell = read_cell(
                state,
                &self.colors.palette,
                &mut row.text,
                column,
                selection,
            )?;
            row.cells.push(cell);
            column += 1;
        }
        Ok(true)
    }

    /// The end of a drawn frame. Without it, every row read now is still reported dirty by the next update.
    pub fn clean(self) -> Result<(), Error> {
        let cleaned = unsafe { sys::ghostty_render_state_clean(self.state.raw) };
        check(cleaned, "clean the render state")
    }
}

impl Row {
    pub fn new() -> Row {
        Row {
            id: RowId([0, 0]),
            y: 0,
            dirty: false,
            text: String::new(),
            cells: Vec::new(),
        }
    }

    pub fn cell_text(&self, cell: &Cell) -> &str {
        &self.text[cell.text.clone()]
    }
}

impl Default for Row {
    fn default() -> Row {
        Row::new()
    }
}

fn advance(rows: sys::GhosttyRenderStateRowIterator, only_dirty: bool) -> bool {
    if only_dirty {
        // The dirty walk also reports a position; `next_row` reads the row's own VIEWPORT_Y for both walks.
        let mut walked_y: u16 = 0;
        return unsafe { sys::ghostty_render_state_row_iterator_next_dirty(rows, &mut walked_y) };
    }
    unsafe { sys::ghostty_render_state_row_iterator_next(rows) }
}

/// A value the library returned that this version does not know.
fn unknown(call: &'static str) -> Error {
    Error {
        code: sys::GhosttyResult_GHOSTTY_INVALID_VALUE,
        call,
    }
}

/// The current row's selected columns, inclusive, when the selection reaches the row.
fn row_selection(state: &RenderState) -> Result<Option<(u16, u16)>, Error> {
    let mut selection: sys::GhosttyRenderStateRowSelection = unsafe { mem::zeroed() };
    selection.size = mem::size_of::<sys::GhosttyRenderStateRowSelection>();
    let read = state.row_query(
        sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION,
        &mut selection,
    );
    if read == sys::GhosttyResult_GHOSTTY_NO_VALUE {
        return Ok(None);
    }
    check(read, "read a row's selection")?;
    Ok(Some((selection.start_x, selection.end_x)))
}

fn read_cell(
    state: &mut RenderState,
    palette: &[Rgb; PALETTE_SIZE],
    text: &mut String,
    column: u16,
    selection: Option<(u16, u16)>,
) -> Result<Cell, Error> {
    let mut raw: sys::GhosttyCell = 0;
    let read = state.cell_query(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
        &mut raw,
    );
    check(read, "read a raw cell")?;
    let width = cell_width(raw)?;
    let mut hyperlink = false;
    let read = raw_cell_query(
        raw,
        sys::GhosttyCellData_GHOSTTY_CELL_DATA_HAS_HYPERLINK,
        &mut hyperlink,
    );
    check(read, "read a cell's hyperlink flag")?;

    let start = text.len();
    state.read_text(text)?;
    let end = text.len();

    let mut styled = false;
    let read = state.cell_query(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING,
        &mut styled,
    );
    check(read, "read a cell's styling flag")?;
    // Zeroed is the default style: no flags, and every color tag 0, which is STYLE_COLOR_NONE.
    let mut style: sys::GhosttyStyle = unsafe { mem::zeroed() };
    if styled {
        let read = state.cell_query(
            sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
            &mut style,
        );
        check(read, "read a cell's style")?;
    }
    let foreground = state.cell_color(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR,
    )?;
    let background = state.cell_color(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR,
    )?;

    let flags = style.flags;
    let underline = underline_of(flags)?;
    let underline_color = style_color(&style.underline_color, palette);
    let mut selected = false;
    if let Some((first, last)) = selection {
        selected = column >= first && column <= last;
    }
    Ok(Cell {
        text: start..end,
        width,
        foreground,
        background,
        underline_color,
        underline,
        bold: flags & FLAG_BOLD != 0,
        italic: flags & FLAG_ITALIC != 0,
        faint: flags & FLAG_FAINT != 0,
        blink: flags & FLAG_BLINK != 0,
        inverse: flags & FLAG_INVERSE != 0,
        invisible: flags & FLAG_INVISIBLE != 0,
        strikethrough: flags & FLAG_STRIKETHROUGH != 0,
        overline: flags & FLAG_OVERLINE != 0,
        selected,
        hyperlink,
    })
}

fn raw_cell_query<T>(
    raw: sys::GhosttyCell,
    data: sys::GhosttyCellData,
    out: &mut T,
) -> sys::GhosttyResult {
    let pointer = out as *mut T as *mut c_void;
    unsafe { sys::ghostty_cell_get(raw, data, pointer) }
}

fn cell_width(raw: sys::GhosttyCell) -> Result<Width, Error> {
    let mut wide: sys::GhosttyCellWide = 0;
    let read = raw_cell_query(raw, sys::GhosttyCellData_GHOSTTY_CELL_DATA_WIDE, &mut wide);
    check(read, "read a cell's width")?;
    match wide {
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_NARROW => Ok(Width::Narrow),
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_WIDE => Ok(Width::Wide),
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_SPACER_TAIL => Ok(Width::SpacerTail),
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_SPACER_HEAD => Ok(Width::SpacerHead),
        _ => Err(unknown("read a cell's width")),
    }
}

fn underline_of(flags: u16) -> Result<Underline, Error> {
    let bits = (flags & UNDERLINE_MASK) >> sys::GHOSTTY_STYLE_UNDERLINE_SHIFT;
    let style = u32::from(bits);
    match style {
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_NONE => Ok(Underline::None),
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_SINGLE => Ok(Underline::Single),
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_DOUBLE => Ok(Underline::Double),
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_CURLY => Ok(Underline::Curly),
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_DOTTED => Ok(Underline::Dotted),
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_DASHED => Ok(Underline::Dashed),
        _ => Err(unknown("read a cell's underline")),
    }
}

fn style_color(color: &sys::GhosttyStyleColor, palette: &[Rgb; PALETTE_SIZE]) -> Option<Rgb> {
    let tag = u32::from(color.tag);
    if tag == sys::GhosttyStyleColorTag_GHOSTTY_STYLE_COLOR_RGB {
        let rgb = unsafe { color.value.rgb };
        return Some(from_sys(rgb));
    }
    if tag == sys::GhosttyStyleColorTag_GHOSTTY_STYLE_COLOR_PALETTE {
        let index = unsafe { color.value.palette };
        return Some(palette[usize::from(index)]);
    }
    None
}

fn from_sys(color: sys::GhosttyColorRgb) -> Rgb {
    Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_protocol::TerminalColors;

    const SIZE: TerminalSize = TerminalSize { cols: 20, rows: 4 };

    fn rows_of(state: &mut RenderState, terminal: &Terminal, only_dirty: bool) -> Vec<Row> {
        let mut frame = state.update(terminal).unwrap();
        let mut rows = Vec::new();
        let mut row = Row::new();
        while frame.next_row(&mut row, only_dirty).unwrap() {
            rows.push(row.clone());
        }
        frame.clean().unwrap();
        rows
    }

    fn rgb(value: u32) -> Rgb {
        let [_, r, g, b] = value.to_be_bytes();
        Rgb { r, g, b }
    }

    #[test]
    fn cells_carry_their_text_width_colors_and_style() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        // a: bold, italic, inverse. 日: wide. c: a 24-bit color. d: a curly underline in palette color 1.
        terminal.write(
            b"\x1b[1;3;7ma\x1b[0m\xe6\x97\xa5\x1b[38;2;1;2;3mc\x1b[0m\x1b[4:3;58:5:1md\x1b[0m",
        );
        // e: faint, blinking, invisible, struck through, overlined, double underline in 24-bit color 9.
        terminal.write(b"\x1b[2;5;8;9;53;21;58:2::0:0:9me\x1b[0m");
        // f: a hyperlink.
        terminal.write(b"\x1b]8;;https://example.com\x07f\x1b]8;;\x07\r\nnext");
        let mut state = RenderState::new().unwrap();
        let palette_1 = state.update(&terminal).unwrap().colors().palette[1];
        let rows = rows_of(&mut state, &terminal, false);
        let first = &rows[0];

        let a = &first.cells[0];
        assert_eq!(first.cell_text(a), "a");
        assert!(a.bold && a.italic && a.inverse);
        assert_eq!(a.foreground, None);

        let wide = &first.cells[1];
        assert_eq!((first.cell_text(wide), wide.width), ("日", Width::Wide));
        let tail = &first.cells[2];
        assert_eq!((first.cell_text(tail), tail.width), ("", Width::SpacerTail));

        let c = &first.cells[3];
        assert_eq!(c.foreground, Some(rgb(0x010203)));

        let d = &first.cells[4];
        assert_eq!(d.underline, Underline::Curly);
        assert_eq!(d.underline_color, Some(palette_1));

        let e = &first.cells[5];
        assert!(e.faint && e.blink && e.invisible && e.strikethrough && e.overline);
        assert!(!e.bold && !e.italic && !e.inverse);
        assert_eq!(e.underline, Underline::Double);
        assert_eq!(e.underline_color, Some(rgb(0x000009)));

        let f = &first.cells[6];
        assert!(f.hyperlink && !e.hyperlink);

        // The second row is read into the same buffer and keeps none of the first.
        let second = &rows[1];
        assert_eq!(second.text, "next");
        assert_eq!(second.cells.len(), usize::from(SIZE.cols));
        assert_ne!(second.id, first.id);
    }

    #[test]
    fn after_a_clean_frame_only_changed_rows_are_dirty() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        // The cursor already sits on the row that changes, since the row it leaves would be redrawn as well.
        terminal.write(b"one\r\ntwo\r\nthree\x1b[2;1H");
        let mut state = RenderState::new().unwrap();
        rows_of(&mut state, &terminal, true);

        terminal.write(b"changed");
        let second = rows_of(&mut state, &terminal, true);
        let ys: Vec<u16> = second.iter().map(|row| row.y).collect();
        assert_eq!(ys, vec![1]);
        assert_eq!(second[0].text, "changed");
        assert!(second[0].dirty);
    }

    #[test]
    fn a_frame_keeps_its_rows_while_the_terminal_changes_under_it() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        terminal.write(b"before");
        let mut state = RenderState::new().unwrap();
        let mut frame = state.update(&terminal).unwrap();

        // A frame borrows the render state, not the terminal: output that scrolls every row away, and a resize, land mid-frame.
        let flood = "line\r\n".repeat(1_000);
        terminal.write(flood.as_bytes());
        let smaller = TerminalSize { cols: 8, rows: 2 };
        terminal.resize(smaller, 8, 16).unwrap();

        let mut row = Row::new();
        let mut texts = Vec::new();
        while frame.next_row(&mut row, false).unwrap() {
            texts.push(row.text.clone());
        }
        assert_eq!(texts.len(), usize::from(SIZE.rows));
        assert_eq!(texts[0], "before");
    }

    #[test]
    fn the_frame_reports_the_cursor_and_the_colors_set() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        let mut ansi = [rgb(0); 16];
        ansi[1] = rgb(0xf0646c);
        let colors = TerminalColors {
            foreground: rgb(0xd6d0c9),
            background: rgb(0x191918),
            cursor: rgb(0xece7e1),
            cursor_text: rgb(0x191918),
            selection: rgb(0x3a3836),
            ansi,
        };
        terminal.set_colors(&colors).unwrap();
        // A bar cursor at column 4 of row 2, and a cursor color the program set.
        terminal.write(b"\x1b[5 q\x1b[3;5H\x1b]12;rgb:12/34/56\x07");

        let mut state = RenderState::new().unwrap();
        let frame = state.update(&terminal).unwrap();
        let cursor = frame.cursor().unwrap();
        assert_eq!(cursor.position, Some((4, 2)));
        assert_eq!(cursor.shape, CursorShape::Bar);
        let read = frame.colors();
        assert_eq!(read.foreground, colors.foreground);
        assert_eq!(read.background, colors.background);
        assert_eq!(read.palette[1], ansi[1]);
        assert_eq!(read.cursor, Some(rgb(0x123456)));
        assert_eq!(frame.size().unwrap(), SIZE);
    }
}
