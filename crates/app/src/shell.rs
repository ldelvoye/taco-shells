//! The taco shell's orange, which marks where a drag will land and what is on, chosen or being set.

use gpui::{Bounds, Canvas, Hsla, Path, PathBuilder, Pixels, Styled, canvas, point, px, rgb};

const ORANGE: u32 = 0xF0902F;

pub fn shell_orange() -> Hsla {
    let orange = rgb(ORANGE);
    orange.into()
}

/// A rounded outline over its parent in the shell's orange, with nothing painted inside it.
pub fn shell_outline(width: f32, radius: f32) -> Canvas<()> {
    let paint = move |bounds: Bounds<Pixels>, (), window: &mut gpui::Window, _: &mut gpui::App| {
        let Some(path) = rounded_outline(bounds, width, radius) else {
            return;
        };
        window.paint_path(path, shell_orange());
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
