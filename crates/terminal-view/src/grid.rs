//! The terminal grid's measures: the cell the font gives, and how many cells fit the middle.

use gpui::{App, Pixels, Point, font, point, px};
use taco_protocol::{CellSize, TerminalSize};

const MINIMUM_COLS: u16 = 2;
const MINIMUM_ROWS: u16 = 1;
/// The family a font list's `monospace` stands for.
const PLATFORM_MONOSPACE: &str = "Menlo";
/// The width of `m` as a share of the font size, for a font that cannot be measured.
const FALLBACK_ADVANCE: f32 = 0.6;

/// One cell in window pixels. Every view shares it, since they all draw in the same font.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

impl CellMetrics {
    /// The cell size the runtime answers a program's size reports with.
    pub fn size_px(self) -> CellSize {
        let width = self.width.round();
        let height = self.height.round();
        CellSize {
            width_px: width as u32,
            height_px: height as u32,
        }
    }

    /// The top-left corner of the cell at `column` and `row`, for a grid whose top-left corner is at `origin`.
    pub fn origin_of(self, origin: Point<Pixels>, column: u16, row: u16) -> Point<Pixels> {
        let x = origin.x + px(self.width) * f32::from(column);
        let y = origin.y + px(self.height) * f32::from(row);
        point(x, y)
    }
}

/// The cell `family` gives at `size`: the advance of `m` across, ascent and descent down.
pub fn measure(family: &str, size: f32, cx: &App) -> CellMetrics {
    let text_system = cx.text_system();
    let font_id = text_system.resolve_font(&font(family.to_string()));
    let font_size = px(size);
    let mut width = size * FALLBACK_ADVANCE;
    if let Ok(advance) = text_system.advance(font_id, font_size, 'm') {
        width = f32::from(advance.width);
    }
    let ascent = f32::from(text_system.ascent(font_id, font_size));
    // font-kit reports the descent below the baseline as a negative number.
    let descent = f32::from(text_system.descent(font_id, font_size)).abs();
    let height = (ascent + descent).ceil();
    CellMetrics { width, height }
}

/// The grid that fits `width` by `height` pixels: whole cells, at least 2 columns and 1 row.
pub fn grid_that_fits(width: f32, height: f32, cell: CellMetrics) -> TerminalSize {
    let columns = (width / cell.width).floor();
    let rows = (height / cell.height).floor();
    let cols = at_least(columns, MINIMUM_COLS);
    let rows = at_least(rows, MINIMUM_ROWS);
    TerminalSize { cols, rows }
}

/// The family to draw with from a CSS-style list such as `Menlo, Monaco, 'Courier New', monospace`: the first installed one.
pub fn font_family(list: &str, installed: &[String]) -> String {
    for entry in list.split(',') {
        let trimmed = entry.trim();
        let name = trimmed.trim_matches(|character| character == '\'' || character == '"');
        if name.eq_ignore_ascii_case("monospace") {
            return PLATFORM_MONOSPACE.to_string();
        }
        let found = installed
            .iter()
            .find(|family| family.eq_ignore_ascii_case(name));
        if let Some(family) = found {
            return family.clone();
        }
    }
    PLATFORM_MONOSPACE.to_string()
}

// `as u16` saturates, so only NaN and counts below the minimum need handling.
fn at_least(count: f32, minimum: u16) -> u16 {
    if count.is_nan() || count < f32::from(minimum) {
        return minimum;
    }
    count as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_never_drops_below_two_columns_and_one_row() {
        let cell = CellMetrics {
            width: 7.5,
            height: 15.0,
        };
        assert_eq!(
            grid_that_fits(10.0, 5.0, cell),
            TerminalSize { cols: 2, rows: 1 }
        );
        assert_eq!(
            grid_that_fits(0.0, 0.0, cell),
            TerminalSize { cols: 2, rows: 1 }
        );
    }

    #[test]
    fn the_font_is_the_first_installed_family_in_the_list() {
        let installed = vec!["Monaco".to_string(), "Courier New".to_string()];
        let default_list = "Menlo, Monaco, 'Courier New', monospace";
        assert_eq!(font_family(default_list, &installed), "Monaco");
        // Matched whatever the case, and named as the system names it.
        assert_eq!(
            font_family("\"courier new\", Monaco", &installed),
            "Courier New"
        );
        assert_eq!(font_family("Nonexistent, monospace", &installed), "Menlo");
    }
}
