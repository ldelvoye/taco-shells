//! The taco shell's yellow and orange, which mark where a drag will land.

use gpui::{
    Background, Bounds, Canvas, Hsla, Path, PathBuilder, Pixels, Styled, canvas, linear_color_stop,
    linear_gradient, point, px, rgb,
};

const YELLOW: u32 = 0xFFC24A;
const ORANGE: u32 = 0xF0902F;

pub fn shell_orange() -> Hsla {
    let orange = rgb(ORANGE);
    orange.into()
}

fn shell_yellow() -> Hsla {
    let yellow = rgb(YELLOW);
    yellow.into()
}

/// The shell's yellow running to orange along `angle` (degrees, 0 pointing up), as a fill.
pub fn shell_gradient(angle: f32) -> Background {
    let from = linear_color_stop(shell_yellow(), 0.);
    let to = linear_color_stop(shell_orange(), 1.);
    linear_gradient(angle, from, to)
}

/// A rounded outline over its parent, yellow running to orange along `angle` (degrees, 0 pointing up), with nothing painted inside it.
pub fn shell_outline(width: f32, radius: f32, angle: f32) -> Canvas<()> {
    let paint = move |bounds: Bounds<Pixels>, (), window: &mut gpui::Window, _: &mut gpui::App| {
        let Some(path) = rounded_outline(bounds, width, radius) else {
            return;
        };
        let shell = shell_gradient(angle);
        window.paint_path(path, shell);
    };
    canvas(|_, _, _| (), paint)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

// The stroke's middle runs half its width in from the edge, so the whole stroke stays inside the bounds.
fn rounded_outline(bounds: Bounds<Pixels>, width: f32, radius: f32) -> Option<Path> {
    let half = width / 2.;
    let left = f32::from(bounds.left()) + half;
    let top = f32::from(bounds.top()) + half;
    let right = f32::from(bounds.right()) - half;
    let bottom = f32::from(bounds.bottom()) - half;
    let corner = (radius - half).max(0.);
    let radii = point(px(corner), px(corner));
    let at = |x: f32, y: f32| point(px(x), px(y));
    let mut builder = PathBuilder::stroke(px(width));
    builder.move_to(at(left + corner, top));
    builder.line_to(at(right - corner, top));
    builder.arc_to(radii, px(0.), false, true, at(right, top + corner));
    builder.line_to(at(right, bottom - corner));
    builder.arc_to(radii, px(0.), false, true, at(right - corner, bottom));
    builder.line_to(at(left + corner, bottom));
    builder.arc_to(radii, px(0.), false, true, at(left, bottom - corner));
    builder.line_to(at(left, top + corner));
    builder.arc_to(radii, px(0.), false, true, at(left + corner, top));
    builder.close();
    let built = builder.build();
    built.ok()
}
