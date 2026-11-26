//! One terminal's view: a silent copy of the runtime's terminal, fed in the order the terminal channel sends.

use taco_ghostty::render::{Dirty, Frame, RenderState, Row};
use taco_ghostty::{Error, Step, Terminal};
use taco_protocol::{CellSize, TerminalColors, TerminalEvent, TerminalSize};

/// What a view needs to start: what every view's terminal is set to.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewConfig {
    pub scrollback: u32,
    pub colors: TerminalColors,
}

/// What an event did to the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// Not for this view: before its `Attached`, or for an attachment it no longer holds.
    Ignored,
    Changed,
    Ended,
}

/// A view that has asked to attach and follows the runtime once `Attached` comes back with its number.
pub struct View {
    attachment: u64,
    /// None until `Attached` arrives.
    terminal: Option<Terminal>,
    history_pending: bool,
    size: Option<TerminalSize>,
    render: RenderState,
    /// The viewport as last read, one row per line, refreshed a dirty row at a time.
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
            size: None,
            render,
            rows: Vec::new(),
            config,
        })
    }

    pub fn apply(
        &mut self,
        event: TerminalEvent,
        cell: Option<CellSize>,
    ) -> Result<Applied, Error> {
        if let TerminalEvent::Attached {
            attachment,
            size,
            screen,
            ..
        } = &event
        {
            if *attachment != self.attachment {
                return Ok(Applied::Ignored);
            }
            let mut terminal = Terminal::restore(screen)?;
            let scrollback = self.config.scrollback as usize;
            terminal.set_scrollback(scrollback)?;
            terminal.set_colors(&self.config.colors)?;
            self.terminal = Some(terminal);
            self.size = Some(*size);
            self.history_pending = true;
            self.rows.clear();
            return Ok(Applied::Changed);
        }
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(Applied::Ignored);
        };

        match event {
            TerminalEvent::Attached { .. } => Ok(Applied::Ignored),
            TerminalEvent::History {
                attachment,
                part,
                last,
                ..
            } => {
                if attachment != self.attachment {
                    return Ok(Applied::Ignored);
                }
                terminal.push_history(&part, last);
                Ok(Applied::Changed)
            }
            TerminalEvent::Output { data, .. } => {
                terminal.write(&data);
                Ok(Applied::Changed)
            }
            TerminalEvent::Resized { size, .. } => {
                let mut pixels = CellSize {
                    width_px: 0,
                    height_px: 0,
                };
                if let Some(known) = cell {
                    pixels = known;
                }
                terminal.resize(size, pixels.width_px, pixels.height_px)?;
                self.size = Some(size);
                Ok(Applied::Changed)
            }
            TerminalEvent::Cleared { .. } => {
                terminal.clear()?;
                Ok(Applied::Changed)
            }
            TerminalEvent::End { .. } => Ok(Applied::Ended),
        }
    }

    /// Applies at most one page of restored history; false once there is none left.
    pub fn step_history(&mut self) -> Result<bool, Error> {
        let Some(terminal) = self.terminal.as_mut() else {
            return Ok(false);
        };
        if !self.history_pending {
            return Ok(false);
        }
        let step = terminal.step_history()?;
        let pending = step != Step::Finished;
        self.history_pending = pending;
        Ok(pending)
    }

    /// The viewport's rows and the frame they came from, refreshed where they changed. None before `Attached`.
    pub fn read(&mut self) -> Result<Option<(Frame<'_>, &[Row])>, Error> {
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
        Ok(Some((frame, &self.rows)))
    }

    #[cfg(test)]
    fn terminal_mut(&mut self) -> &mut Terminal {
        self.terminal.as_mut().expect("attached")
    }
}

#[cfg(test)]
impl View {
    fn size(&self) -> Option<TerminalSize> {
        self.size
    }

    fn has_history_to_apply(&self) -> bool {
        self.history_pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_ghostty::Snapshot;
    use taco_protocol::{Rgb, TerminalId};

    const SIZE: TerminalSize = TerminalSize { cols: 20, rows: 4 };

    fn config() -> ViewConfig {
        let grey = Rgb {
            r: 0x80,
            g: 0x80,
            b: 0x80,
        };
        ViewConfig {
            scrollback: 1_000,
            colors: TerminalColors {
                foreground: grey,
                background: grey,
                cursor: grey,
                cursor_text: grey,
                selection: grey,
                ansi: [grey; 16],
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

    fn texts(view: &mut View) -> Vec<String> {
        let (frame, rows) = view.read().unwrap().unwrap();
        let texts = rows.iter().map(|row| row.text.clone()).collect();
        frame.clean().unwrap();
        texts
    }

    #[test]
    fn nothing_reaches_a_view_before_its_attached_or_for_another_attachment() {
        let mut source = Terminal::new(SIZE).unwrap();
        source.write(b"from the snapshot");
        let mut view = View::new(2, config()).unwrap();

        assert_eq!(
            view.apply(output("too early"), None).unwrap(),
            Applied::Ignored
        );
        // The answer to an earlier attach, still on its way when this view was made.
        let (stale, _) = attached(1, &source);
        assert_eq!(view.apply(stale, None).unwrap(), Applied::Ignored);
        assert!(view.read().unwrap().is_none());

        let (own, history) = attached(2, &source);
        assert_eq!(view.apply(own, None).unwrap(), Applied::Changed);
        for (index, part) in history.iter().enumerate() {
            let last = index + 1 == history.len();
            let stale_part = TerminalEvent::History {
                id: id(),
                attachment: 1,
                part: part.clone(),
                last,
            };
            assert_eq!(view.apply(stale_part, None).unwrap(), Applied::Ignored);
        }
        assert_eq!(view.apply(output(" ok"), None).unwrap(), Applied::Changed);
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
        let (event, history) = attached(1, &source);
        view.apply(event, None).unwrap();
        let last_index = history.len() - 1;
        for (index, part) in history.into_iter().enumerate() {
            let history_part = TerminalEvent::History {
                id: id(),
                attachment: 1,
                part,
                last: index == last_index,
            };
            view.apply(history_part, None).unwrap();
        }

        let mut steps = 0;
        while view.step_history().unwrap() {
            steps += 1;
            view.apply(output("live\r\n"), None).unwrap();
        }
        assert!(steps > 1, "a long history should take more than one step");
        assert!(!view.has_history_to_apply());
        let screen = texts(&mut view);
        assert_eq!(screen[2], "live");
    }

    #[test]
    fn resize_and_clear_apply_where_they_come_in_the_output() {
        let source = Terminal::new(SIZE).unwrap();
        let mut view = View::new(1, config()).unwrap();
        let (event, _) = attached(1, &source);
        view.apply(event, None).unwrap();

        view.apply(output("one\r\ntwo\r\nprompt $"), None).unwrap();
        let cleared = TerminalEvent::Cleared { id: id() };
        view.apply(cleared, None).unwrap();
        assert_eq!(texts(&mut view), vec!["prompt $", "", "", ""]);

        let smaller = TerminalSize { cols: 10, rows: 2 };
        let resized = TerminalEvent::Resized {
            id: id(),
            size: smaller,
        };
        view.apply(resized, None).unwrap();
        assert_eq!(view.size(), Some(smaller));
        assert_eq!(texts(&mut view).len(), 2);

        let end = TerminalEvent::End { id: id() };
        assert_eq!(view.apply(end, None).unwrap(), Applied::Ended);
    }

    #[test]
    fn a_views_terminal_never_answers_a_program() {
        let source = Terminal::new(SIZE).unwrap();
        let mut view = View::new(1, config()).unwrap();
        let (event, _) = attached(1, &source);
        view.apply(event, None).unwrap();

        // Device attributes, cursor position and a color: the runtime answers these, so a view must not.
        view.apply(output("\x1b[c\x1b[6n\x1b]10;?\x07"), None)
            .unwrap();
        assert!(view.terminal_mut().take_replies().is_empty());
    }
}
