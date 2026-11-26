//! The views kept and the attaches they ask for, with no window: what `Terminals` drives, and what the tests check.

use std::collections::{HashMap, HashSet};

use taco_protocol::{CellSize, TerminalEvent, TerminalId, TerminalRequest, TerminalSize};

use crate::shown::{KEPT_VIEWS, after_showing, viewed};
use crate::view::{Applied, View, ViewConfig};

/// The size a newly attached view asks for, once the middle has been laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub size: TerminalSize,
    pub cell: CellSize,
}

/// Every attach waits in the runtime as a whole snapshot until its `Attached` is read, so the attaches outstanding are bounded:
/// one per terminal, and no more in all than the views kept.
pub struct Views {
    views: HashMap<TerminalId, View>,
    open: HashSet<TerminalId>,
    /// Their shell exited while they are still in the arrangement: the runtime holds nothing more for them.
    ended: HashSet<TerminalId>,
    /// Their view failed since the arrangement last changed, and was attached again once.
    failed: HashSet<TerminalId>,
    /// Their view failed twice since the arrangement last changed; they get none until it changes.
    given_up: HashSet<TerminalId>,
    /// An attach was sent and its `Attached` has not been read yet, view or no view.
    awaiting: HashSet<TerminalId>,
    recency: Vec<TerminalId>,
    on_screen: Option<TerminalId>,
    /// Counts every attach sent, so each has a number of its own.
    attachments: u64,
    /// The size last asked of the runtime for each terminal, so the same one is not asked twice.
    asked: HashMap<TerminalId, TerminalSize>,
    fit: Option<Fit>,
    /// Used before the middle has been laid out.
    cell: CellSize,
    config: ViewConfig,
    outbox: Vec<TerminalRequest>,
}

impl Views {
    pub fn new(config: ViewConfig, cell: CellSize) -> Views {
        Views {
            views: HashMap::new(),
            open: HashSet::new(),
            ended: HashSet::new(),
            failed: HashSet::new(),
            given_up: HashSet::new(),
            awaiting: HashSet::new(),
            recency: Vec::new(),
            on_screen: None,
            attachments: 0,
            asked: HashMap::new(),
            fit: None,
            cell,
            config,
            outbox: Vec::new(),
        }
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

        // Views are let go before new ones are made.
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
        self.failed.clear();
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
        let already_asked = self.asked.get(&id) == Some(&fit.size);
        if already_asked || !self.views.contains_key(&id) {
            return;
        }
        self.asked.insert(id.clone(), fit.size);
        self.outbox.push(TerminalRequest::Resize {
            id,
            size: fit.size,
            cell: fit.cell,
        });
    }

    /// True when the terminal on screen changed or the views kept did.
    pub fn receive(&mut self, event: TerminalEvent) -> bool {
        let id = event_terminal(&event).clone();
        let is_attached = matches!(event, TerminalEvent::Attached { .. });
        if is_attached {
            // One attach per terminal is ever outstanding, so this answers it.
            self.awaiting.remove(&id);
        }
        let on_screen = self.on_screen.as_ref() == Some(&id);
        let Some(view) = self.views.get_mut(&id) else {
            if is_attached {
                self.fill();
            }
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

    /// Sends what a view of the terminal on screen wants written, as typed.
    pub fn write_on_screen(&mut self, data: Vec<u8>) {
        let Some(id) = self.on_screen.clone() else {
            return;
        };
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
        let first_failure = self.failed.insert(id.clone());
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
            self.failed.insert(id);
            return;
        };
        let mut size = None;
        let mut cell = self.cell;
        if let Some(fit) = self.fit {
            self.asked.insert(id.clone(), fit.size);
            size = Some(fit.size);
            cell = fit.cell;
        }
        self.outbox.push(TerminalRequest::Attach {
            id: id.clone(),
            attachment: self.attachments,
            size,
            cell: Some(cell),
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

        // a lost its view while its snapshot was still coming; showing it again sends nothing yet.
        views.show(&all, Some(&a));
        assert_eq!(attaches(views.take_requests()), vec![]);

        // Once that snapshot is read, a is attached afresh.
        views.receive(attached(&a, 1, real_screen()));
        let after = attaches(views.take_requests());
        assert_eq!(after, vec![(a, 9)]);
    }
}
