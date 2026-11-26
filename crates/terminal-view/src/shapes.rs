//! Block elements and box-drawing lines, drawn as shapes that fill their cell exactly, so neighbours join without seams at any font size or line height.

use gpui::{Bounds, Hsla, PathBuilder, Pixels, Point, Window, fill, point, px, size};

/// A light line's thickness as a share of the font size. A heavy line is twice as thick.
const LIGHT_LINE_SHARE: f32 = 1.0 / 14.0;
const LIGHT_SHADE: f32 = 0.25;
const MEDIUM_SHADE: f32 = 0.5;
const DARK_SHADE: f32 = 0.75;

/// A rectangle inside a cell, in eighths of its width and height: x, y, width, height.
type Eighths = [u8; 4];

const UPPER_LEFT: Eighths = [0, 0, 4, 4];
const UPPER_RIGHT: Eighths = [4, 0, 4, 4];
const LOWER_LEFT: Eighths = [0, 4, 4, 4];
const LOWER_RIGHT: Eighths = [4, 4, 4, 4];

/// U+2580 to U+259F in order. The three shades are not rectangles and have none.
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

/// The shape a character is drawn as, for the block elements and the box-drawing characters handled here. Dashed, double and diagonal lines are left to the font.
pub fn shape_of(glyph: char) -> Option<Shape> {
    let code = u32::from(glyph);
    match code {
        0x2591 => Some(Shape::Shade(LIGHT_SHADE)),
        0x2592 => Some(Shape::Shade(MEDIUM_SHADE)),
        0x2593 => Some(Shape::Shade(DARK_SHADE)),
        0x2580..=0x259F => {
            let index = (code - 0x2580) as usize;
            Some(Shape::Blocks(BLOCKS[index]))
        }
        0x256D => Some(Shape::Arc {
            down: true,
            right: true,
        }),
        0x256E => Some(Shape::Arc {
            down: true,
            right: false,
        }),
        0x256F => Some(Shape::Arc {
            down: false,
            right: false,
        }),
        0x2570 => Some(Shape::Arc {
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

/// The cell at `column`, `row` with its edges on whole device pixels, so a cell and its neighbour share an edge exactly.
pub fn cell_bounds(
    origin: Point<Pixels>,
    cell_width: f32,
    cell_height: f32,
    column: u16,
    row: u16,
    scale: f32,
) -> Bounds<Pixels> {
    let column = f32::from(column);
    let row = f32::from(row);
    let left = snap(f32::from(origin.x) + cell_width * column, scale);
    let right = snap(f32::from(origin.x) + cell_width * (column + 1.0), scale);
    let top = snap(f32::from(origin.y) + cell_height * row, scale);
    let bottom = snap(f32::from(origin.y) + cell_height * (row + 1.0), scale);
    Bounds::new(
        point(px(left), px(top)),
        size(px(right - left), px(bottom - top)),
    )
}

pub fn paint_shape(
    shape: Shape,
    cell: Bounds<Pixels>,
    color: Hsla,
    font_size: Pixels,
    scale: f32,
    window: &mut Window,
) {
    let light_device_pixels = (f32::from(font_size) * LIGHT_LINE_SHARE * scale).round();
    let light = light_device_pixels.max(1.0) / scale;
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
    Bounds::new(point(px(x0), px(y0)), size(px(x1 - x0), px(y1 - y0)))
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
    let [up, right, down, left] = arms.map(|arm| thickness(arm, light));
    let widest = up.max(right).max(down).max(left);
    let cell_left = f32::from(cell.origin.x);
    let cell_top = f32::from(cell.origin.y);
    let cell_right = cell_left + f32::from(cell.size.width);
    let cell_bottom = cell_top + f32::from(cell.size.height);
    let middle_x = cell_left + f32::from(cell.size.width) / 2.0;
    let middle_y = cell_top + f32::from(cell.size.height) / 2.0;
    let joint_left = snap(middle_x - widest / 2.0, scale);
    let joint_top = snap(middle_y - widest / 2.0, scale);
    let joint_right = joint_left + widest;
    let joint_bottom = joint_top + widest;

    let mut rects = Vec::with_capacity(4);
    if left > 0.0 {
        let top = snap(middle_y - left / 2.0, scale);
        rects.push((cell_left, top, joint_right, top + left));
    }
    if right > 0.0 {
        let top = snap(middle_y - right / 2.0, scale);
        rects.push((joint_left, top, cell_right, top + right));
    }
    if up > 0.0 {
        let x = snap(middle_x - up / 2.0, scale);
        rects.push((x, cell_top, x + up, joint_bottom));
    }
    if down > 0.0 {
        let x = snap(middle_x - down / 2.0, scale);
        rects.push((x, joint_top, x + down, cell_bottom));
    }
    for (x0, y0, x1, y1) in rects {
        let bounds = Bounds::new(point(px(x0), px(y0)), size(px(x1 - x0), px(y1 - y0)));
        window.paint_quad(fill(bounds, color));
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
    let width = f32::from(cell.size.width);
    let height = f32::from(cell.size.height);
    let radius = width.min(height) / 2.0;
    // The stroke's middle, placed so its edges land on whole device pixels like a straight light line's.
    let line_x = snap(f32::from(cell.origin.x) + width / 2.0 - light / 2.0, scale) + light / 2.0;
    let line_y = snap(f32::from(cell.origin.y) + height / 2.0 - light / 2.0, scale) + light / 2.0;

    let mut toward_edge_y = -1.0;
    let mut edge_y = f32::from(cell.origin.y);
    if down {
        toward_edge_y = 1.0;
        edge_y += height;
    }
    let mut toward_edge_x = -1.0;
    let mut edge_x = f32::from(cell.origin.x);
    if right {
        toward_edge_x = 1.0;
        edge_x += width;
    }
    // Clockwise on screen for ╭ and ╯, whose two edges lie on the same side of the turn.
    let clockwise = down == right;

    let mut path = PathBuilder::stroke(px(light));
    path.move_to(point(px(line_x), px(edge_y)));
    path.line_to(point(px(line_x), px(line_y + toward_edge_y * radius)));
    path.arc_to(
        point(px(radius), px(radius)),
        px(0.0),
        false,
        clockwise,
        point(px(line_x + toward_edge_x * radius), px(line_y)),
    );
    path.line_to(point(px(edge_x), px(line_y)));
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

fn snap(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neighbouring_cells_share_an_edge_on_a_device_pixel_whatever_the_cell_width() {
        let origin = point(px(3.3), px(1.7));
        let scale = 2.0;
        for column in 0..40 {
            let this = cell_bounds(origin, 7.83, 17.0, column, 3, scale);
            let next = cell_bounds(origin, 7.83, 17.0, column + 1, 3, scale);
            let below = cell_bounds(origin, 7.83, 17.0, column, 4, scale);
            assert_eq!(this.right(), next.left(), "column {column}");
            assert_eq!(this.bottom(), below.top(), "column {column}");
            let device_left = f32::from(this.left()) * scale;
            assert_eq!(device_left, device_left.round());
        }
    }
}
