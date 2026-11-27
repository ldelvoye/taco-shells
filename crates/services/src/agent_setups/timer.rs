use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Instant;

use taco_protocol::TerminalId;

pub(super) enum TimerMessage {
    Schedule(TerminalId, Instant),
    Cancel(TerminalId),
    Refresh,
    // Sent at shutdown, since a PluginHeard handle keeps a sender alive and the channel never closes.
    Stop,
}

enum Next {
    Message(TimerMessage),
    Due,
    Closed,
}

pub(super) struct TimerWork {
    pub(super) stopping: Box<dyn Fn() -> bool + Send>,
    pub(super) check: Box<dyn Fn(&TerminalId) + Send>,
    pub(super) refresh: Box<dyn Fn() + Send>,
}

fn next_message(messages: &Receiver<TimerMessage>, earliest: Option<Instant>) -> Next {
    let Some(due) = earliest else {
        return match messages.recv() {
            Ok(message) => Next::Message(message),
            Err(_) => Next::Closed,
        };
    };
    let now = Instant::now();
    let wait = due.saturating_duration_since(now);
    match messages.recv_timeout(wait) {
        Ok(message) => Next::Message(message),
        Err(RecvTimeoutError::Timeout) => Next::Due,
        Err(RecvTimeoutError::Disconnected) => Next::Closed,
    }
}

fn run_due_checks(work: &TimerWork, pending: &mut HashMap<TerminalId, Instant>) {
    if (work.stopping)() {
        return;
    }
    let now = Instant::now();
    let mut due = Vec::new();
    for (terminal, at) in pending.iter() {
        if *at <= now {
            due.push(terminal.clone());
        }
    }
    for terminal in &due {
        pending.remove(terminal);
    }
    for terminal in &due {
        if (work.stopping)() {
            return;
        }
        let check = std::panic::AssertUnwindSafe(|| (work.check)(terminal));
        let _ = std::panic::catch_unwind(check);
    }
}

fn run_refresh(work: &TimerWork) {
    if (work.stopping)() {
        return;
    }
    let refreshing = std::panic::AssertUnwindSafe(|| (work.refresh)());
    let _ = std::panic::catch_unwind(refreshing);
}

pub(super) fn timer_loop(work: TimerWork, messages: Receiver<TimerMessage>) {
    let mut pending: HashMap<TerminalId, Instant> = HashMap::new();
    loop {
        let earliest = pending.values().min().copied();
        let next = next_message(&messages, earliest);
        match next {
            Next::Message(TimerMessage::Schedule(terminal, due)) => {
                let slot = pending.entry(terminal).or_insert(due);
                if due < *slot {
                    *slot = due;
                }
            }
            Next::Message(TimerMessage::Cancel(terminal)) => {
                pending.remove(&terminal);
            }
            Next::Message(TimerMessage::Refresh) => run_refresh(&work),
            Next::Message(TimerMessage::Stop) => return,
            Next::Due => run_due_checks(&work, &mut pending),
            Next::Closed => return,
        }
    }
}
