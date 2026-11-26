//! Turning a view's rows into backgrounds, runs of text and a cursor, then painting them in the window.

use gpui::{
    App, BorderStyle, Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, Point, Rgba,
    StrikethroughStyle, TextAlign, TextRun, UnderlineStyle, Window, fill, font, outline, point, px,
    size,
};
use taco_ghostty::render::{Cell, CursorShape, FrameColors, FrameCursor, Row, Underline, Width};
use taco_protocol::Rgb;

use crate::grid::CellMetrics;
use crate::shapes::{Shape, cell_bounds, paint_shape, shape_of};

const FAINT_TEXT_ALPHA: f32 = 0.6;
/// A block cursor lets the character under it show through.
const BLOCK_CURSOR_ALPHA: f32 = 0.6;
const TEXT_LINE_THICKNESS: f32 = 1.0;
const CURSOR_LINE_THICKNESS: f32 = 2.0;

/// A stretch of cells in one row whose background differs from the terminal's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Background {
    pub column: u16,
    pub row: u16,
    pub columns: u16,
    pub color: Hsla,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLine {
    pub color: Hsla,
    pub wavy: bool,
}

/// Cells in a row that share a style, drawn as one shaped line.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub column: u16,
    pub row: u16,
    /// How many cells the run covers, which its text alone does not say.
    pub columns: u16,
    pub text: String,
    pub color: Hsla,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<TextLine>,
    pub strikethrough: Option<Hsla>,
    /// Each cell drawn at exactly one cell's width. Off for a wide character or a cell of several code points, which keep their own advance.
    pub one_cell_per_glyph: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMark {
    pub column: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub color: Hsla,
    pub columns: u16,
    /// The settings and the program both let it blink.
    pub blinking: bool,
}

/// A block element or box-drawing character, drawn as shapes rather than from the font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellShape {
    pub column: u16,
    pub row: u16,
    pub shape: Shape,
    pub color: Hsla,
}

/// Everything one frame draws, in grid coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Painting {
    pub backgrounds: Vec<Background>,
    pub shapes: Vec<CellShape>,
    pub runs: Vec<Run>,
    pub cursor: Option<CursorMark>,
}

/// The painting for `rows`. `selection` is the background a selected cell gets.
pub fn lay_out(
    rows: &[Row],
    cursor: FrameCursor,
    colors: &FrameColors,
    selection: Rgb,
) -> Painting {
    let mut painting = Painting::default();
    // A grid has at most u16::MAX rows and columns, so the indexes below fit.
    for (index, row) in rows.iter().enumerate() {
        lay_out_row(row, index as u16, colors, selection, &mut painting);
    }
    if cursor.visible
        && let Some((column, row)) = cursor.position
    {
        let mut cursor_color = colors.foreground;
        if let Some(set) = colors.cursor {
            cursor_color = set;
        }
        let mut columns = 1;
        if is_wide_at(rows, column, row) {
            columns = 2;
        }
        painting.cursor = Some(CursorMark {
            column,
            row,
            shape: cursor.shape,
            color: hsla(cursor_color),
            columns,
            blinking: cursor.blinking,
        });
    }
    painting
}

/// The cursor as drawn this frame: a block is hollow while the terminal does not have focus, and a blinking one is hidden in its off phase.
pub fn cursor_as_shown(
    cursor: Option<CursorMark>,
    focused: bool,
    blink_on: bool,
) -> Option<CursorMark> {
    let mut shown = cursor?;
    if !focused {
        if shown.shape == CursorShape::Block {
            shown.shape = CursorShape::BlockHollow;
        }
        return Some(shown);
    }
    if shown.blinking && !blink_on {
        return None;
    }
    Some(shown)
}

fn is_wide_at(rows: &[Row], column: u16, row: u16) -> bool {
    let Some(line) = rows.get(usize::from(row)) else {
        return false;
    };
    let Some(cell) = line.cells.get(usize::from(column)) else {
        return false;
    };
    cell.width == Width::Wide
}

fn lay_out_row(row: &Row, y: u16, colors: &FrameColors, selection: Rgb, painting: &mut Painting) {
    for (index, cell) in row.cells.iter().enumerate() {
        let column = index as u16;
        let spacer = matches!(cell.width, Width::SpacerTail | Width::SpacerHead);
        if spacer {
            continue;
        }
        let mut columns = 1;
        if cell.width == Width::Wide {
            columns = 2;
        }

        let (foreground, background) = cell_colors(cell, colors, selection);
        if let Some(color) = background {
            let stretch = Background {
                column,
                row: y,
                columns,
                color: hsla(color),
            };
            add_background(&mut painting.backgrounds, stretch);
        }

        let text = row.cell_text(cell);
        if text.is_empty() || cell.invisible {
            continue;
        }
        let mut color = hsla(foreground);
        if cell.faint {
            color.a *= FAINT_TEXT_ALPHA;
        }
        if columns == 1
            && let Some(shape) = cell_shape(text)
        {
            let drawn = CellShape {
                column,
                row: y,
                shape,
                color,
            };
            painting.shapes.push(drawn);
            continue;
        }
        let mut strikethrough = None;
        if cell.strikethrough {
            strikethrough = Some(color);
        }
        let several_code_points = text.chars().nth(1).is_some();
        let run = Run {
            column,
            row: y,
            columns,
            text: text.to_string(),
            color,
            bold: cell.bold,
            italic: cell.italic,
            underline: underline_of(cell, color),
            strikethrough,
            one_cell_per_glyph: columns == 1 && !several_code_points,
        };
        add_run(&mut painting.runs, run);
    }
}

fn cell_shape(text: &str) -> Option<Shape> {
    let mut chars = text.chars();
    let glyph = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    shape_of(glyph)
}

/// The colors a cell is drawn in: inverse swaps them, and a selected cell takes the selection background.
fn cell_colors(cell: &Cell, colors: &FrameColors, selection: Rgb) -> (Rgb, Option<Rgb>) {
    let mut text = colors.foreground;
    if let Some(own) = cell.foreground {
        text = own;
    }
    let mut fill_color = cell.background;
    if cell.inverse {
        let mut ground = colors.background;
        if let Some(own) = cell.background {
            ground = own;
        }
        fill_color = Some(text);
        text = ground;
    }
    if cell.selected {
        fill_color = Some(selection);
    }
    (text, fill_color)
}

fn underline_of(cell: &Cell, text_color: Hsla) -> Option<TextLine> {
    if cell.underline == Underline::None {
        return None;
    }
    let mut color = text_color;
    if let Some(own) = cell.underline_color {
        color = hsla(own);
    }
    let wavy = cell.underline == Underline::Curly;
    Some(TextLine { color, wavy })
}

fn add_background(backgrounds: &mut Vec<Background>, stretch: Background) {
    if let Some(last) = backgrounds.last_mut() {
        let continues = last.row == stretch.row
            && last.column + last.columns == stretch.column
            && last.color == stretch.color;
        if continues {
            last.columns += stretch.columns;
            return;
        }
    }
    backgrounds.push(stretch);
}

fn add_run(runs: &mut Vec<Run>, run: Run) {
    if let Some(last) = runs.last_mut() {
        let continues = last.one_cell_per_glyph
            && run.one_cell_per_glyph
            && last.row == run.row
            && last.column + last.columns == run.column
            && last.color == run.color
            && last.bold == run.bold
            && last.italic == run.italic
            && last.underline == run.underline
            && last.strikethrough == run.strikethrough;
        if continues {
            last.text.push_str(&run.text);
            last.columns += run.columns;
            return;
        }
    }
    runs.push(run);
}

/// Paints `painting` with its grid's top-left corner at `origin`.
pub fn paint(
    painting: &Painting,
    origin: Point<Pixels>,
    cell: CellMetrics,
    family: &str,
    font_size: Pixels,
    window: &mut Window,
    cx: &mut App,
) {
    let cell_width = px(cell.width);
    let cell_height = px(cell.height);
    let at = |column: u16, row: u16| {
        let x = origin.x + cell_width * f32::from(column);
        let y = origin.y + cell_height * f32::from(row);
        point(x, y)
    };

    for background in &painting.backgrounds {
        let width = cell_width * f32::from(background.columns);
        let bounds = Bounds::new(
            at(background.column, background.row),
            size(width, cell_height),
        );
        window.paint_quad(fill(bounds, background.color));
    }

    let scale = window.scale_factor();
    for drawn in &painting.shapes {
        let bounds = cell_bounds(
            origin,
            cell.width,
            cell.height,
            drawn.column,
            drawn.row,
            scale,
        );
        paint_shape(drawn.shape, bounds, drawn.color, font_size, scale, window);
    }

    for run in &painting.runs {
        let text_run = text_run_for(run, family);
        // gpui gives every glyph exactly this advance, which keeps a run on the grid.
        let mut force_width = None;
        if run.one_cell_per_glyph {
            force_width = Some(cell_width);
        }
        let shaped = window.text_system().shape_line(
            run.text.clone().into(),
            font_size,
            &[text_run],
            force_width,
        );
        let _ = shaped.paint(
            at(run.column, run.row),
            cell_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    if let Some(cursor) = painting.cursor {
        let top_left = at(cursor.column, cursor.row);
        let width = cell_width * f32::from(cursor.columns);
        let bounds = cursor_bounds(cursor.shape, top_left, width, cell_height);
        match cursor.shape {
            CursorShape::BlockHollow => {
                window.paint_quad(outline(bounds, cursor.color, BorderStyle::Solid));
            }
            CursorShape::Block => {
                let mut color = cursor.color;
                color.a *= BLOCK_CURSOR_ALPHA;
                window.paint_quad(fill(bounds, color));
            }
            CursorShape::Bar | CursorShape::Underline => {
                window.paint_quad(fill(bounds, cursor.color));
            }
        }
    }
}

fn cursor_bounds(
    shape: CursorShape,
    top_left: Point<Pixels>,
    width: Pixels,
    height: Pixels,
) -> Bounds<Pixels> {
    let thickness = px(CURSOR_LINE_THICKNESS);
    match shape {
        CursorShape::Bar => Bounds::new(top_left, size(thickness, height)),
        CursorShape::Underline => {
            let bottom = point(top_left.x, top_left.y + height - thickness);
            Bounds::new(bottom, size(width, thickness))
        }
        CursorShape::Block | CursorShape::BlockHollow => Bounds::new(top_left, size(width, height)),
    }
}

fn text_run_for(run: &Run, family: &str) -> TextRun {
    let mut run_font: Font = font(family.to_string());
    if run.bold {
        run_font.weight = FontWeight::BOLD;
    }
    if run.italic {
        run_font.style = FontStyle::Italic;
    }
    let mut underline = None;
    if let Some(line) = run.underline {
        underline = Some(UnderlineStyle {
            thickness: px(TEXT_LINE_THICKNESS),
            color: Some(line.color),
            wavy: line.wavy,
        });
    }
    let mut strikethrough = None;
    if let Some(color) = run.strikethrough {
        strikethrough = Some(StrikethroughStyle {
            thickness: px(TEXT_LINE_THICKNESS),
            color: Some(color),
        });
    }
    TextRun {
        len: run.text.len(),
        font: run_font,
        color: run.color,
        background_color: None,
        underline,
        strikethrough,
    }
}

pub fn hsla(color: Rgb) -> Hsla {
    let rgba = Rgba {
        r: f32::from(color.r) / 255.0,
        g: f32::from(color.g) / 255.0,
        b: f32::from(color.b) / 255.0,
        a: 1.0,
    };
    rgba.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_ghostty::Terminal;
    use taco_ghostty::input::CellPoint;
    use taco_ghostty::render::RenderState;
    use taco_protocol::TerminalSize;

    const SELECTION: Rgb = Rgb {
        r: 0x3a,
        g: 0x38,
        b: 0x36,
    };

    fn painting_of(terminal: &Terminal) -> (Painting, FrameColors) {
        let mut state = RenderState::new().unwrap();
        let mut frame = state.update(terminal).unwrap();
        let mut rows = Vec::new();
        let mut row = Row::new();
        while frame.next_row(&mut row, false).unwrap() {
            rows.push(row.clone());
        }
        let colors = frame.colors().clone();
        let cursor = frame.cursor().unwrap();
        (lay_out(&rows, cursor, &colors, SELECTION), colors)
    }

    #[test]
    fn cells_become_runs_and_backgrounds_in_the_colors_we_chose() {
        let mut terminal = Terminal::new(TerminalSize { cols: 20, rows: 3 }).unwrap();
        // Row 0: plain text, an inverse word, a hidden word, a wide character.
        terminal.write(b"ab \x1b[7minv\x1b[0m \x1b[8mhid\x1b[0m \xe6\x97\xa5\r\n");
        // Row 1: faint text, a curly underline, an e with a combining accent, then plain text after it.
        terminal.write(b"\x1b[2mfa\x1b[0m \x1b[4:3mcu\x1b[0m e\xcc\x81 x");
        terminal
            .select(CellPoint { x: 0, y: 2 }, CellPoint { x: 3, y: 2 })
            .unwrap();
        let (painting, colors) = painting_of(&terminal);

        let first_row: Vec<(&str, u16)> = painting
            .runs
            .iter()
            .filter(|run| run.row == 0)
            .map(|run| (run.text.as_str(), run.column))
            .collect();
        // Hidden text is not drawn, and the wide character stands alone.
        assert_eq!(
            first_row,
            vec![("ab ", 0), ("inv", 3), (" ", 6), (" ", 10), ("日", 11)]
        );
        let inverse = &painting.runs[1];
        assert_eq!(inverse.color, hsla(colors.background));
        let inverse_fill = painting.backgrounds[0];
        assert_eq!(
            (
                inverse_fill.column,
                inverse_fill.columns,
                inverse_fill.color
            ),
            (3, 3, hsla(colors.foreground))
        );

        let second_row: Vec<&Run> = painting.runs.iter().filter(|run| run.row == 1).collect();
        let faint = second_row[0];
        assert_eq!(faint.text, "fa");
        assert!(faint.color.a < 1.0);
        let curly = second_row.iter().find(|run| run.text == "cu").unwrap();
        assert!(curly.underline.unwrap().wavy);
        // The combined character is a run of its own, and the text after it keeps its column.
        let accent = second_row
            .iter()
            .find(|run| run.text == "e\u{301}")
            .unwrap();
        assert!(!accent.one_cell_per_glyph);
        let after = second_row.last().unwrap();
        assert_eq!((after.text.as_str(), after.column), (" x", 7));

        // The selected cells on the blank third row share one selection-colored stretch.
        let selected = painting
            .backgrounds
            .iter()
            .find(|stretch| stretch.row == 2)
            .unwrap();
        assert_eq!(
            (selected.column, selected.columns, selected.color),
            (0, 4, hsla(SELECTION))
        );
    }

    #[test]
    fn the_cursor_covers_a_wide_character_and_is_not_drawn_when_hidden() {
        let mut terminal = Terminal::new(TerminalSize { cols: 10, rows: 2 }).unwrap();
        terminal.write(b"\xe6\x97\xa5\x1b[1;1H");
        let (painting, _) = painting_of(&terminal);
        let cursor = painting.cursor.unwrap();
        assert_eq!((cursor.column, cursor.columns), (0, 2));

        terminal.write(b"\x1b[?25l");
        let (hidden, _) = painting_of(&terminal);
        assert_eq!(hidden.cursor, None);
    }

    #[test]
    fn block_and_box_characters_are_shapes_in_their_cells_colors_not_text() {
        let mut terminal = Terminal::new(TerminalSize { cols: 20, rows: 1 }).unwrap();
        // Claude Code's logo paints blocks over a black background; a box corner, a line, and a dashed line the font keeps.
        terminal.write("a\x1b[38;2;215;119;87;48;2;0;0;0m▛█\x1b[0m\x1b[2m╭─\x1b[0m┄b".as_bytes());
        let (painting, _) = painting_of(&terminal);

        let text: Vec<(&str, u16)> = painting
            .runs
            .iter()
            .map(|run| (run.text.as_str(), run.column))
            .collect();
        assert_eq!(text, vec![("a", 0), ("┄b", 5)]);
        let shapes: Vec<(u16, Shape)> = painting
            .shapes
            .iter()
            .map(|drawn| (drawn.column, drawn.shape))
            .collect();
        assert_eq!(shapes.len(), 4);
        assert_eq!(shapes[0], (1, shape_of('▛').unwrap()));
        assert_eq!(
            shapes[2],
            (
                3,
                Shape::Arc {
                    down: true,
                    right: true
                }
            )
        );
        let clawd = Rgb {
            r: 215,
            g: 119,
            b: 87,
        };
        assert_eq!(painting.shapes[0].color, hsla(clawd));
        // The black behind the block still fills the cell, to show through the quadrant ▛ leaves empty.
        let behind = painting.backgrounds[0];
        assert_eq!((behind.column, behind.columns), (1, 2));
        assert!(
            painting.shapes[3].color.a < 1.0,
            "faint applies to shapes too"
        );
    }

    #[test]
    fn an_unfocused_block_is_hollow_and_a_blinking_cursor_hides_in_its_off_phase() {
        let block = CursorMark {
            column: 0,
            row: 0,
            shape: CursorShape::Block,
            color: hsla(SELECTION),
            columns: 1,
            blinking: true,
        };
        let shown = |focused, blink_on| cursor_as_shown(Some(block), focused, blink_on);
        assert_eq!(shown(true, true), Some(block));
        assert_eq!(shown(true, false), None);
        // Without focus it never blinks, and a block is drawn as an outline.
        let unfocused = shown(false, false).unwrap();
        assert_eq!(unfocused.shape, CursorShape::BlockHollow);
        let bar = CursorMark {
            shape: CursorShape::Bar,
            ..block
        };
        let unfocused_bar = cursor_as_shown(Some(bar), false, true).unwrap();
        assert_eq!(unfocused_bar.shape, CursorShape::Bar);
    }
}
