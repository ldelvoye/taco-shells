use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use taco_agents::{HARNESSES, Harness, Version};

use super::{Shared, refresh};
use crate::agent_version::{find_program, program_version};
use crate::sync::lock;

const VERSION_THREAD_NAME: &str = "agent-version";

pub(super) fn installed_version(home: Option<&Path>, harness: &'static Harness) -> Option<Version> {
    let home = home?;
    let setup = harness.setup()?;
    let command = setup.commands[0];
    let program = find_program(home, command)?;
    program_version(&program, harness)
}

pub(super) fn probed_harness() -> Option<&'static Harness> {
    for harness in HARNESSES {
        if harness.setup().is_some() {
            return Some(*harness);
        }
    }
    None
}

pub(super) fn current_version(shared: &Shared) -> Option<Version> {
    let version = lock(&shared.version);
    *version
}

fn find_version_now(shared: &Shared) {
    let found = (shared.find_version)();
    {
        let mut version = lock(&shared.version);
        *version = found;
    }
    shared.checking_version.store(false, Ordering::SeqCst);
    if !shared.stopping.load(Ordering::SeqCst) {
        refresh(shared);
    }
}

pub(super) fn check_version(shared: &Arc<Shared>) {
    let claimed =
        shared
            .checking_version
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst);
    if claimed.is_err() {
        return;
    }
    let for_thread = Arc::clone(shared);
    let builder = thread::Builder::new().name(VERSION_THREAD_NAME.to_string());
    // Off the caller's thread: this runs the user's login shell, and a slow startup file must cost a late line, never a late window.
    let spawned = builder.spawn(move || find_version_now(&for_thread));
    if spawned.is_err() {
        shared.checking_version.store(false, Ordering::SeqCst);
    }
}
