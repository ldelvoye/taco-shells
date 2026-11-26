//! One terminal's view: a silent copy of the runtime's terminal, fed in the order the terminal channel sends.

use taco_ghostty::input::{CellPoint, KeyEncoder, KeyPress, MouseEncoder, MouseInput, Scroll};
use taco_ghostty::render::{CursorShape, Dirty, RenderState, Row};
use taco_ghostty::{Error, Step, Terminal};
use taco_protocol::{Rgb, TerminalColors, TerminalEvent};

use crate::draw::{Painting, lay_out};
use crate::links::url_at;

/// What every view's terminal is set to, from the settings and the theme.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewConfig {
    pub scrollback: u32,
    pub colors: TerminalColors,
    /// The cursor until a program chooses its own.
    pub cursor_shape: CursorShape,
    pub cursor_blink: bool,
}

/// What an event did to the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Not for this view: before its `Attached`, or for an attachment it does not hold.
    Ignored,
    Changed,
    /// The shell exited and the runtime let the terminal go; there is nothing more to show.
    Ended,
}

/// One frame of the view: what to draw and the background under it.
pub struct Drawn {
    pub painting: Painting,
    pub background: Rgb,
}

/// A view that has asked to attach, and follows the runtime once `Attached` comes back with its number.
pub struct View {
    attachment: u64,
    /// None until `Attached` arrives.
    terminal: Option<Terminal>,
    history_pending: bool,
    render: RenderState,
    /// The viewport as last read, refreshed a dirty row at a time.
    rows: Vec<Row>,
    /// Remembers the last cell it reported, so it belongs to one terminal.
    mouse: MouseEncoder,
    /// The text selected when the selection was made. Output that changes it, by pruning its rows from the scrollback or writing over them, ends the selection.
    selected_text: Option<String>,
    config: ViewConfig,
}

impl View {
    pub fn new(attachment: u64, config: ViewConfig) -> Result<View, Error> {
        let render = RenderState::new()?;
        let mouse = MouseEncoder::new()?;
        Ok(View {
            attachment,
            terminal: None,
            history_pending: false,
            render,
            rows: Vec::new(),
            mouse,
            selected_text: None,
            config,
        })
    }

    pub fn apply(&mut self, event: TerminalEvent) -> Result<Applied, Error> {
        match event {
            TerminalEvent::Attached {
                attachment, screen, ..
            } => {
                if attachment != self.attachment {
                    return Ok(Applied::Ignored);
                }
                let mut terminal = Terminal::restore(&screen)?;
                configure_terminal(&mut terminal, &self.config)?;
                self.terminal = Some(terminal);
                self.history_pending = true;
                Ok(Applied::Changed)
            }
            TerminalEvent::History {
                attachment,
                part,
                last,
                ..
            } => {
                let Some(terminal) = self.terminal_for(attachment) else {
                    return Ok(Applied::Ignored);
                };
                terminal.push_history(&part, last);
                Ok(Applied::Changed)
            }
            TerminalEvent::Output { data, .. } => {
                let Some(terminal) = self.terminal.as_mut() else {
                    return Ok(Applied::Ignored);
                };
                terminal.write(&data);
                self.end_a_stale_selection()?;
                Ok(Applied::Changed)
            }
            TerminalEvent::Resized { size, .. } => {
                let Some(terminal) = self.terminal.as_mut() else {
                    return Ok(Applied::Ignored);
                };
                // Pixel sizes matter only for answering size reports, which the runtime does.
                terminal.resize(size, 0, 0)?;
                self.end_a_stale_selection()?;
                Ok(Applied::Changed)
            }
            TerminalEvent::Cleared { .. } => {
                let Some(terminal) = self.terminal.as_mut() else {
                    return Ok(Applied::Ignored);
                };
                terminal.clear()?;
                self.end_a_stale_selection()?;
                Ok(Applied::Changed)
            }
            TerminalEvent::End { .. } => Ok(Applied::Ended),
        }
    }

    /// Applies at most one page of restored history; false once there is none left.
    pub fn step_history(&mut self) -> Result<bool, Error> {
        if !self.history_pending {
            return Ok(false);
        }
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(false);
        };
        let step = terminal.step_history()?;
        self.history_pending = step != Step::Finished;
        Ok(self.history_pending)
    }

    /// The viewport laid out for drawing, with the rows that changed read again. None before `Attached`.
    pub fn draw(&mut self, selection: Rgb) -> Result<Option<Drawn>, Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(None);
        };
        let mut frame = self.render.update(terminal)?;
        let dirty = frame.dirty()?;
        let size = frame.size()?;
        let rows = usize::from(size.rows);
        let resized = self.rows.len() != rows;
        if resized {
            self.rows.resize_with(rows, Row::new);
        }
        let only_dirty = dirty != Dirty::Full && !resized;
        let mut row = Row::new();
        while frame.next_row(&mut row, only_dirty)? {
            let index = usize::from(row.y);
            if index < self.rows.len() {
                std::mem::swap(&mut self.rows[index], &mut row);
            }
        }
        let colors = frame.colors().clone();
        let cursor = frame.cursor()?;
        frame.clean()?;
        let painting = lay_out(&self.rows, cursor, &colors, selection);
        Ok(Some(Drawn {
            painting,
            background: colors.background,
        }))
    }

    /// What `press` sends to the program, under its current modes. Empty before `Attached`.
    pub fn key(&mut self, keys: &mut KeyEncoder, press: &KeyPress) -> Result<Vec<u8>, Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(Vec::new());
        };
        let bytes = keys.encode(terminal, press)?;
        // Typing brings the view back to the newest output.
        if !bytes.is_empty() {
            terminal.scroll(Scroll::Bottom)?;
        }
        Ok(bytes)
    }

    pub fn paste_bytes(&mut self, text: &str) -> Result<Vec<u8>, Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(Vec::new());
        };
        let bytes = terminal.paste_bytes(text)?;
        if !bytes.is_empty() {
            terminal.scroll(Scroll::Bottom)?;
        }
        Ok(bytes)
    }

    pub fn is_attached(&self) -> bool {
        self.terminal.is_some()
    }

    pub fn focus_report(&self, focused: bool) -> Result<Option<Vec<u8>>, Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(None);
        };
        terminal.focus_report(focused)
    }

    pub fn tracks_mouse(&self) -> Result<bool, Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(false);
        };
        terminal.mouse_tracking()
    }

    pub fn wheel_sends_arrows(&self) -> Result<bool, Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(false);
        };
        terminal.wheel_sends_arrows()
    }

    pub fn mouse(&mut self, input: &MouseInput) -> Result<Vec<u8>, Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(Vec::new());
        };
        self.mouse.encode(terminal, input)
    }

    pub fn scroll(&mut self, scroll: Scroll) -> Result<(), Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        terminal.scroll(scroll)
    }

    pub fn select(&mut self, from: CellPoint, to: CellPoint) -> Result<(), Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        terminal.select(from, to)?;
        self.remember_selection()
    }

    pub fn select_word(&mut self, at: CellPoint) -> Result<(), Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        terminal.select_word(at)?;
        self.remember_selection()
    }

    pub fn select_line(&mut self, at: CellPoint) -> Result<(), Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        terminal.select_line(at)?;
        self.remember_selection()
    }

    pub fn clear_selection(&mut self) -> Result<(), Error> {
        self.selected_text = None;
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        terminal.clear_selection()
    }

    /// The selected text, as it is copied.
    pub fn selection(&self) -> Option<String> {
        let selected = self.selected_text.as_ref()?;
        let empty = selected.is_empty();
        if empty {
            return None;
        }
        Some(selected.clone())
    }

    /// The link at a cell: one a program attached to it, or else a URL written across it.
    pub fn link_at(&self, at: CellPoint) -> Result<Option<String>, Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(None);
        };
        let attached = terminal.hyperlink_at(at)?;
        if attached.is_some() {
            return Ok(attached);
        }
        let Some(row) = self.rows.get(usize::from(at.y)) else {
            return Ok(None);
        };
        Ok(url_at(row, at.x))
    }

    fn remember_selection(&mut self) -> Result<(), Error> {
        let Some(terminal) = self.terminal.as_ref() else {
            return Ok(());
        };
        self.selected_text = terminal.selection_text()?;
        Ok(())
    }

    fn end_a_stale_selection(&mut self) -> Result<(), Error> {
        let Some(remembered) = self.selected_text.as_ref() else {
            return Ok(());
        };
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        let now = terminal.selection_text()?;
        if now.as_ref() != Some(remembered) {
            terminal.clear_selection()?;
            self.selected_text = None;
        }
        Ok(())
    }

    fn terminal_for(&mut self, attachment: u64) -> Option<&mut Terminal> {
        if attachment != self.attachment {
            return None;
        }
        self.terminal.as_mut()
    }

    /// New settings or theme colors, applied to the terminal at once if it has arrived.
    pub fn configure(&mut self, config: ViewConfig) -> Result<(), Error> {
        self.config = config;
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(());
        };
        configure_terminal(terminal, &self.config)
    }
}

fn configure_terminal(terminal: &mut Terminal, config: &ViewConfig) -> Result<(), Error> {
    let scrollback = config.scrollback as usize;
    terminal.set_scrollback(scrollback)?;
    terminal.set_colors(&config.colors)?;
    terminal.set_cursor_defaults(config.cursor_shape, config.cursor_blink)
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_ghostty::Snapshot;
    use taco_protocol::{TerminalId, TerminalSize};

    const SIZE: TerminalSize = TerminalSize { cols: 20, rows: 4 };
    const GREY: Rgb = Rgb {
        r: 0x80,
        g: 0x80,
        b: 0x80,
    };

    fn config() -> ViewConfig {
        ViewConfig {
            scrollback: 1_000,
            colors: TerminalColors {
                foreground: GREY,
                background: GREY,
                cursor: GREY,
                cursor_text: GREY,
                selection: GREY,
                ansi: [GREY; 16],
            },
            cursor_shape: CursorShape::Block,
            cursor_blink: false,
        }
    }

    fn id() -> TerminalId {
        TerminalId("t1".to_string())
    }

    fn output(text: &str) -> TerminalEvent {
        TerminalEvent::Output {
            id: id(),
            data: text.as_bytes().to_vec(),
        }
    }

    fn attached(attachment: u64, source: &Terminal) -> (TerminalEvent, Vec<Vec<u8>>) {
        let snapshot = Snapshot::take(source).unwrap();
        let history: Vec<Vec<u8>> = snapshot.history().map(|part| part.to_vec()).collect();
        let event = TerminalEvent::Attached {
            id: id(),
            attachment,
            size: SIZE,
            screen: snapshot.screen().to_vec(),
        };
        (event, history)
    }

    fn history(attachment: u64, parts: Vec<Vec<u8>>) -> Vec<TerminalEvent> {
        let last_index = parts.len().saturating_sub(1);
        let events = parts
            .into_iter()
            .enumerate()
            .map(|(index, part)| TerminalEvent::History {
                id: id(),
                attachment,
                part,
                last: index == last_index,
            });
        events.collect()
    }

    /// Each row's text, read the way a frame is drawn.
    fn texts(view: &mut View) -> Vec<String> {
        view.draw(GREY).unwrap().unwrap();
        view.rows.iter().map(|row| row.text.clone()).collect()
    }

    #[test]
    fn nothing_reaches_a_view_before_its_attached_or_for_another_attachment() {
        let mut source = Terminal::new(SIZE).unwrap();
        source.write(b"from the snapshot");
        let mut view = View::new(2, config()).unwrap();

        assert_eq!(view.apply(output("too early")).unwrap(), Applied::Ignored);
        // The answer to an earlier attach, still on its way when this view was made.
        let (stale, _) = attached(1, &source);
        assert_eq!(view.apply(stale).unwrap(), Applied::Ignored);
        assert!(view.draw(GREY).unwrap().is_none());

        let (own, parts) = attached(2, &source);
        assert_eq!(view.apply(own).unwrap(), Applied::Changed);
        for stale_part in history(1, parts) {
            assert_eq!(view.apply(stale_part).unwrap(), Applied::Ignored);
        }
        assert_eq!(view.apply(output(" ok")).unwrap(), Applied::Changed);
        assert_eq!(texts(&mut view)[0], "from the snapshot ok");
    }

    #[test]
    fn restored_history_fills_in_a_page_per_step_while_output_keeps_coming() {
        let mut source = Terminal::new(SIZE).unwrap();
        source.set_scrollback(10_000).unwrap();
        for line in 0..5_000 {
            source.write(format!("line {line}\r\n").as_bytes());
        }
        let mut view = View::new(1, config()).unwrap();
        let (event, parts) = attached(1, &source);
        view.apply(event).unwrap();
        for part in history(1, parts) {
            view.apply(part).unwrap();
        }

        let mut steps = 0;
        while view.step_history().unwrap() {
            steps += 1;
            view.apply(output("live\r\n")).unwrap();
        }
        assert!(steps > 1, "a long history should take more than one step");
        assert_eq!(texts(&mut view)[2], "live");
    }

    #[test]
    fn resize_and_clear_apply_where_they_come_in_the_output() {
        let source = Terminal::new(SIZE).unwrap();
        let mut view = View::new(1, config()).unwrap();
        let (event, _) = attached(1, &source);
        view.apply(event).unwrap();

        view.apply(output("one\r\ntwo\r\nprompt $")).unwrap();
        view.apply(TerminalEvent::Cleared { id: id() }).unwrap();
        assert_eq!(texts(&mut view), vec!["prompt $", "", "", ""]);

        let smaller = TerminalSize { cols: 10, rows: 2 };
        view.apply(TerminalEvent::Resized {
            id: id(),
            size: smaller,
        })
        .unwrap();
        assert_eq!(texts(&mut view).len(), 2);

        assert_eq!(
            view.apply(TerminalEvent::End { id: id() }).unwrap(),
            Applied::Ended
        );
    }

    #[test]
    fn a_views_terminal_never_answers_a_program() {
        let source = Terminal::new(SIZE).unwrap();
        let mut view = View::new(1, config()).unwrap();
        let (event, _) = attached(1, &source);
        view.apply(event).unwrap();

        // Device attributes, cursor position and a color: the runtime answers these, so a view must not.
        view.apply(output("\x1b[c\x1b[6n\x1b]10;?\x07")).unwrap();
        let terminal = view.terminal.as_mut().unwrap();
        assert!(terminal.take_replies().is_empty());
    }

    #[test]
    fn a_selection_ends_when_output_changes_the_text_it_covered() {
        let source = Terminal::new(SIZE).unwrap();
        let mut view = View::new(1, config()).unwrap();
        let (event, _) = attached(1, &source);
        view.apply(event).unwrap();
        view.apply(output("hello world\r\n")).unwrap();
        view.select(CellPoint { x: 0, y: 0 }, CellPoint { x: 4, y: 0 })
            .unwrap();
        assert_eq!(view.selection().as_deref(), Some("hello"));

        // Output that only scrolls the selected line up keeps it.
        view.apply(output("one\r\ntwo\r\nthree\r\n")).unwrap();
        assert_eq!(view.selection().as_deref(), Some("hello"));

        // Once the line is pruned from the scrollback (1,000 lines here) the selection would point at other text, so it ends.
        let flood = "filler line\r\n".repeat(5_000);
        view.apply(output(&flood)).unwrap();
        assert_eq!(view.selection(), None);
        let terminal = view.terminal.as_ref().unwrap();
        assert_eq!(terminal.selection_text().unwrap(), None);

        // Clear drops the rows above the cursor, and a selection on them with it.
        view.apply(output("kept\r\nprompt")).unwrap();
        view.select(CellPoint { x: 0, y: 2 }, CellPoint { x: 3, y: 2 })
            .unwrap();
        assert_eq!(view.selection().as_deref(), Some("kept"));
        view.apply(TerminalEvent::Cleared { id: id() }).unwrap();
        assert_eq!(view.selection(), None);
    }

    #[test]
    fn typing_brings_a_scrolled_back_view_to_the_newest_output() {
        let source = Terminal::new(SIZE).unwrap();
        let mut view = View::new(1, config()).unwrap();
        let (event, _) = attached(1, &source);
        view.apply(event).unwrap();
        let lines: String = (0..50).map(|line| format!("line {line}\r\n")).collect();
        view.apply(output(&lines)).unwrap();
        view.scroll(Scroll::Lines(-20)).unwrap();
        assert_eq!(texts(&mut view)[0], "line 27");

        let mut keys = KeyEncoder::new().unwrap();
        let press = KeyPress {
            key: taco_ghostty::input::Key::Char('a'),
            mods: taco_ghostty::input::Mods {
                ctrl: true,
                ..Default::default()
            },
            text: "a",
            repeat: false,
        };
        let sent = view.key(&mut keys, &press).unwrap();
        assert_eq!(sent, b"\x01");
        assert_eq!(texts(&mut view)[0], "line 47");
    }
}
