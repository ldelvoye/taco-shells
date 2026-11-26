use std::ops::Range;

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, GlobalElementId,
    InteractiveElement, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement, Pixels, Point, Render, ShapedLine,
    Style, Styled, TextAlign, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill,
    point, px, relative, size,
};

use crate::look::Look;

const KEY_CONTEXT: &str = "TextField";
const FIELD_HEIGHT: f32 = 24.;
const FIELD_PADDING: f32 = 6.;
const FIELD_RADIUS: f32 = 6.;
const CARET_WIDTH: f32 = 1.;

actions!(
    text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit,
    ]
);

/// What the field tells its owner.
pub enum TextFieldEvent {
    /// The text changed, by typing, a paste, a cut or a deletion.
    Edited(String),
    /// Enter was pressed.
    Submitted,
}

pub struct TextField {
    focus_handle: FocusHandle,
    look: Look,
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
}

struct TextElement {
    field: Entity<TextField>,
}

struct PrepaintState {
    line: Option<ShapedLine>,
    caret: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl EventEmitter<TextFieldEvent> for TextField {}

impl TextField {
    /// A field holding `text`, all of it selected.
    pub fn new(text: &str, look: Look, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            look,
            content: text.to_string(),
            selected_range: 0..text.len(),
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
        }
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let target = self.previous_boundary(self.cursor_offset());
            self.move_to(target, window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx);
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let target = self.next_boundary(self.selected_range.end);
            self.move_to(target, window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.previous_boundary(self.cursor_offset());
        self.select_to(target, window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.next_boundary(self.cursor_offset());
        self.select_to(target, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx);
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let target = self.previous_boundary(self.cursor_offset());
            self.select_to(target, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let target = self.next_boundary(self.cursor_offset());
            self.select_to(target, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn submit(&mut self, _: &Submit, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(TextFieldEvent::Submitted);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        let Some(text) = item.text() else {
            return;
        };
        let line = one_line(&text);
        self.replace_text_in_range(None, &line, window, cx);
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        let selected = self.content[self.selected_range.clone()].to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(selected));
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            return;
        }
        let selected = self.content[self.selected_range.clone()].to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(selected));
        self.replace_text_in_range(None, "", window, cx);
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        let target = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.select_to(target, window, cx);
        } else {
            self.move_to(target, window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_selecting {
            return;
        }
        let target = self.index_for_mouse_position(event.position);
        self.select_to(target, window, cx);
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        let x = position.x - bounds.left();
        line.closest_index_for_x(x)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        let before = &self.content[..offset];
        let mut chars = before.char_indices();
        match chars.next_back() {
            Some((index, _)) => index,
            None => 0,
        }
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let after = &self.content[offset..];
        let mut chars = after.chars();
        match chars.next() {
            Some(next) => offset + next.len_utf8(),
            None => offset,
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn replaced_range(&self, range_utf16: Option<Range<usize>>) -> Range<usize> {
        if let Some(given) = range_utf16 {
            return self.range_from_utf16(&given);
        }
        if let Some(marked) = self.marked_range.clone() {
            return marked;
        }
        self.selected_range.clone()
    }

    fn splice(&mut self, range: &Range<usize>, new_text: &str, cx: &mut Context<Self>) {
        let mut next = String::new();
        next.push_str(&self.content[..range.start]);
        next.push_str(new_text);
        next.push_str(&self.content[range.end..]);
        self.content = next;
        let edited = TextFieldEvent::Edited(self.content.clone());
        cx.emit(edited);
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        adjusted_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        let marked = self.marked_range.as_ref()?;
        Some(self.range_to_utf16(marked))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.replaced_range(range_utf16);
        self.splice(&range, new_text, cx);
        let caret = range.start + new_text.len();
        self.selected_range = caret..caret;
        self.selection_reversed = false;
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.replaced_range(range_utf16);
        self.splice(&range, new_text, cx);
        let inserted_end = range.start + new_text.len();
        if new_text.is_empty() {
            self.marked_range = None;
        } else {
            self.marked_range = Some(range.start..inserted_end);
        }
        match new_selected_range_utf16 {
            Some(selected) => {
                let insertion_start_utf16 = self.offset_to_utf16(range.start);
                let inserted_length_utf16 = new_text.encode_utf16().count();
                let selected_start =
                    insertion_start_utf16 + selected.start.min(inserted_length_utf16);
                let selected_end = insertion_start_utf16 + selected.end.min(inserted_length_utf16);
                let selected_utf16 = selected_start..selected_end;
                self.selected_range = self.range_from_utf16(&selected_utf16);
            }
            None => {
                self.selected_range = inserted_end..inserted_end;
            }
        }
        self.selection_reversed = false;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.content.as_str() {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let start = point(
            bounds.left() + last_layout.x_for_index(range.start),
            bounds.top(),
        );
        let end = point(
            bounds.left() + last_layout.x_for_index(range.end),
            bounds.bottom(),
        );
        Some(Bounds::from_corners(start, end))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.content.as_str() {
            return None;
        }
        let utf8_index = last_layout.closest_index_for_x(line_point.x);
        Some(self.offset_to_utf16(utf8_index))
    }
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
        let line = window
            .text_system()
            .shape_line(content.into(), font_size, &runs, None);

        let top = bounds.top();
        let bottom = bounds.bottom();
        let mut caret = None;
        let mut selection = None;
        if selected_range.is_empty() {
            let caret_x = bounds.left() + line.x_for_index(caret_offset);
            let caret_size = size(px(CARET_WIDTH), bottom - top);
            let caret_bounds = Bounds::new(point(caret_x, top), caret_size);
            caret = Some(fill(caret_bounds, look.text));
        } else {
            let start = point(bounds.left() + line.x_for_index(selected_range.start), top);
            let end = point(bounds.left() + line.x_for_index(selected_range.end), bottom);
            let selection_bounds = Bounds::from_corners(start, end);
            selection = Some(fill(selection_bounds, look.strong_veil));
        }
        PrepaintState {
            line: Some(line),
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
        let handler = ElementInputHandler::new(bounds, self.field.clone());
        window.handle_input(&focus_handle, handler, cx);
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        let line_height = window.line_height();
        let painted = line.paint(
            bounds.origin,
            line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
        if let Err(error) = painted {
            eprintln!("the text field's line did not paint: {error:#}");
        }
        if focus_handle.is_focused(window)
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }
        self.field.update(cx, |field, _| {
            field.last_layout = Some(line);
            field.last_bounds = Some(bounds);
        });
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = self.look.selected_card;
        let border = self.look.hairline;
        let focus_handle = self.focus_handle.clone();
        let line = TextElement { field: cx.entity() };
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .flex()
            .items_center()
            .h(px(FIELD_HEIGHT))
            .px(px(FIELD_PADDING))
            .overflow_hidden()
            .rounded(px(FIELD_RADIUS))
            .border_1()
            .border_color(border)
            .bg(background)
            .child(line)
    }
}

/// Binds the field's keys under its own key context. Called once when the app starts.
pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("left", Left, context),
        KeyBinding::new("right", Right, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("cmd-left", Home, context),
        KeyBinding::new("home", Home, context),
        KeyBinding::new("cmd-right", End, context),
        KeyBinding::new("end", End, context),
        KeyBinding::new("cmd-a", SelectAll, context),
        KeyBinding::new("cmd-c", Copy, context),
        KeyBinding::new("cmd-x", Cut, context),
        KeyBinding::new("cmd-v", Paste, context),
        KeyBinding::new("enter", Submit, context),
    ]);
}

fn one_line(text: &str) -> String {
    let is_break = |c: char| c == '\r' || c == '\n';
    let trimmed = text.trim_matches(is_break);
    let mut joined = String::new();
    let mut in_break = false;
    for c in trimmed.chars() {
        if is_break(c) {
            if !in_break {
                joined.push(' ');
            }
            in_break = true;
        } else {
            joined.push(c);
            in_break = false;
        }
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_turns_each_run_of_line_breaks_into_one_space() {
        assert_eq!(one_line("a\nb"), "a b");
        assert_eq!(one_line("a\r\n\r\nb"), "a b");
        assert_eq!(one_line("name"), "name");
        assert_eq!(one_line("name\n"), "name");
        assert_eq!(one_line("\r\nname\r\n"), "name");
    }
}
