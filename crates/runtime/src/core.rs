use std::collections::HashMap;
use std::time::{Duration, Instant};

use taco_ghostty::{Snapshot, Terminal};
use taco_protocol::{
    RuntimeAnswer, RuntimeCommand, RuntimeQuery, RuntimeReport, TerminalColors, TerminalEvent,
    TerminalId, TerminalRequest, TerminalSize, TerminalStart,
};

use crate::shells::Shells;

/// The scrollback a terminal has until the app pushes the setting.
const DEFAULT_SCROLLBACK: usize = 1000;
/// The longest title reported, since every row's title is published with the workspace's state.
const TITLE_CHARS: usize = 1024;
/// The shortest time between two reads of a terminal's directory, however much its shell prints.
const DIRECTORY_FLOOR: Duration = Duration::from_millis(200);
/// A write holding this byte submits a line, typed or pasted.
const CARRIAGE_RETURN: u8 = b'\r';
/// What a shell that could not start reports, like an exit.
const FAILED_START_CODE: i32 = 1;

/// One thing that happened, handed to the core in the order it happened.
#[derive(Debug)]
pub enum Input {
    Command(RuntimeCommand),
    Request(TerminalRequest),
    /// Bytes a shell printed.
    Output {
        id: TerminalId,
        data: Vec<u8>,
    },
    /// A shell has exited. Its reader sends this after its last output.
    Exited {
        id: TerminalId,
        code: i32,
    },
}

/// What has to go out because of an input.
#[derive(Debug, PartialEq)]
pub enum Effect {
    Report(RuntimeReport),
    Event(TerminalEvent),
    /// Output no view is watching has been taken in, so its bytes go back to the shell's budget now.
    /// Output a view is watching goes back once it has been written to the window.
    Parsed {
        id: TerminalId,
        bytes: usize,
    },
    Stop,
}

enum View {
    None,
    Attached,
    /// The snapshot could not be taken (an unfinished sequence over the library's limit); it is tried again after the next output.
    Waiting(u64),
}

struct Screen {
    terminal: Terminal,
    size: TerminalSize,
    view: View,
    reported_title: Option<String>,
    reported_directory: String,
    directory_checked: Option<Instant>,
    directory_due: bool,
}

/// Every terminal's screen, and what each input does to them. No threads, no clock of its own: the driver passes the time in.
pub struct Core<S: Shells> {
    shells: S,
    screens: HashMap<TerminalId, Screen>,
    scrollback: usize,
    colors: Option<TerminalColors>,
    effects: Vec<Effect>,
}

impl<S: Shells> Core<S> {
    pub fn new(shells: S) -> Core<S> {
        Core {
            shells,
            screens: HashMap::new(),
            scrollback: DEFAULT_SCROLLBACK,
            colors: None,
            effects: Vec::new(),
        }
    }

    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        match input {
            Input::Command(command) => self.command(command),
            Input::Request(request) => self.request(request),
            Input::Output { id, data } => self.output(id, data, now),
            Input::Exited { id, code } => self.exited(id, code),
        }
        std::mem::take(&mut self.effects)
    }

    /// Runs the directory reads put off by the floor. Call it at `next_deadline`.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut due = Vec::new();
        for (id, screen) in &self.screens {
            if screen.directory_due {
                due.push(id.clone());
            }
        }
        for id in due {
            self.check_directory(&id, now);
        }
        std::mem::take(&mut self.effects)
    }

    /// When `tick` next has something to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        let mut earliest: Option<Instant> = None;
        for screen in self.screens.values() {
            if !screen.directory_due {
                continue;
            }
            let Some(checked) = screen.directory_checked else {
                continue;
            };
            let deadline = checked + DIRECTORY_FLOOR;
            let sooner = match earliest {
                Some(current) => deadline < current,
                None => true,
            };
            if sooner {
                earliest = Some(deadline);
            }
        }
        earliest
    }

    pub fn shells_mut(&mut self) -> &mut S {
        &mut self.shells
    }

    fn command(&mut self, command: RuntimeCommand) {
        match command {
            RuntimeCommand::Open(start) => self.open(start),
            RuntimeCommand::Close { id } => self.shells.close(&id),
            RuntimeCommand::Scrollback { lines } => self.set_scrollback(lines as usize),
            RuntimeCommand::Colors(colors) => self.set_colors(colors),
            RuntimeCommand::Ask { ask, query } => {
                let value = self.answer(query);
                self.report(RuntimeReport::Answer { ask, value });
            }
            RuntimeCommand::Stop => self.effects.push(Effect::Stop),
        }
    }

    fn request(&mut self, request: TerminalRequest) {
        match request {
            TerminalRequest::Attach {
                id,
                attachment,
                size,
            } => self.attach(id, attachment, size),
            TerminalRequest::Detach { id } => {
                if let Some(screen) = self.screens.get_mut(&id) {
                    screen.view = View::None;
                }
            }
            TerminalRequest::Write { id, data } => self.write(id, data),
            TerminalRequest::Resize { id, size } => self.resize(&id, size),
            TerminalRequest::Clear { id } => self.clear(id),
        }
    }

    fn open(&mut self, start: TerminalStart) {
        // While its terminal lives, an id names that one terminal; a second shell under it would take over its screen.
        if self.screens.contains_key(&start.id) {
            return;
        }

        let mut requested = start.directory;
        if requested.is_none()
            && let Some(other) = &start.directory_of
        {
            requested = self.shells.directory(other);
        }

        let terminal = match self.new_terminal(start.size) {
            Ok(terminal) => terminal,
            Err(error) => {
                eprintln!("no screen for {}: {error}", start.id.0);
                self.report(RuntimeReport::Exited {
                    id: start.id,
                    code: FAILED_START_CODE,
                });
                return;
            }
        };
        let Some(directory) = self
            .shells
            .start(&start.id, requested.as_deref(), start.size)
        else {
            self.report(RuntimeReport::Exited {
                id: start.id,
                code: FAILED_START_CODE,
            });
            return;
        };

        let screen = Screen {
            terminal,
            size: start.size,
            view: View::None,
            reported_title: None,
            reported_directory: directory.clone(),
            directory_checked: None,
            directory_due: false,
        };
        self.screens.insert(start.id.clone(), screen);
        self.report(RuntimeReport::Started {
            id: start.id.clone(),
            directory,
        });
        if let Some(command) = start.command {
            self.shells.write(&start.id, command.as_bytes());
        }
    }

    fn new_terminal(&self, size: TerminalSize) -> Result<Terminal, taco_ghostty::Error> {
        let mut terminal = Terminal::new(size)?;
        terminal.set_scrollback(self.scrollback)?;
        if let Some(colors) = &self.colors {
            terminal.set_colors(colors)?;
        }
        // The runtime's screen answers a program's questions: a view is not always there to.
        terminal.answer_queries()?;
        Ok(terminal)
    }

    fn output(&mut self, id: TerminalId, data: Vec<u8>, now: Instant) {
        let Some(screen) = self.screens.get_mut(&id) else {
            return;
        };
        screen.terminal.write(&data);

        let replies = screen.terminal.take_replies();
        if !replies.is_empty() {
            self.shells.write(&id, &replies);
        }

        let title = screen.terminal.take_title();
        match title {
            Ok(Some(title)) => self.titled(&id, title),
            Ok(None) => {}
            Err(error) => eprintln!("could not read the title of {}: {error}", id.0),
        }

        let bytes = data.len();
        let Some(screen) = self.screens.get_mut(&id) else {
            return;
        };
        let waiting = match screen.view {
            View::Waiting(attachment) => Some(attachment),
            View::Attached | View::None => None,
        };
        if let View::Attached = screen.view {
            let event = TerminalEvent::Output {
                id: id.clone(),
                data,
            };
            self.effects.push(Effect::Event(event));
        } else {
            // A waiting view gets these bytes inside its snapshot, not as output.
            self.effects.push(Effect::Parsed {
                id: id.clone(),
                bytes,
            });
        }
        if let Some(attachment) = waiting {
            self.send_snapshot(&id, attachment);
        }

        self.check_directory(&id, now);
    }

    fn titled(&mut self, id: &TerminalId, title: String) {
        let kept: String = title.chars().take(TITLE_CHARS).collect();
        let Some(screen) = self.screens.get_mut(id) else {
            return;
        };
        if screen.reported_title.as_deref() == Some(kept.as_str()) {
            return;
        }
        screen.reported_title = Some(kept.clone());
        self.report(RuntimeReport::Titled {
            id: id.clone(),
            title: kept,
        });
    }

    fn check_directory(&mut self, id: &TerminalId, now: Instant) {
        let Some(screen) = self.screens.get_mut(id) else {
            return;
        };
        if let Some(checked) = screen.directory_checked
            && now < checked + DIRECTORY_FLOOR
        {
            screen.directory_due = true;
            return;
        }
        screen.directory_checked = Some(now);
        screen.directory_due = false;

        // A program running in the foreground is not the shell, so its directory is not the terminal's.
        if !self.shells.at_prompt(id) {
            return;
        }
        let Some(directory) = self.shells.directory(id) else {
            return;
        };
        let Some(screen) = self.screens.get_mut(id) else {
            return;
        };
        if directory == screen.reported_directory {
            return;
        }
        screen.reported_directory = directory.clone();
        self.report(RuntimeReport::DirectoryChanged {
            id: id.clone(),
            directory,
        });
    }

    fn exited(&mut self, id: TerminalId, code: i32) {
        let Some(screen) = self.screens.remove(&id) else {
            return;
        };
        if let View::Attached = screen.view {
            self.effects
                .push(Effect::Event(TerminalEvent::End { id: id.clone() }));
        }
        self.report(RuntimeReport::Exited { id, code });
    }

    fn attach(&mut self, id: TerminalId, attachment: u64, size: Option<TerminalSize>) {
        let Some(screen) = self.screens.get_mut(&id) else {
            return;
        };
        // With no view while it resizes, nobody is told: the snapshot that follows has the new size already.
        screen.view = View::None;
        if let Some(size) = size {
            self.resize(&id, size);
        }
        self.send_snapshot(&id, attachment);
    }

    fn send_snapshot(&mut self, id: &TerminalId, attachment: u64) {
        let Some(screen) = self.screens.get_mut(id) else {
            return;
        };
        let snapshot = match Snapshot::take(&screen.terminal) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                eprintln!("no snapshot of {} yet: {error}", id.0);
                screen.view = View::Waiting(attachment);
                return;
            }
        };
        screen.view = View::Attached;

        let screen_part = snapshot.screen().to_vec();
        let attached = TerminalEvent::Attached {
            id: id.clone(),
            attachment,
            size: screen.size,
            screen: screen_part,
        };
        self.effects.push(Effect::Event(attached));
        let parts: Vec<&[u8]> = snapshot.history().collect();
        let count = parts.len();
        for (index, part) in parts.into_iter().enumerate() {
            let last = index + 1 == count;
            let history = TerminalEvent::History {
                id: id.clone(),
                attachment,
                part: part.to_vec(),
                last,
            };
            self.effects.push(Effect::Event(history));
        }
    }

    fn write(&mut self, id: TerminalId, data: Vec<u8>) {
        if !self.screens.contains_key(&id) {
            return;
        }
        self.shells.write(&id, &data);
        if data.contains(&CARRIAGE_RETURN) {
            self.report(RuntimeReport::Submitted { id });
        }
    }

    fn resize(&mut self, id: &TerminalId, size: TerminalSize) {
        let Some(screen) = self.screens.get_mut(id) else {
            return;
        };
        if screen.size == size {
            return;
        }
        if let Err(error) = screen.terminal.resize(size, 0, 0) {
            eprintln!("could not resize {}: {error}", id.0);
            return;
        }
        screen.size = size;
        let attached = matches!(screen.view, View::Attached);
        self.shells.resize(id, size);
        if attached {
            self.effects.push(Effect::Event(TerminalEvent::Resized {
                id: id.clone(),
                size,
            }));
        }
    }

    fn clear(&mut self, id: TerminalId) {
        let Some(screen) = self.screens.get_mut(&id) else {
            return;
        };
        if let Err(error) = screen.terminal.clear() {
            eprintln!("could not clear {}: {error}", id.0);
            return;
        }
        if let View::Attached = screen.view {
            self.effects
                .push(Effect::Event(TerminalEvent::Cleared { id }));
        }
    }

    fn set_scrollback(&mut self, lines: usize) {
        self.scrollback = lines;
        for (id, screen) in &mut self.screens {
            if let Err(error) = screen.terminal.set_scrollback(lines) {
                eprintln!("could not set the scrollback of {}: {error}", id.0);
            }
        }
    }

    fn set_colors(&mut self, colors: TerminalColors) {
        for (id, screen) in &mut self.screens {
            if let Err(error) = screen.terminal.set_colors(&colors) {
                eprintln!("could not set the colors of {}: {error}", id.0);
            }
        }
        self.colors = Some(colors);
    }

    fn answer(&self, query: RuntimeQuery) -> RuntimeAnswer {
        match query {
            RuntimeQuery::DirectoryOf { id } => {
                let directory = self.shells.directory(&id);
                RuntimeAnswer::Directory(directory)
            }
            RuntimeQuery::ForegroundCommands { id } => {
                let commands = self.shells.foreground_commands(&id);
                RuntimeAnswer::Commands(commands)
            }
            RuntimeQuery::TerminalForPids { pids } => {
                let terminal = self.terminal_for_pids(&pids);
                RuntimeAnswer::Terminal(terminal)
            }
        }
    }

    /// The terminal whose shell comes first among these processes, which a hook lists from itself up to its terminal's shell.
    fn terminal_for_pids(&self, pids: &[u32]) -> Option<TerminalId> {
        for pid in pids {
            for id in self.screens.keys() {
                if self.shells.shell_pid(id) == Some(*pid) {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    fn report(&mut self, report: RuntimeReport) {
        self.effects.push(Effect::Report(report));
    }

    #[cfg(test)]
    pub(crate) fn terminal(&self, id: &TerminalId) -> Option<&Terminal> {
        self.screens.get(id).map(|screen| &screen.terminal)
    }
}
