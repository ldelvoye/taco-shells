//! The Electron app's hook scripts: the folder its settings.json entries call, and the stubs that answer them until Set up removes those entries. The whole file goes in the release after cutover, so nothing new belongs here.

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const HOOKS_DIRECTORY_NAME: &str = "hooks";
const SCRIPT_MODE: u32 = 0o755;
const HOOK_SCRIPT_NAMES: [&str; 2] = ["taco-shells-attention.sh", "taco-shells-claude-session.sh"];
const STUB_SCRIPT: &str = "#!/bin/sh\nexit 0\n";

static STUB_WRITES: AtomicU64 = AtomicU64::new(0);

pub fn hooks_directory(config_directory: &Path) -> PathBuf {
    config_directory.join(HOOKS_DIRECTORY_NAME)
}

fn write_stub(target: &Path, name: &str) -> io::Result<()> {
    let process = std::process::id();
    let counter = STUB_WRITES.fetch_add(1, Ordering::Relaxed);
    let temporary_name = format!(".{name}.{process}.{counter}.taco-tmp");
    let temporary = target.with_file_name(temporary_name);
    let written = fs::write(&temporary, STUB_SCRIPT);
    let mode = fs::Permissions::from_mode(SCRIPT_MODE);
    let marked = written.and_then(|()| fs::set_permissions(&temporary, mode));
    let renamed = marked.and_then(|()| fs::rename(&temporary, target));
    if renamed.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    renamed
}

pub fn stub_hook_scripts(destination: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    for name in HOOK_SCRIPT_NAMES {
        let target = destination.join(name);
        let stubbed = write_stub(&target, name);
        if let Err(error) = stubbed {
            let shown = target.display();
            problems.push(format!("{shown} could not be replaced: {error}"));
        }
    }
    problems
}
