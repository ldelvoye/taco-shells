use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

use taco_protocol::TerminalId;

/// How much of a shell's output may be read and not yet passed on before its reads stop. The shell then blocks in the kernel.
pub const BUDGET_BYTES: usize = 1_000_000;

#[derive(Default)]
struct State {
    outstanding: usize,
    /// The shell has exited: its last output is read whatever the budget says, so its exit can follow it.
    closed: bool,
}

/// One shell's output that has been read and not yet passed on.
#[derive(Default)]
pub struct Budget {
    state: Mutex<State>,
    changed: Condvar,
}

/// Every shell's budget, shared by the shells' readers, the core and the window's writer.
#[derive(Default)]
pub struct Budgets {
    budgets: Mutex<HashMap<TerminalId, Arc<Budget>>>,
}

impl Budget {
    /// Blocks until there is room to read more, or the shell has exited.
    pub fn wait_for_room(&self) {
        let mut state = self.state.lock().expect("budget lock");
        while state.outstanding >= BUDGET_BYTES && !state.closed {
            state = self.changed.wait(state).expect("budget lock");
        }
    }

    pub fn spend(&self, bytes: usize) {
        let mut state = self.state.lock().expect("budget lock");
        state.outstanding += bytes;
    }

    pub fn give_back(&self, bytes: usize) {
        let mut state = self.state.lock().expect("budget lock");
        state.outstanding = state.outstanding.saturating_sub(bytes);
        self.changed.notify_all();
    }

    pub fn close(&self) {
        let mut state = self.state.lock().expect("budget lock");
        state.closed = true;
        self.changed.notify_all();
    }
}

impl Budgets {
    pub fn open(&self, id: &TerminalId) -> Arc<Budget> {
        let budget = Arc::new(Budget::default());
        let mut budgets = self.budgets.lock().expect("budgets lock");
        budgets.insert(id.clone(), budget.clone());
        budget
    }

    pub fn give_back(&self, id: &TerminalId, bytes: usize) {
        let budgets = self.budgets.lock().expect("budgets lock");
        if let Some(budget) = budgets.get(id) {
            budget.give_back(bytes);
        }
    }

    pub fn forget(&self, id: &TerminalId) {
        let mut budgets = self.budgets.lock().expect("budgets lock");
        budgets.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    const HOLD_CHECK: Duration = Duration::from_millis(50);

    /// A reader on its own thread, which says when it has room to read again.
    fn waiting_reader(budget: &Arc<Budget>) -> (mpsc::Receiver<()>, thread::JoinHandle<()>) {
        let (done, finished) = mpsc::channel();
        let shared = budget.clone();
        let handle = thread::spawn(move || {
            shared.wait_for_room();
            let _ = done.send(());
        });
        (finished, handle)
    }

    #[test]
    fn a_full_budget_holds_the_reader_until_bytes_come_back_or_the_shell_exits() {
        let budget = Arc::new(Budget::default());
        budget.spend(BUDGET_BYTES);
        let (finished, reader) = waiting_reader(&budget);
        // Long enough for a reader let through by mistake to say so.
        let woke = finished.recv_timeout(HOLD_CHECK);
        assert!(woke.is_err(), "a full budget lets no more be read");
        budget.give_back(BUDGET_BYTES / 2);
        reader.join().unwrap();

        budget.spend(BUDGET_BYTES);
        let (_, reader) = waiting_reader(&budget);
        // An exited shell's last output is read whatever is outstanding.
        budget.close();
        reader.join().unwrap();
    }
}
