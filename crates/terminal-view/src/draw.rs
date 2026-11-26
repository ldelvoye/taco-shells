//! Turning a view's rows into backgrounds, runs of text and a cursor, then painting them in the window.

use gpui::{
    App, Bounds, Font, FontStyle, FontWeight, Hsla, Pixels, Point, Rgba, StrikethroughStyle,
    TextAlign, TextRun, UnderlineStyle, Window, fill, font, point, px, size,
};
use taco_ghostty::render::{
    Cell as GridCell, CursorShape, FrameColors, FrameCursor, Row, Underline, Width,
};
use taco_protocol::Rgb;

use crate::grid::Cell;

const FAINT_ALPHA: f32 = 0.6;
const LINE_THICKNESS: f32 = 1.0;
const BAR_WIDTH: f32 = 2.0;

/// A cell's background where it differs from the terminal's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Background {
    pub column: u16,
    pub row: u16,
    pub span: u16,
    pub color: Hsla,
}

/// Cells in a row that share a style, drawn as one shaped line.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub column: u16,
    pub row: u16,
    pub text: String,
    pub color: Hsla,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<(Hsla, bool)>,
    pub strikethrough: Option<Hsla>,
    /// A wide character, drawn across two cells at its own advance.
    pub wide: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CursorMark {
    pub column: u16,
    pub row: u16,
    pub shape: CursorShape,
    pub color: Hsla,
    pub span: u16,
}

/// Everything one frame draws, in grid coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Painting {
    pub backgrounds: Vec<Background>,
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
        let mut span = 1;
        if is_wide_at(rows, column, row) {
            span = 2;
        }
        painting.cursor = Some(CursorMark {
            column,
            row,
            shape: cursor.shape,
            color: hsla(cursor_color),
            span,
        });
    }
    painting
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
        let skipped = matches!(cell.width, Width::SpacerTail | Width::SpacerHead);
        if skipped {
            continue;
        }
        let wide = cell.width == Width::Wide;
        let mut span = 1;
        if wide {
            span = 2;
        }

        let (foreground, background) = cell_colors(cell, colors, selection);
        if let Some(color) = background {
            painting.backgrounds.push(Background {
                column,
                row: y,
                span,
                color: hsla(color),
            });
        }

        let text = row.cell_text(cell);
        if text.is_empty() || cell.invisible {
            continue;
        }
        let mut color = hsla(foreground);
        if cell.faint {
            color.a *= FAINT_ALPHA;
        }
        let underline = underline_of(cell, color);
        let mut strikethrough = None;
        if cell.strikethrough {
            strikethrough = Some(color);
        }
        let run = Run {
            column,
            row: y,
            text: text.to_string(),
            color,
            bold: cell.bold,
            italic: cell.italic,
            underline,
            strikethrough,
            wide,
        };
        append(&mut painting.runs, run);
    }
}

/// The colors a cell is drawn in: inverse swaps them, and a selected cell takes the selection background.
fn cell_colors(cell: &GridCell, colors: &FrameColors, selection: Rgb) -> (Rgb, Option<Rgb>) {
    let mut foreground = colors.foreground;
    if let Some(own) = cell.foreground {
        foreground = own;
    }
    let mut background = cell.background;
    if cell.inverse {
        let mut swapped_background = colors.background;
        if let Some(own) = cell.background {
            swapped_background = own;
        }
        background = Some(foreground);
        foreground = swapped_background;
    }
    if cell.selected {
        background = Some(selection);
    }
    (foreground, background)
}

fn underline_of(cell: &GridCell, text_color: Hsla) -> Option<(Hsla, bool)> {
    if cell.underline == Underline::None {
        return None;
    }
    let mut color = text_color;
    if let Some(own) = cell.underline_color {
        color = hsla(own);
    }
    let wavy = cell.underline == Underline::Curly;
    Some((color, wavy))
}

/// Adds `run` to the last run when it continues it on the same row in the same style.
fn append(runs: &mut Vec<Run>, run: Run) {
    if let Some(last) = runs.last_mut() {
        let next_column = last.column + last.text.chars().count() as u16;
        let continues = !last.wide
            && !run.wide
            && last.row == run.row
            && next_column == run.column
            && last.color == run.color
            && last.bold == run.bold
            && last.italic == run.italic
            && last.underline == run.underline
            && last.strikethrough == run.strikethrough;
        if continues {
            last.text.push_str(&run.text);
            return;
        }
    }
    runs.push(run);
}

/// Paints `painting` with its grid's top-left corner at `origin`.
pub fn paint(
    painting: &Painting,
    origin: Point<Pixels>,
    cell: Cell,
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
        let width = cell_width * f32::from(background.span);
        let bounds = Bounds::new(
            at(background.column, background.row),
            size(width, cell_height),
        );
        window.paint_quad(fill(bounds, background.color));
    }

    for run in &painting.runs {
        let text_run = text_run_for(run, family);
        let mut force_width = Some(cell_width);
        if run.wide {
            force_width = None;
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
        let width = cell_width * f32::from(cursor.span);
        let bounds = cursor_bounds(cursor.shape, top_left, width, cell_height);
        let mut color = cursor.color;
        if cursor.shape == CursorShape::Block {
            color.a *= FAINT_ALPHA;
        }
        if cursor.shape == CursorShape::BlockHollow {
            window.paint_quad(gpui::outline(bounds, color, gpui::BorderStyle::Solid));
        } else {
            window.paint_quad(fill(bounds, color));
        }
    }
}

fn cursor_bounds(
    shape: CursorShape,
    top_left: Point<Pixels>,
    width: Pixels,
    height: Pixels,
) -> Bounds<Pixels> {
    match shape {
        CursorShape::Bar => Bounds::new(top_left, size(px(BAR_WIDTH), height)),
        CursorShape::Underline => {
            let bottom = point(top_left.x, top_left.y + height - px(BAR_WIDTH));
            Bounds::new(bottom, size(width, px(BAR_WIDTH)))
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
    if let Some((color, wavy)) = run.underline {
        underline = Some(UnderlineStyle {
            thickness: px(LINE_THICKNESS),
            color: Some(color),
            wavy,
        });
    }
    let mut strikethrough = None;
    if let Some(color) = run.strikethrough {
        strikethrough = Some(StrikethroughStyle {
            thickness: px(LINE_THICKNESS),
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
    use taco_ghostty::render::RenderState;
    use taco_protocol::TerminalSize;

    fn rgb(value: u32) -> Rgb {
        let [_, r, g, b] = value.to_be_bytes();
        Rgb { r, g, b }
    }

    #[test]
    fn cells_become_backgrounds_runs_and_a_cursor_in_the_colors_we_chose() {
        let mut terminal = Terminal::new(TerminalSize { cols: 20, rows: 2 }).unwrap();
        // Plain text, an inverse word, a hidden word, a wide character, then the cursor after it.
        terminal.write(b"ab \x1b[7minv\x1b[0m \x1b[8mhid\x1b[0m \xe6\x97\xa5");
        let mut state = RenderState::new().unwrap();
        let mut frame = state.update(&terminal).unwrap();
        let mut rows = Vec::new();
        let mut row = Row::new();
        while frame.next_row(&mut row, false).unwrap() {
            rows.push(row.clone());
        }
        let colors = frame.colors().clone();
        let cursor = frame.cursor().unwrap();
        let selection = rgb(0x3a3836);
        let painting = lay_out(&rows, cursor, &colors, selection);

        let texts: Vec<&str> = painting.runs.iter().map(|run| run.text.as_str()).collect();
        // Same-style neighbours join; the inverse word is its own run; hidden text is not drawn; the wide character stands alone.
        assert_eq!(texts, vec!["ab ", "inv", " ", " ", "日"]);
        let inverse = &painting.runs[1];
        assert_eq!(inverse.color, hsla(colors.background));
        let inverse_backgrounds: Vec<u16> = painting
            .backgrounds
            .iter()
            .map(|quad| quad.column)
            .collect();
        assert_eq!(inverse_backgrounds, vec![3, 4, 5]);
        let wide = &painting.runs[4];
        assert_eq!((wide.column, wide.wide), (11, true));

        let mark = painting.cursor.unwrap();
        assert_eq!((mark.column, mark.row, mark.span), (13, 0, 1));
    }
}
