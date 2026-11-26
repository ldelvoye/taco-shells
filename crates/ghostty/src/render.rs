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

/// How much of the viewport changed since the frame was last cleaned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dirty {
    Clean,
    Partial,
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
    /// On the second cell of a wide character.
    pub wide_tail: bool,
    /// What the program's modes say; a hidden cursor is not drawn.
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
    /// A wide character that did not fit at the end of a line continues on the next one; this is the empty cell it left.
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
    /// The cell's text, as a range of `Row::text`. Empty for a blank cell.
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
    /// Foreground and background swapped. The colors above are not swapped already.
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

/// Reads a terminal's viewport for drawing. Keep one per terminal: it tracks what changed since the last frame.
pub struct RenderState {
    raw: sys::GhosttyRenderState,
    rows: sys::GhosttyRenderStateRowIterator,
    cells: sys::GhosttyRenderStateRowCells,
    /// Reused for each cell's text.
    utf8: Vec<u8>,
}

/// The viewport as of one update. Rows are read from it in order, top to bottom.
pub struct Frame<'a> {
    state: &'a mut RenderState,
    /// For style colors given as a palette index.
    palette: [sys::GhosttyColorRgb; PALETTE_SIZE],
}

impl RenderState {
    pub fn new() -> Result<RenderState, Error> {
        let mut raw: sys::GhosttyRenderState = ptr::null_mut();
        let created = unsafe { sys::ghostty_render_state_new(ptr::null(), &mut raw) };
        check(created, "ghostty_render_state_new")?;

        let mut rows: sys::GhosttyRenderStateRowIterator = ptr::null_mut();
        let created = unsafe { sys::ghostty_render_state_row_iterator_new(ptr::null(), &mut rows) };
        if let Err(error) = check(created, "ghostty_render_state_row_iterator_new") {
            unsafe { sys::ghostty_render_state_free(raw) };
            return Err(error);
        }

        let mut cells: sys::GhosttyRenderStateRowCells = ptr::null_mut();
        let created = unsafe { sys::ghostty_render_state_row_cells_new(ptr::null(), &mut cells) };
        if let Err(error) = check(created, "ghostty_render_state_row_cells_new") {
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

    /// Reads the terminal's viewport. What the frame returns stays valid until the next update.
    pub fn update(&mut self, terminal: &Terminal) -> Result<Frame<'_>, Error> {
        let updated = unsafe { sys::ghostty_render_state_update(self.raw, terminal.as_raw()) };
        check(updated, "ghostty_render_state_update")?;
        let iterator = &mut self.rows as *mut sys::GhosttyRenderStateRowIterator as *mut c_void;
        let populated = unsafe {
            sys::ghostty_render_state_get(
                self.raw,
                sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                iterator,
            )
        };
        check(populated, "populate the row iterator")?;
        let mut palette = [sys::GhosttyColorRgb { r: 0, g: 0, b: 0 }; PALETTE_SIZE];
        self.get(
            sys::GhosttyRenderStateData_GHOSTTY_RENDER_STATE_DATA_COLOR_PALETTE,
            &mut palette,
        )?;
        Ok(Frame {
            state: self,
            palette,
        })
    }

    fn get<T>(&self, data: sys::GhosttyRenderStateData, out: &mut T) -> Result<(), Error> {
        let pointer = out as *mut T as *mut c_void;
        let read = unsafe { sys::ghostty_render_state_get(self.raw, data, pointer) };
        check(read, "ghostty_render_state_get")
    }

    fn row_get<T>(&self, data: sys::GhosttyRenderStateRowData, out: &mut T) -> sys::GhosttyResult {
        let pointer = out as *mut T as *mut c_void;
        unsafe { sys::ghostty_render_state_row_get(self.rows, data, pointer) }
    }

    fn cell_get<T>(
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
        let mut read = self.cell_get(
            sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
            &mut buffer,
        );
        if read == sys::GhosttyResult_GHOSTTY_OUT_OF_SPACE {
            let needed = buffer.len;
            self.utf8.reserve(needed);
            buffer = sys::GhosttyBuffer {
                ptr: self.utf8.as_mut_ptr(),
                cap: self.utf8.capacity(),
                len: 0,
            };
            read = self.cell_get(
                sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                &mut buffer,
            );
        }
        check(read, "a cell's text")?;
        let bytes = unsafe { std::slice::from_raw_parts(buffer.ptr, buffer.len) };
        let decoded = String::from_utf8_lossy(bytes);
        text.push_str(&decoded);
        Ok(())
    }

    /// The current cell's color of `data`, or none when the cell has no color of its own.
    fn cell_color(&self, data: sys::GhosttyRenderStateRowCellsData) -> Result<Option<Rgb>, Error> {
        let mut color = sys::GhosttyColorRgb { r: 0, g: 0, b: 0 };
        let read = self.cell_get(data, &mut color);
        if read == sys::GhosttyResult_GHOSTTY_NO_VALUE {
            return Ok(None);
        }
        check(read, "a cell's color")?;
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
        let kind = match dirty {
            sys::GhosttyRenderStateDirty_GHOSTTY_RENDER_STATE_DIRTY_FALSE => Dirty::Clean,
            sys::GhosttyRenderStateDirty_GHOSTTY_RENDER_STATE_DIRTY_PARTIAL => Dirty::Partial,
            _ => Dirty::Full,
        };
        Ok(kind)
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
            sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_UNDERLINE => CursorShape::Underline,
            sys::GhosttyCursorStyle_GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW => CursorShape::BlockHollow,
            _ => CursorShape::Block,
        };
        Ok(FrameCursor {
            position,
            wide_tail,
            visible: raw.visible,
            blinking: raw.blinking,
            shape,
        })
    }

    pub fn colors(&self) -> Result<FrameColors, Error> {
        let mut raw: sys::GhosttyRenderStateColors = unsafe { mem::zeroed() };
        raw.size = mem::size_of::<sys::GhosttyRenderStateColors>();
        self.state.get(
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

    /// Reads the next row into `row`: every row in turn, or with `only_dirty` only those that changed. False once there are no more.
    pub fn next_row(&mut self, row: &mut Row, only_dirty: bool) -> Result<bool, Error> {
        let state = &mut *self.state;
        let mut y: u16 = 0;
        let moved = if only_dirty {
            unsafe { sys::ghostty_render_state_row_iterator_next_dirty(state.rows, &mut y) }
        } else {
            unsafe { sys::ghostty_render_state_row_iterator_next(state.rows) }
        };
        if !moved {
            return Ok(false);
        }

        let mut viewport_y: i32 = 0;
        let read = state.row_get(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_VIEWPORT_Y,
            &mut viewport_y,
        );
        check(read, "a row's position")?;
        let mut id = sys::GhosttyRenderStateRowId { bits: [0, 0] };
        let read = state.row_get(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_ID,
            &mut id,
        );
        check(read, "a row's id")?;
        let mut dirty = false;
        let read = state.row_get(
            sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY,
            &mut dirty,
        );
        check(read, "a row's dirty flag")?;
        let selection = row_selection(state)?;

        let cells = &mut state.cells as *mut sys::GhosttyRenderStateRowCells as *mut c_void;
        let populated = unsafe {
            sys::ghostty_render_state_row_get(
                state.rows,
                sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                cells,
            )
        };
        check(populated, "a row's cells")?;

        row.id = RowId(id.bits);
        row.y = u16::try_from(viewport_y).unwrap_or(0);
        row.dirty = dirty;
        row.text.clear();
        row.cells.clear();
        let mut column: u16 = 0;
        while unsafe { sys::ghostty_render_state_row_cells_next(state.cells) } {
            let cell = read_cell(state, &self.palette, &mut row.text, column, selection)?;
            row.cells.push(cell);
            column += 1;
        }
        Ok(true)
    }

    /// Marks the whole frame drawn, so the next update reports only what changes after it.
    pub fn clean(self) -> Result<(), Error> {
        let cleaned = unsafe { sys::ghostty_render_state_clean(self.state.raw) };
        check(cleaned, "ghostty_render_state_clean")
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

/// The current row's selected columns, inclusive, when the selection crosses it.
fn row_selection(state: &RenderState) -> Result<Option<(u16, u16)>, Error> {
    let mut selection: sys::GhosttyRenderStateRowSelection = unsafe { mem::zeroed() };
    selection.size = mem::size_of::<sys::GhosttyRenderStateRowSelection>();
    let read = state.row_get(
        sys::GhosttyRenderStateRowData_GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION,
        &mut selection,
    );
    if read == sys::GhosttyResult_GHOSTTY_NO_VALUE {
        return Ok(None);
    }
    check(read, "a row's selection")?;
    Ok(Some((selection.start_x, selection.end_x)))
}

fn read_cell(
    state: &mut RenderState,
    palette: &[sys::GhosttyColorRgb; PALETTE_SIZE],
    text: &mut String,
    column: u16,
    selection: Option<(u16, u16)>,
) -> Result<Cell, Error> {
    let mut raw: sys::GhosttyCell = 0;
    let read = state.cell_get(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW,
        &mut raw,
    );
    check(read, "a raw cell")?;
    let width = cell_width(raw)?;
    let mut hyperlink = false;
    let read = unsafe {
        sys::ghostty_cell_get(
            raw,
            sys::GhosttyCellData_GHOSTTY_CELL_DATA_HAS_HYPERLINK,
            &mut hyperlink as *mut bool as *mut c_void,
        )
    };
    check(read, "a cell's hyperlink flag")?;

    let start = text.len();
    state.read_text(text)?;
    let end = text.len();

    let mut styled = false;
    let read = state.cell_get(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING,
        &mut styled,
    );
    check(read, "a cell's styling flag")?;
    let mut style: sys::GhosttyStyle = unsafe { mem::zeroed() };
    if styled {
        let read = state.cell_get(
            sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE,
            &mut style,
        );
        check(read, "a cell's style")?;
    }
    let foreground = state.cell_color(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR,
    )?;
    let background = state.cell_color(
        sys::GhosttyRenderStateRowCellsData_GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR,
    )?;

    let flags = style.flags;
    let mut selected = false;
    if let Some((first, last)) = selection {
        selected = column >= first && column <= last;
    }
    Ok(Cell {
        text: start..end,
        width,
        foreground,
        background,
        underline_color: style_color(&style.underline_color, palette),
        underline: underline_of(flags),
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

fn cell_width(raw: sys::GhosttyCell) -> Result<Width, Error> {
    let mut wide: sys::GhosttyCellWide = 0;
    let read = unsafe {
        sys::ghostty_cell_get(
            raw,
            sys::GhosttyCellData_GHOSTTY_CELL_DATA_WIDE,
            &mut wide as *mut sys::GhosttyCellWide as *mut c_void,
        )
    };
    check(read, "a cell's width")?;
    let width = match wide {
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_WIDE => Width::Wide,
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_SPACER_TAIL => Width::SpacerTail,
        sys::GhosttyCellWide_GHOSTTY_CELL_WIDE_SPACER_HEAD => Width::SpacerHead,
        _ => Width::Narrow,
    };
    Ok(width)
}

fn underline_of(flags: u16) -> Underline {
    let style = u32::from((flags & UNDERLINE_MASK) >> sys::GHOSTTY_STYLE_UNDERLINE_SHIFT);
    match style {
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_NONE => Underline::None,
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_DOUBLE => Underline::Double,
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_CURLY => Underline::Curly,
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_DOTTED => Underline::Dotted,
        sys::GhosttySgrUnderline_GHOSTTY_SGR_UNDERLINE_DASHED => Underline::Dashed,
        _ => Underline::Single,
    }
}

fn style_color(
    color: &sys::GhosttyStyleColor,
    palette: &[sys::GhosttyColorRgb; PALETTE_SIZE],
) -> Option<Rgb> {
    let tag = u32::from(color.tag);
    if tag == sys::GhosttyStyleColorTag_GHOSTTY_STYLE_COLOR_RGB {
        let rgb = unsafe { color.value.rgb };
        return Some(from_sys(rgb));
    }
    if tag == sys::GhosttyStyleColorTag_GHOSTTY_STYLE_COLOR_PALETTE {
        let index = unsafe { color.value.palette };
        return Some(from_sys(palette[usize::from(index)]));
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

    #[test]
    fn cells_carry_their_text_width_colors_and_style() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        // Bold italic inverse "a", a wide character, a 24-bit "c", then a curly underline colored by palette index 1.
        terminal.write(
            b"\x1b[1;3;7ma\x1b[0m\xe6\x97\xa5\x1b[38;2;1;2;3mc\x1b[0m\x1b[4:3;58:5:1md\x1b[0m",
        );
        let mut state = RenderState::new().unwrap();
        let rows = rows_of(&mut state, &terminal, false);
        let first = &rows[0];
        let palette_red = state.update(&terminal).unwrap().colors().unwrap().palette[1];

        let a = &first.cells[0];
        assert_eq!(first.cell_text(a), "a");
        assert!(a.bold && a.italic && a.inverse);
        assert!(!a.faint && !a.strikethrough);
        assert_eq!(a.foreground, None);

        let wide = &first.cells[1];
        assert_eq!((first.cell_text(wide), wide.width), ("日", Width::Wide));
        let tail = &first.cells[2];
        assert_eq!((first.cell_text(tail), tail.width), ("", Width::SpacerTail));

        let c = &first.cells[3];
        assert_eq!(c.foreground, Some(Rgb { r: 1, g: 2, b: 3 }));
        assert!(!c.bold);

        let d = &first.cells[4];
        assert_eq!(d.underline, Underline::Curly);
        assert_eq!(d.underline_color, Some(palette_red));

        let blank = &first.cells[5];
        assert_eq!(first.cell_text(blank), "");
        assert_eq!(first.cells.len(), usize::from(SIZE.cols));
    }

    #[test]
    fn after_a_clean_frame_only_changed_rows_are_dirty() {
        let mut terminal = Terminal::new(SIZE).unwrap();
        // The cursor waits on the row that changes: a row the cursor leaves is redrawn too.
        terminal.write(b"one\r\ntwo\r\nthree\x1b[2;1H");
        let mut state = RenderState::new().unwrap();
        let first = rows_of(&mut state, &terminal, true);
        assert_eq!(first.len(), usize::from(SIZE.rows));

        terminal.write(b"changed");
        let second = rows_of(&mut state, &terminal, true);
        let ys: Vec<u16> = second.iter().map(|row| row.y).collect();
        assert_eq!(ys, vec![1]);
        assert_eq!(second[0].text, "changed");
        // The row keeps its id while it changes in place.
        assert_eq!(second[0].id, first[1].id);
    }
}
