//! The views kept and the attaches they ask for, with no window: what `Terminals` drives, and what the tests check.

use std::collections::{HashMap, HashSet};

use taco_ghostty::input::Scroll;
use taco_protocol::{CellSize, TerminalEvent, TerminalId, TerminalRequest, TerminalSize};

use crate::shown::{KEPT_VIEWS, after_showing, viewed};
use crate::view::{Applied, View, ViewConfig};

/// The size a newly attached view asks for, once the middle has been laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub size: TerminalSize,
    pub cell: CellSize,
}

/// The views kept and their attaches. An attach waits in the runtime as a whole snapshot until its `Attached` is read,
/// so at most one is outstanding per terminal and `KEPT_VIEWS` in all.
pub struct Views {
    views: HashMap<TerminalId, View>,
    open: HashSet<TerminalId>,
    /// Terminals whose shell exited while they are still in the arrangement: the runtime holds nothing more for them.
    ended: HashSet<TerminalId>,
    /// Terminals whose view failed since the arrangement last changed, and which were attached again once.
    retried: HashSet<TerminalId>,
    /// Terminals whose view failed twice since the arrangement last changed; they get none until it changes.
    given_up: HashSet<TerminalId>,
    /// Terminals with an attach sent whose `Attached` has not been read yet, view or no view.
    awaiting: HashSet<TerminalId>,
    recency: Vec<TerminalId>,
    on_screen: Option<TerminalId>,
    /// Counts every attach sent, so each has a number of its own.
    attachments: u64,
    /// The size and cell last asked of the runtime for each terminal, so the same are not asked twice.
    asked: HashMap<TerminalId, Fit>,
    fit: Option<Fit>,
    /// Used before the middle has been laid out.
    cell: CellSize,
    config: ViewConfig,
    /// The focus last reported to a program, so each change is reported once and a terminal switched away from hears it lost focus.
    reported_focus: Option<(TerminalId, bool)>,
    outbox: Vec<TerminalRequest>,
}

impl Views {
    pub fn new(config: ViewConfig, cell: CellSize) -> Views {
        Views {
            views: HashMap::new(),
            open: HashSet::new(),
            ended: HashSet::new(),
            retried: HashSet::new(),
            given_up: HashSet::new(),
            awaiting: HashSet::new(),
            recency: Vec::new(),
            on_screen: None,
            attachments: 0,
            asked: HashMap::new(),
            fit: None,
            cell,
            config,
            reported_focus: None,
            outbox: Vec::new(),
        }
    }

    pub fn config(&self) -> &ViewConfig {
        &self.config
    }

    /// The requests to send, in order.
    pub fn take_requests(&mut self) -> Vec<TerminalRequest> {
        std::mem::take(&mut self.outbox)
    }

    pub fn on_screen(&self) -> Option<&TerminalId> {
        self.on_screen.as_ref()
    }

    pub fn on_screen_view(&mut self) -> Option<&mut View> {
        let id = self.on_screen.as_ref()?;
        self.views.get_mut(id)
    }

    pub fn view(&mut self, id: &TerminalId) -> Option<&mut View> {
        self.views.get_mut(id)
    }

    pub fn show(&mut self, open: &[TerminalId], on_screen: Option<&TerminalId>) {
        self.open = open.iter().cloned().collect();
        self.recency = after_showing(&self.recency, on_screen, &self.open);
        let wanted = viewed(&self.recency);

        // Views are let go before new ones are made, so no more than `KEPT_VIEWS` are ever held.
        let held: Vec<TerminalId> = self.views.keys().cloned().collect();
        for id in held {
            if !wanted.contains(&id) {
                self.let_go(&id);
            }
        }
        let open_now = &self.open;
        self.ended.retain(|id| open_now.contains(id));
        self.asked.retain(|id, _| open_now.contains(id));
        self.awaiting.retain(|id| open_now.contains(id));
        self.retried.clear();
        self.given_up.clear();
        self.on_screen = on_screen.cloned();
        self.fill();
    }

    /// The middle's size: what new views ask for, and a resize for the terminal on screen when it changed.
    pub fn fit(&mut self, fit: Fit) {
        self.fit = Some(fit);
        self.cell = fit.cell;
        let Some(id) = self.on_screen.clone() else {
            return;
        };
        let already_asked = self.asked.get(&id) == Some(&fit);
        if already_asked || !self.views.contains_key(&id) {
            return;
        }
        self.asked.insert(id.clone(), fit);
        self.outbox.push(TerminalRequest::Resize {
            id,
            size: fit.size,
            cell: fit.cell,
        });
    }

    /// True when the window should draw again: the terminal on screen changed, or a view ended or started over.
    pub fn receive(&mut self, event: TerminalEvent) -> bool {
        let id = event_terminal(&event).clone();
        // One attach per terminal is ever outstanding: an `Attached` answers it, and after `End` none will come.
        let settles_attach = matches!(
            event,
            TerminalEvent::Attached { .. } | TerminalEvent::End { .. }
        );
        if settles_attach {
            self.awaiting.remove(&id);
        }
        let redraw = self.apply_to_view(id, event);
        if settles_attach {
            self.fill();
        }
        redraw
    }

    fn apply_to_view(&mut self, id: TerminalId, event: TerminalEvent) -> bool {
        let on_screen = self.on_screen.as_ref() == Some(&id);
        let Some(view) = self.views.get_mut(&id) else {
            return false;
        };
        let applied = view.apply(event);
        match applied {
            Ok(Applied::Ignored) => false,
            Ok(Applied::Changed) => on_screen,
            Ok(Applied::Ended) => {
                self.views.remove(&id);
                self.ended.insert(id);
                true
            }
            Err(_) => {
                self.start_over(id);
                true
            }
        }
    }

    /// Applies a page of each view's restored history; true while some is left.
    pub fn step_history(&mut self) -> bool {
        let mut left = false;
        let mut broken = Vec::new();
        for (id, view) in self.views.iter_mut() {
            match view.step_history() {
                Ok(true) => left = true,
                Ok(false) => {}
                Err(_) => broken.push(id.clone()),
            }
        }
        for id in broken {
            self.start_over(id);
        }
        left
    }

    /// New settings or theme colors for every view, and for the views made from now on.
    pub fn configure(&mut self, config: ViewConfig) {
        self.config = config.clone();
        let mut broken = Vec::new();
        for (id, view) in self.views.iter_mut() {
            if view.configure(config.clone()).is_err() {
                broken.push(id.clone());
            }
        }
        for id in broken {
            self.start_over(id);
        }
    }

    /// Tells programs that asked (mode 1004) when the terminal on screen gains or loses focus, and the one switched away from that it lost it.
    pub fn report_focus(&mut self, focused: bool) {
        let Some(id) = self.on_screen.clone() else {
            return;
        };
        let current = (id.clone(), focused);
        if self.reported_focus.as_ref() == Some(&current) {
            return;
        }
        let attached = self.views.get(&id).is_some_and(|view| view.is_attached());
        if !attached {
            return;
        }
        if let Some((previous, true)) = self.reported_focus.clone()
            && previous != id
        {
            self.send_focus(&previous, false);
        }
        self.send_focus(&id, focused);
        self.reported_focus = Some(current);
    }

    fn send_focus(&mut self, id: &TerminalId, focused: bool) {
        let Some(view) = self.views.get(id) else {
            return;
        };
        if let Ok(Some(report)) = view.focus_report(focused) {
            self.write(id.clone(), report);
        }
    }

    /// Sends `data` as typed into the terminal, which brings its view back to the newest output.
    pub fn type_into(&mut self, id: TerminalId, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }
        if let Some(view) = self.views.get_mut(&id) {
            let _ = view.scroll(Scroll::Bottom);
        }
        self.write(id, data);
    }

    pub fn write(&mut self, id: TerminalId, data: Vec<u8>) {
        if data.is_empty() {
            return;
        }
        self.outbox.push(TerminalRequest::Write { id, data });
    }

    pub fn clear(&mut self, id: TerminalId) {
        self.outbox.push(TerminalRequest::Clear { id });
    }

    /// A view that could not apply what it was sent starts over from a fresh snapshot, once per change to the arrangement.
    pub fn start_over(&mut self, id: TerminalId) {
        self.let_go(&id);
        let first_failure = self.retried.insert(id.clone());
        if !first_failure {
            self.given_up.insert(id);
        }
        self.fill();
    }

    /// Attaches every terminal that should have a view and can, within the bound.
    fn fill(&mut self) {
        let wanted = viewed(&self.recency);
        for id in wanted {
            let has_view = self.views.contains_key(&id);
            let held_back = self.ended.contains(&id) || self.given_up.contains(&id);
            if !has_view && !held_back && self.can_attach(&id) {
                self.attach(id);
            }
        }
    }

    fn can_attach(&self, id: &TerminalId) -> bool {
        !self.awaiting.contains(id) && self.awaiting.len() < KEPT_VIEWS
    }

    fn attach(&mut self, id: TerminalId) {
        self.attachments += 1;
        let Ok(view) = View::new(self.attachments, self.config.clone()) else {
            self.given_up.insert(id);
            return;
        };
        let mut size = None;
        let mut cell = self.cell;
        if let Some(fit) = self.fit {
            self.asked.insert(id.clone(), fit);
            size = Some(fit.size);
            cell = fit.cell;
        }
        self.outbox.push(TerminalRequest::Attach {
            id: id.clone(),
            attachment: self.attachments,
            size,
            cell,
        });
        self.awaiting.insert(id.clone());
        self.views.insert(id, view);
    }

    fn let_go(&mut self, id: &TerminalId) {
        let had_view = self.views.remove(id).is_some();
        if had_view && self.open.contains(id) {
            self.outbox.push(TerminalRequest::Detach { id: id.clone() });
        }
    }
}

fn event_terminal(event: &TerminalEvent) -> &TerminalId {
    match event {
        TerminalEvent::Attached { id, .. } => id,
        TerminalEvent::History { id, .. } => id,
        TerminalEvent::Output { id, .. } => id,
        TerminalEvent::Resized { id, .. } => id,
        TerminalEvent::Cleared { id } => id,
        TerminalEvent::CursorDefaults { id, .. } => id,
        TerminalEvent::End { id } => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_ghostty::{Snapshot, Terminal};
    use taco_protocol::{Rgb, TerminalColors};

    const SIZE: TerminalSize = TerminalSize { cols: 20, rows: 4 };
    const CELL: CellSize = CellSize {
        width_px: 7,
        height_px: 14,
    };

    fn views() -> Views {
        let grey = Rgb {
            r: 0x80,
            g: 0x80,
            b: 0x80,
        };
        let config = ViewConfig {
            scrollback: 100,
            colors: TerminalColors {
                foreground: grey,
                background: grey,
                cursor: grey,
                cursor_text: grey,
                selection: grey,
                ansi: [grey; 16],
            },
        };
        Views::new(config, CELL)
    }

    fn id(name: &str) -> TerminalId {
        TerminalId(name.to_string())
    }

    /// The terminals attached by `requests`, with each attach's number.
    fn attaches(requests: Vec<TerminalRequest>) -> Vec<(TerminalId, u64)> {
        let mut found = Vec::new();
        for request in requests {
            if let TerminalRequest::Attach { id, attachment, .. } = request {
                found.push((id, attachment));
            }
        }
        found
    }

    fn attached(id: &TerminalId, attachment: u64, screen: Vec<u8>) -> TerminalEvent {
        TerminalEvent::Attached {
            id: id.clone(),
            attachment,
            size: SIZE,
            screen,
        }
    }

    fn real_screen() -> Vec<u8> {
        let terminal = Terminal::new(SIZE).unwrap();
        let snapshot = Snapshot::take(&terminal).unwrap();
        snapshot.screen().to_vec()
    }

    #[test]
    fn a_failing_view_is_attached_again_once_per_change_to_the_arrangement() {
        let mut views = views();
        let a = id("a");
        let open = [a.clone()];
        views.show(&open, Some(&a));
        assert_eq!(attaches(views.take_requests()), vec![(a.clone(), 1)]);

        // A snapshot the view cannot restore: it starts over once.
        let broken = vec![0xde, 0xad];
        views.receive(attached(&a, 1, broken.clone()));
        assert_eq!(attaches(views.take_requests()), vec![(a.clone(), 2)]);

        // Failing again, it waits for the arrangement to change.
        views.receive(attached(&a, 2, broken));
        assert_eq!(attaches(views.take_requests()), vec![]);
        views.show(&open, Some(&a));
        assert_eq!(attaches(views.take_requests()), vec![(a, 3)]);
    }

    #[test]
    fn attaches_wait_for_their_snapshots_one_per_terminal_and_eight_in_all() {
        let mut views = views();
        let all: Vec<TerminalId> = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
            .iter()
            .map(|name| id(name))
            .collect();

        // Ten terminals shown in turn while no snapshot is read: only eight attaches go out.
        let mut sent = Vec::new();
        for shown in &all {
            views.show(&all, Some(shown));
            sent.extend(attaches(views.take_requests()));
        }
        assert_eq!(sent.len(), KEPT_VIEWS);
        let a = id("a");
        assert!(sent.contains(&(a.clone(), 1)));

        // Snapshots read for b to h free their places, and i and j, held back, are attached.
        let mut freed = Vec::new();
        for (index, name) in ["b", "c", "d", "e", "f", "g", "h"].iter().enumerate() {
            let attachment = index as u64 + 2;
            views.receive(attached(&id(name), attachment, real_screen()));
            freed.extend(attaches(views.take_requests()));
        }
        assert_eq!(freed, vec![(id("j"), 9), (id("i"), 10)]);

        // `a` lost its view while its snapshot was still coming. Places are free, but it is not attached again yet.
        views.show(&all, Some(&a));
        assert_eq!(attaches(views.take_requests()), vec![]);

        // Once that snapshot is read, `a` is attached afresh.
        views.receive(attached(&a, 1, real_screen()));
        assert_eq!(attaches(views.take_requests()), vec![(a, 11)]);
    }

    #[test]
    fn focus_is_reported_once_per_change_and_a_terminal_switched_away_from_hears_it_lost_it() {
        let mut views = views();
        let a = id("a");
        let b = id("b");
        let c = id("c");
        let open = [a.clone(), b.clone(), c.clone()];
        let writes = |views: &mut Views| -> Vec<(TerminalId, Vec<u8>)> {
            let mut found = Vec::new();
            for request in views.take_requests() {
                if let TerminalRequest::Write { id, data } = request {
                    found.push((id, data));
                }
            }
            found
        };

        views.show(&open, Some(&a));
        views.show(&open, Some(&b));
        for (terminal, attachment) in [(&a, 1), (&b, 2)] {
            views.receive(attached(terminal, attachment, real_screen()));
            let asks_for_focus_reports = TerminalEvent::Output {
                id: terminal.clone(),
                data: b"\x1b[?1004h".to_vec(),
            };
            views.receive(asks_for_focus_reports);
        }
        views.show(&open, Some(&a));
        views.take_requests();

        views.report_focus(true);
        assert_eq!(writes(&mut views), vec![(a.clone(), b"\x1b[I".to_vec())]);
        views.report_focus(true);
        assert_eq!(writes(&mut views), vec![]);

        // Switching tells the terminal left that it lost focus, and the new one that it has it.
        views.show(&open, Some(&b));
        views.report_focus(true);
        assert_eq!(
            writes(&mut views),
            vec![(a, b"\x1b[O".to_vec()), (b.clone(), b"\x1b[I".to_vec())]
        );

        // A terminal whose view has not attached yet is told nothing; back on `b`, losing focus is reported.
        views.show(&open, Some(&c));
        views.report_focus(true);
        assert_eq!(writes(&mut views), vec![]);
        views.show(&open, Some(&b));
        views.report_focus(false);
        assert_eq!(writes(&mut views), vec![(b, b"\x1b[O".to_vec())]);
    }

    #[test]
    fn typing_brings_a_scrolled_back_view_to_the_newest_output() {
        let mut views = views();
        let a = id("a");
        views.show(std::slice::from_ref(&a), Some(&a));
        views.receive(attached(&a, 1, real_screen()));
        let lines: String = (0..50).map(|line| format!("line {line}\r\n")).collect();
        views.receive(TerminalEvent::Output {
            id: a.clone(),
            data: lines.into_bytes(),
        });
        views.take_requests();
        let top_line = |views: &mut Views| {
            let view = views.view(&id("a")).unwrap();
            let lines = view.visible_lines().unwrap().unwrap();
            lines[0].clone()
        };
        views.view(&a).unwrap().scroll(Scroll::Lines(-20)).unwrap();
        assert_eq!(top_line(&mut views), "line 27");

        views.type_into(a.clone(), b"x".to_vec());
        assert_eq!(top_line(&mut views), "line 47");
        let sent = vec![TerminalRequest::Write {
            id: a,
            data: b"x".to_vec(),
        }];
        assert_eq!(views.take_requests(), sent);
    }

    #[test]
    fn the_middle_size_is_asked_once_and_given_to_new_views() {
        let mut views = views();
        let a = id("a");
        let b = id("b");
        let open = [a.clone(), b.clone()];
        let fit = Fit {
            size: TerminalSize {
                cols: 100,
                rows: 30,
            },
            cell: CELL,
        };
        views.fit(fit);
        views.show(&open, Some(&a));
        let first = views.take_requests();
        let asked_for = matches!(
            first.as_slice(),
            [TerminalRequest::Attach { size: Some(size), .. }] if *size == fit.size
        );
        assert!(asked_for, "{first:?}");

        // The same size again asks nothing; a new one is asked once.
        views.fit(fit);
        assert_eq!(views.take_requests(), vec![]);
        let wider = Fit {
            size: TerminalSize {
                cols: 120,
                rows: 30,
            },
            cell: CELL,
        };
        views.fit(wider);
        views.fit(wider);
        let resizes = views.take_requests();
        assert_eq!(
            resizes,
            vec![TerminalRequest::Resize {
                id: a,
                size: wider.size,
                cell: CELL
            }]
        );
    }
}
