//! The text field's element: laying its one line out, scrolled so the caret stays in view, and painting it.

use gpui::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    LayoutId, PaintQuad, Pixels, Point, ShapedLine, Style, TextAlign, TextRun, UnderlineStyle,
    Window, fill, point, px, relative, size,
};

use super::TextField;

const CARET_WIDTH: f32 = 1.;

pub(super) struct TextElement {
    pub(super) field: Entity<TextField>,
}

pub(super) struct PrepaintState {
    line: Option<ShapedLine>,
    /// The placeholder, while the field is empty.
    hint: Option<ShapedLine>,
    /// Where the whole line sits, its start possibly left of the field.
    line_bounds: Bounds<Pixels>,
    scroll: f32,
    caret: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.field.read(cx);
        let content = field.content.clone();
        let selected_range = field.selected_range.clone();
        let marked_range = field.marked_range.clone();
        let caret_offset = field.cursor_offset();
        let last_scroll = field.scroll;
        let focused = field.focus_handle.is_focused(window);
        let look = field.look.clone();
        let style = window.text_style();

        let run = TextRun {
            len: content.len(),
            font: style.font(),
            color: look.text,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match marked_range {
            Some(marked) => {
                let before = TextRun {
                    len: marked.start,
                    ..run.clone()
                };
                let underline = UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.),
                    wavy: false,
                };
                let marked_run = TextRun {
                    len: marked.end - marked.start,
                    underline: Some(underline),
                    ..run.clone()
                };
                let after = TextRun {
                    len: content.len() - marked.end,
                    ..run
                };
                let all = vec![before, marked_run, after];
                let mut kept = Vec::new();
                for piece in all {
                    if piece.len > 0 {
                        kept.push(piece);
                    }
                }
                kept
            }
            None => vec![run],
        };

        let font_size = style.font_size.to_pixels(window.rem_size());
        let mut hint = None;
        if content.is_empty()
            && let Some(placeholder) = field.placeholder.clone()
        {
            let hint_run = TextRun {
                len: placeholder.len(),
                font: style.font(),
                color: look.secondary_text,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = window
                .text_system()
                .shape_line(placeholder, font_size, &[hint_run], None);
            hint = Some(shaped);
        }
        let line = window
            .text_system()
            .shape_line(content.into(), font_size, &runs, None);

        let top = bounds.top();
        let bottom = bounds.bottom();
        let caret_in_line = f32::from(line.x_for_index(caret_offset));
        let line_width = f32::from(line.width);
        let visible = f32::from(bounds.size.width);
        // A field nobody is typing in shows its start.
        let mut scroll = 0.;
        if focused {
            scroll = scroll_showing(last_scroll, caret_in_line, line_width, visible);
        }
        let line_left = bounds.left() - px(scroll);
        let line_size = size(bounds.size.width + px(scroll), bounds.size.height);
        let line_bounds = Bounds::new(point(line_left, top), line_size);
        let mut caret = None;
        let mut selection = None;
        let typing_at = focused && selected_range.is_empty();
        let selecting = focused && !selected_range.is_empty();
        if typing_at {
            let caret_x = line_left + line.x_for_index(caret_offset);
            let caret_size = size(px(CARET_WIDTH), bottom - top);
            let caret_bounds = Bounds::new(point(caret_x, top), caret_size);
            caret = Some(fill(caret_bounds, look.text));
        }
        if selecting {
            let start = point(line_left + line.x_for_index(selected_range.start), top);
            let end = point(line_left + line.x_for_index(selected_range.end), bottom);
            let selection_bounds = Bounds::from_corners(start, end);
            selection = Some(fill(selection_bounds, look.strong_veil));
        }
        PrepaintState {
            line: Some(line),
            hint,
            line_bounds,
            scroll,
            caret,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.field.read(cx).focus_handle.clone();
        let line_bounds = prepaint.line_bounds;
        let handler = ElementInputHandler::new(line_bounds, self.field.clone());
        window.handle_input(&focus_handle, handler, cx);
        let Some(line) = prepaint.line.take() else {
            return;
        };
        // The line can start left of the field, so only what is inside the field is drawn.
        let mask = ContentMask { bounds };
        window.with_content_mask(Some(mask), |window| {
            paint_line(&line, prepaint, line_bounds.origin, window, cx);
        });
        let scroll = prepaint.scroll;
        self.field.update(cx, |field, _| {
            field.last_layout = Some(line);
            field.last_bounds = Some(line_bounds);
            field.scroll = scroll;
        });
    }
}

fn paint_line(
    line: &ShapedLine,
    prepaint: &mut PrepaintState,
    origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(selection) = prepaint.selection.take() {
        window.paint_quad(selection);
    }
    let line_height = window.line_height();
    let painted = line.paint(origin, line_height, TextAlign::Left, None, window, cx);
    if let Err(error) = painted {
        eprintln!("the text field's line did not paint: {error:#}");
    }
    if let Some(hint) = prepaint.hint.take() {
        let hint_painted = hint.paint(origin, line_height, TextAlign::Left, None, window, cx);
        if let Err(error) = hint_painted {
            eprintln!("the text field's placeholder did not paint: {error:#}");
        }
    }
    if let Some(caret) = prepaint.caret.take() {
        window.paint_quad(caret);
    }
}

/// The scroll that keeps the caret in view with the least movement.
fn scroll_showing(scroll: f32, caret: f32, line_width: f32, visible: f32) -> f32 {
    let room = visible - CARET_WIDTH;
    let mut shown = scroll;
    if caret < shown {
        shown = caret;
    }
    if caret > shown + room {
        shown = caret - room;
    }
    let furthest = (line_width - room).max(0.);
    shown.clamp(0., furthest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_caret_stays_in_view_and_the_line_never_scrolls_past_its_end() {
        let visible = 100.;
        let room = visible - CARET_WIDTH;
        // A short line never scrolls.
        assert_eq!(scroll_showing(0., 40., 60., visible), 0.);
        // The caret at a long line's end brings that end to the field's right edge.
        assert_eq!(scroll_showing(0., 300., 300., visible), 300. - room);
        // A caret already in view does not move the line.
        assert_eq!(scroll_showing(150., 200., 300., visible), 150.);
        // A caret left of the view scrolls back just enough to show it.
        assert_eq!(scroll_showing(150., 120., 300., visible), 120.);
        // A line made shorter, by a deletion, gives back the scroll it no longer needs.
        assert_eq!(scroll_showing(250., 120., 150., visible), 150. - room);
    }
}
