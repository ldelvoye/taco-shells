//! Block elements and box-drawing lines, drawn as shapes that fill their cell exactly, so neighbours join without seams at any font size or line height.

use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, fill, point, px, size};

use crate::grid::CellMetrics;

/// A light line's thickness as a share of the font size. A heavy line is twice as thick.
const LIGHT_LINE_SHARE: f32 = 1.0 / 14.0;
const LIGHT_SHADE: f32 = 0.25;
const MEDIUM_SHADE: f32 = 0.5;
const DARK_SHADE: f32 = 0.75;
const UPPER_LEFT: Eighths = [0, 0, 4, 4];
const UPPER_RIGHT: Eighths = [4, 0, 4, 4];
const LOWER_LEFT: Eighths = [0, 4, 4, 4];
const LOWER_RIGHT: Eighths = [4, 4, 4, 4];

/// U+2580 to U+259F in order. The shades ░ ▒ ▓ are matched before this table and have no rectangles here.
const BLOCKS: [&[Eighths]; 32] = [
    &[[0, 0, 8, 4]],
    &[[0, 7, 8, 1]],
    &[[0, 6, 8, 2]],
    &[[0, 5, 8, 3]],
    &[[0, 4, 8, 4]],
    &[[0, 3, 8, 5]],
    &[[0, 2, 8, 6]],
    &[[0, 1, 8, 7]],
    &[[0, 0, 8, 8]],
    &[[0, 0, 7, 8]],
    &[[0, 0, 6, 8]],
    &[[0, 0, 5, 8]],
    &[[0, 0, 4, 8]],
    &[[0, 0, 3, 8]],
    &[[0, 0, 2, 8]],
    &[[0, 0, 1, 8]],
    &[[4, 0, 4, 8]],
    &[],
    &[],
    &[],
    &[[0, 0, 8, 1]],
    &[[7, 0, 1, 8]],
    &[LOWER_LEFT],
    &[LOWER_RIGHT],
    &[UPPER_LEFT],
    &[UPPER_LEFT, LOWER_LEFT, LOWER_RIGHT],
    &[UPPER_LEFT, LOWER_RIGHT],
    &[UPPER_LEFT, UPPER_RIGHT, LOWER_LEFT],
    &[UPPER_LEFT, UPPER_RIGHT, LOWER_RIGHT],
    &[UPPER_RIGHT],
    &[UPPER_RIGHT, LOWER_LEFT],
    &[UPPER_RIGHT, LOWER_LEFT, LOWER_RIGHT],
];

/// A rectangle inside a cell, in eighths of its width and height: x, y, width, height.
type Eighths = [u8; 4];
/// A rectangle in pixels: left, top, right, bottom.
type Edges = [f32; 4];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    None,
    Light,
    Heavy,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Blocks(&'static [Eighths]),
    /// The whole cell in the foreground at this opacity: ░ ▒ ▓.
    Shade(f32),
    /// The line reaching each edge from the middle: up, right, down, left.
    Lines([Weight; 4]),
    /// A rounded corner from the middle of the top or bottom edge to the middle of the left or right one.
    Arc {
        down: bool,
        right: bool,
    },
}

/// The shape a cell's text is drawn as: one block element or light, heavy or rounded box-drawing character. Dashed, double and diagonal lines, and a character with a combining mark, stay with the font.
pub fn shape_of_text(text: &str) -> Option<Shape> {
    let mut chars = text.chars();
    let glyph = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    shape_of(glyph)
}

fn shape_of(glyph: char) -> Option<Shape> {
    match glyph {
        '░' => Some(Shape::Shade(LIGHT_SHADE)),
        '▒' => Some(Shape::Shade(MEDIUM_SHADE)),
        '▓' => Some(Shape::Shade(DARK_SHADE)),
        '▀'..='▟' => {
            let index = (u32::from(glyph) - u32::from('▀')) as usize;
            Some(Shape::Blocks(BLOCKS[index]))
        }
        '╭' => Some(Shape::Arc {
            down: true,
            right: true,
        }),
        '╮' => Some(Shape::Arc {
            down: true,
            right: false,
        }),
        '╯' => Some(Shape::Arc {
            down: false,
            right: false,
        }),
        '╰' => Some(Shape::Arc {
            down: false,
            right: true,
        }),
        _ => {
            let arms = arms_of(glyph)?;
            Some(Shape::Lines(arms.map(weight)))
        }
    }
}

/// Each line's weight, up, right, down, left: 0 none, 1 light, 2 heavy.
fn arms_of(glyph: char) -> Option<[u8; 4]> {
    let arms = match glyph {
        '─' => [0, 1, 0, 1],
        '━' => [0, 2, 0, 2],
        '│' => [1, 0, 1, 0],
        '┃' => [2, 0, 2, 0],
        '┌' => [0, 1, 1, 0],
        '┍' => [0, 2, 1, 0],
        '┎' => [0, 1, 2, 0],
        '┏' => [0, 2, 2, 0],
        '┐' => [0, 0, 1, 1],
        '┑' => [0, 0, 1, 2],
        '┒' => [0, 0, 2, 1],
        '┓' => [0, 0, 2, 2],
        '└' => [1, 1, 0, 0],
        '┕' => [1, 2, 0, 0],
        '┖' => [2, 1, 0, 0],
        '┗' => [2, 2, 0, 0],
        '┘' => [1, 0, 0, 1],
        '┙' => [1, 0, 0, 2],
        '┚' => [2, 0, 0, 1],
        '┛' => [2, 0, 0, 2],
        '├' => [1, 1, 1, 0],
        '┝' => [1, 2, 1, 0],
        '┞' => [2, 1, 1, 0],
        '┟' => [1, 1, 2, 0],
        '┠' => [2, 1, 2, 0],
        '┡' => [2, 2, 1, 0],
        '┢' => [1, 2, 2, 0],
        '┣' => [2, 2, 2, 0],
        '┤' => [1, 0, 1, 1],
        '┥' => [1, 0, 1, 2],
        '┦' => [2, 0, 1, 1],
        '┧' => [1, 0, 2, 1],
        '┨' => [2, 0, 2, 1],
        '┩' => [2, 0, 1, 2],
        '┪' => [1, 0, 2, 2],
        '┫' => [2, 0, 2, 2],
        '┬' => [0, 1, 1, 1],
        '┭' => [0, 1, 1, 2],
        '┮' => [0, 2, 1, 1],
        '┯' => [0, 2, 1, 2],
        '┰' => [0, 1, 2, 1],
        '┱' => [0, 1, 2, 2],
        '┲' => [0, 2, 2, 1],
        '┳' => [0, 2, 2, 2],
        '┴' => [1, 1, 0, 1],
        '┵' => [1, 1, 0, 2],
        '┶' => [1, 2, 0, 1],
        '┷' => [1, 2, 0, 2],
        '┸' => [2, 1, 0, 1],
        '┹' => [2, 1, 0, 2],
        '┺' => [2, 2, 0, 1],
        '┻' => [2, 2, 0, 2],
        '┼' => [1, 1, 1, 1],
        '┽' => [1, 1, 1, 2],
        '┾' => [1, 2, 1, 1],
        '┿' => [1, 2, 1, 2],
        '╀' => [2, 1, 1, 1],
        '╁' => [1, 1, 2, 1],
        '╂' => [2, 1, 2, 1],
        '╃' => [2, 1, 1, 2],
        '╄' => [2, 2, 1, 1],
        '╅' => [1, 1, 2, 2],
        '╆' => [1, 2, 2, 1],
        '╇' => [2, 2, 1, 2],
        '╈' => [1, 2, 2, 2],
        '╉' => [2, 1, 2, 2],
        '╊' => [2, 2, 2, 1],
        '╋' => [2, 2, 2, 2],
        '╴' => [0, 0, 0, 1],
        '╵' => [1, 0, 0, 0],
        '╶' => [0, 1, 0, 0],
        '╷' => [0, 0, 1, 0],
        '╸' => [0, 0, 0, 2],
        '╹' => [2, 0, 0, 0],
        '╺' => [0, 2, 0, 0],
        '╻' => [0, 0, 2, 0],
        '╼' => [0, 2, 0, 1],
        '╽' => [1, 0, 2, 0],
        '╾' => [0, 1, 0, 2],
        '╿' => [2, 0, 1, 0],
        _ => return None,
    };
    Some(arms)
}

fn weight(arm: u8) -> Weight {
    match arm {
        0 => Weight::None,
        1 => Weight::Light,
        _ => Weight::Heavy,
    }
}

/// `columns` cells from `column` in `row`, with their edges on whole device pixels, so neighbouring cells share an edge exactly.
pub fn cells_bounds(
    origin: Point<Pixels>,
    cell: CellMetrics,
    column: u16,
    row: u16,
    columns: u16,
    scale: f32,
) -> Bounds<Pixels> {
    let first_column = f32::from(column);
    let end_column = first_column + f32::from(columns);
    let row = f32::from(row);
    let unsnapped_left = f32::from(origin.x) + cell.width * first_column;
    let unsnapped_right = f32::from(origin.x) + cell.width * end_column;
    let unsnapped_top = f32::from(origin.y) + cell.height * row;
    let unsnapped_bottom = unsnapped_top + cell.height;
    let left = snap(unsnapped_left, scale);
    let right = snap(unsnapped_right, scale);
    let top = snap(unsnapped_top, scale);
    let bottom = snap(unsnapped_bottom, scale);
    bounds_between(left, top, right, bottom)
}

/// A light line's thickness in pixels: a whole number of device pixels, at least one.
pub fn light_line(font_size: Pixels, scale: f32) -> f32 {
    let unrounded = f32::from(font_size) * LIGHT_LINE_SHARE * scale;
    let device_pixels = unrounded.round().max(1.0);
    device_pixels / scale
}

pub fn paint_shape(
    shape: Shape,
    cell: Bounds<Pixels>,
    color: Hsla,
    light: f32,
    scale: f32,
    window: &mut Window,
) {
    match shape {
        Shape::Blocks(rects) => {
            for eighths in rects {
                let bounds = block_bounds(cell, *eighths, scale);
                window.paint_quad(fill(bounds, color));
            }
        }
        Shape::Shade(opacity) => {
            let mut shaded = color;
            shaded.a *= opacity;
            window.paint_quad(fill(cell, shaded));
        }
        Shape::Lines(arms) => paint_lines(arms, cell, color, light, scale, window),
        Shape::Arc { down, right } => paint_arc(down, right, cell, color, light, scale, window),
    }
}

fn block_bounds(cell: Bounds<Pixels>, eighths: Eighths, scale: f32) -> Bounds<Pixels> {
    let [x, y, width, height] = eighths.map(f32::from);
    let left = f32::from(cell.origin.x);
    let top = f32::from(cell.origin.y);
    let eighth_x = f32::from(cell.size.width) / 8.0;
    let eighth_y = f32::from(cell.size.height) / 8.0;
    let x0 = snap(left + eighth_x * x, scale);
    let x1 = snap(left + eighth_x * (x + width), scale);
    let y0 = snap(top + eighth_y * y, scale);
    let y1 = snap(top + eighth_y * (y + height), scale);
    bounds_between(x0, y0, x1, y1)
}

/// Each arm runs from its edge across the joint in the middle, so arms of different weights meet without a gap.
fn paint_lines(
    arms: [Weight; 4],
    cell: Bounds<Pixels>,
    color: Hsla,
    light: f32,
    scale: f32,
    window: &mut Window,
) {
    let [up_width, right_width, down_width, left_width] = arms.map(|arm| thickness(arm, light));
    let mut widest = up_width;
    for arm_width in [right_width, down_width, left_width] {
        widest = widest.max(arm_width);
    }
    let cell_left = f32::from(cell.origin.x);
    let cell_top = f32::from(cell.origin.y);
    let cell_width = f32::from(cell.size.width);
    let cell_height = f32::from(cell.size.height);
    let cell_right = cell_left + cell_width;
    let cell_bottom = cell_top + cell_height;
    let middle_x = cell_left + cell_width / 2.0;
    let middle_y = cell_top + cell_height / 2.0;
    let joint_left = snap(middle_x - widest / 2.0, scale);
    let joint_top = snap(middle_y - widest / 2.0, scale);
    let joint_right = joint_left + widest;
    let joint_bottom = joint_top + widest;

    let mut arm_edges = [[0.0; 4]; 4];
    let mut arm_count = 0;
    if left_width > 0.0 {
        let top = snap(middle_y - left_width / 2.0, scale);
        arm_edges[arm_count] = [cell_left, top, joint_right, top + left_width];
        arm_count += 1;
    }
    if right_width > 0.0 {
        let top = snap(middle_y - right_width / 2.0, scale);
        arm_edges[arm_count] = [joint_left, top, cell_right, top + right_width];
        arm_count += 1;
    }
    if up_width > 0.0 {
        let left = snap(middle_x - up_width / 2.0, scale);
        arm_edges[arm_count] = [left, cell_top, left + up_width, joint_bottom];
        arm_count += 1;
    }
    if down_width > 0.0 {
        let left = snap(middle_x - down_width / 2.0, scale);
        arm_edges[arm_count] = [left, joint_top, left + down_width, cell_bottom];
        arm_count += 1;
    }
    let arms_drawn = &arm_edges[..arm_count];
    for_each_piece(arms_drawn, |[left, top, right, bottom]| {
        let piece = bounds_between(left, top, right, bottom);
        window.paint_quad(fill(piece, color));
    });
}

/// The area `arms` cover, as rectangles that do not overlap, so a translucent line is painted once at every pixel. Each is left, top, right, bottom.
fn for_each_piece(arms: &[Edges], mut paint: impl FnMut(Edges)) {
    let mut xs = [0.0; 8];
    let mut ys = [0.0; 8];
    for (index, [left, top, right, bottom]) in arms.iter().enumerate() {
        xs[index * 2] = *left;
        xs[index * 2 + 1] = *right;
        ys[index * 2] = *top;
        ys[index * 2 + 1] = *bottom;
    }
    let edge_count = arms.len() * 2;
    let xs = &mut xs[..edge_count];
    let ys = &mut ys[..edge_count];
    xs.sort_by(f32::total_cmp);
    ys.sort_by(f32::total_cmp);

    // Each band between two horizontal edges is painted as the stretches of it that an arm covers.
    for band in ys.windows(2) {
        let [top, bottom] = [band[0], band[1]];
        if bottom <= top {
            continue;
        }
        let middle_y = (top + bottom) / 2.0;
        let mut stretch_left = None;
        for tile in xs.windows(2) {
            let [left, right] = [tile[0], tile[1]];
            if right <= left {
                continue;
            }
            let middle_x = (left + right) / 2.0;
            let covered = arms.iter().any(|arm| {
                arm[0] <= middle_x && middle_x < arm[2] && arm[1] <= middle_y && middle_y < arm[3]
            });
            match (covered, stretch_left) {
                (true, None) => stretch_left = Some(left),
                (false, Some(start)) => {
                    paint([start, top, left, bottom]);
                    stretch_left = None;
                }
                _ => {}
            }
        }
        if let (Some(start), Some(end)) = (stretch_left, xs.last()) {
            paint([start, top, *end, bottom]);
        }
    }
}

fn paint_arc(
    down: bool,
    right: bool,
    cell: Bounds<Pixels>,
    color: Hsla,
    light: f32,
    scale: f32,
    window: &mut Window,
) {
    let cell_left = f32::from(cell.origin.x);
    let cell_top = f32::from(cell.origin.y);
    let width = f32::from(cell.size.width);
    let height = f32::from(cell.size.height);
    let radius = width.min(height) / 2.0;
    // The stroke's middle, where a straight light line's middle is, so the corner meets ─ and │ exactly.
    let line_left = snap(cell_left + width / 2.0 - light / 2.0, scale);
    let line_top = snap(cell_top + height / 2.0 - light / 2.0, scale);
    let line_x = line_left + light / 2.0;
    let line_y = line_top + light / 2.0;

    let mut toward_edge_y = -1.0;
    let mut edge_y = cell_top;
    if down {
        toward_edge_y = 1.0;
        edge_y += height;
    }
    let mut toward_edge_x = -1.0;
    let mut edge_x = cell_left;
    if right {
        toward_edge_x = 1.0;
        edge_x += width;
    }
    // ╭ and ╯ turn clockwise on screen; ╮ and ╰ turn the other way.
    let clockwise = down == right;
    let start = point(px(line_x), px(edge_y));
    let turn_start = point(px(line_x), px(line_y + toward_edge_y * radius));
    let turn_end = point(px(line_x + toward_edge_x * radius), px(line_y));
    let end = point(px(edge_x), px(line_y));
    let radii = point(px(radius), px(radius));

    let mut path = PathBuilder::stroke(px(light));
    path.move_to(start);
    path.line_to(turn_start);
    path.arc_to(radii, px(0.0), false, clockwise, turn_end);
    path.line_to(end);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn thickness(arm: Weight, light: f32) -> f32 {
    match arm {
        Weight::None => 0.0,
        Weight::Light => light,
        Weight::Heavy => light * 2.0,
    }
}

fn bounds_between(left: f32, top: f32, right: f32, bottom: f32) -> Bounds<Pixels> {
    let top_left = point(px(left), px(top));
    let extent = size(px(right - left), px(bottom - top));
    Bounds::new(top_left, extent)
}

/// `value` moved to the nearest whole device pixel.
fn snap(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_cells_share_an_edge_on_a_device_pixel_whatever_the_cell_width() {
        let origin = point(px(3.3), px(1.7));
        let cell = CellMetrics {
            width: 7.83,
            height: 17.0,
        };
        let scale = 2.0;
        for column in 0..40 {
            let this = cells_bounds(origin, cell, column, 3, 1, scale);
            let next = cells_bounds(origin, cell, column + 1, 3, 1, scale);
            let below = cells_bounds(origin, cell, column, 4, 1, scale);
            assert_eq!(this.right(), next.left(), "column {column}");
            assert_eq!(this.bottom(), below.top(), "column {column}");
            let device_left = f32::from(this.left()) * scale;
            assert_eq!(device_left, device_left.round());
        }
    }

    #[test]
    fn line_pieces_never_overlap_and_cover_exactly_the_arms() {
        // ╂ (a light horizontal across a heavy vertical), ┝, ─ and a lone ╴, as paint_lines lays them out in an 8 by 16 cell.
        let cases: [&[Edges]; 4] = [
            &[
                [0.0, 7.0, 5.0, 8.0],
                [3.0, 7.0, 8.0, 8.0],
                [3.0, 0.0, 5.0, 9.0],
                [3.0, 7.0, 5.0, 16.0],
            ],
            &[
                [3.5, 0.0, 4.5, 9.0],
                [3.5, 7.0, 4.5, 16.0],
                [3.5, 7.0, 8.0, 9.0],
            ],
            &[[0.0, 7.0, 4.5, 8.0], [3.5, 7.0, 8.0, 8.0]],
            &[[0.0, 7.0, 4.5, 8.0]],
        ];
        for arms in cases {
            let mut pieces = Vec::new();
            for_each_piece(arms, |piece| pieces.push(piece));
            // Every half-pixel point is painted once if an arm covers it, and never otherwise.
            for step_y in 0..32 {
                for step_x in 0..16 {
                    let x = f32::from(step_x as u8) / 2.0 + 0.25;
                    let y = f32::from(step_y as u8) / 2.0 + 0.25;
                    let inside = |edges: &Edges| {
                        edges[0] <= x && x < edges[2] && edges[1] <= y && y < edges[3]
                    };
                    let in_arm = arms.iter().any(inside);
                    let times_painted = pieces.iter().filter(|piece| inside(piece)).count();
                    assert_eq!(times_painted, usize::from(in_arm), "{arms:?} at {x}, {y}");
                }
            }
        }
    }
}
