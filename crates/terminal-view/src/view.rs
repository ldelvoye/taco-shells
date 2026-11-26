//! One terminal's view: a silent copy of the runtime's terminal, fed in the order the terminal channel sends.

use taco_ghostty::render::{Dirty, RenderState, Row};
use taco_ghostty::{Error, Step, Terminal};
use taco_protocol::{Rgb, TerminalColors, TerminalEvent};

use crate::draw::{Painting, lay_out};

/// What every view's terminal is set to.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewConfig {
    pub scrollback: u32,
    pub colors: TerminalColors,
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
    config: ViewConfig,
}

impl View {
    pub fn new(attachment: u64, config: ViewConfig) -> Result<View, Error> {
        let render = RenderState::new()?;
        Ok(View {
            attachment,
            terminal: None,
            history_pending: false,
            render,
            rows: Vec::new(),
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
                let scrollback = self.config.scrollback as usize;
                terminal.set_scrollback(scrollback)?;
                terminal.set_colors(&self.config.colors)?;
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
                Ok(Applied::Changed)
            }
            TerminalEvent::Resized { size, .. } => {
                let Some(terminal) = self.terminal.as_mut() else {
                    return Ok(Applied::Ignored);
                };
                // Pixel sizes matter only for answering size reports, which the runtime does.
                terminal.resize(size, 0, 0)?;
                Ok(Applied::Changed)
            }
            TerminalEvent::Cleared { .. } => {
                let Some(terminal) = self.terminal.as_mut() else {
                    return Ok(Applied::Ignored);
                };
                terminal.clear()?;
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

    fn terminal_for(&mut self, attachment: u64) -> Option<&mut Terminal> {
        if attachment != self.attachment {
            return None;
        }
        self.terminal.as_mut()
    }
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
}
