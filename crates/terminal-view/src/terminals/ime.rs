//! The view answers an input method with an empty, caret-only text field: the shell owns the line being edited.

use std::ops::Range;

use gpui::{Bounds, Context, EntityInputHandler, Pixels, Point, UTF16Selection, Window, px, size};
use taco_ghostty::input::Scroll;

use super::Terminals;

impl EntityInputHandler for Terminals {
    fn text_for_range(
        &mut self,
        _range: Range<usize>,
        _adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<Range<usize>> {
        let marked = self.marked_text.as_ref()?;
        let length = marked.encode_utf16().count();
        Some(0..length)
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut Context<Terminals>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        self.marked_text = None;
        self.blink_on = true;
        if let Some(view) = self.views.on_screen_view() {
            let _ = view.scroll(Scroll::Bottom);
        }
        self.views.write_on_screen(text.as_bytes().to_vec());
        self.send_requests();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Terminals>,
    ) {
        let mut marked = None;
        if !new_text.is_empty() {
            marked = Some(new_text.to_string());
        }
        self.marked_text = marked;
        cx.notify();
    }

    /// Where an input method puts its candidate window: at the cursor.
    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<Bounds<Pixels>> {
        let (column, row) = self.cursor_cell?;
        let width = px(self.cell.width);
        let height = px(self.cell.height);
        let top_left = self.cell.origin_of(element_bounds.origin, column, row);
        let cell_size = size(width, height);
        Some(Bounds::new(top_left, cell_size))
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Terminals>,
    ) -> Option<usize> {
        None
    }
}
