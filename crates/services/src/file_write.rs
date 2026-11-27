use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SYMLINK_HOPS: usize = 16;

static TEMPORARY_WRITES: AtomicU64 = AtomicU64::new(0);

fn link_target(link: &Path, target: PathBuf) -> PathBuf {
    if target.is_absolute() {
        return target;
    }
    match link.parent() {
        Some(directory) => directory.join(target),
        None => target,
    }
}

pub(crate) fn real_file_of(path: &Path) -> io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_HOPS {
        let metadata = fs::symlink_metadata(&current);
        match metadata {
            Ok(found) if found.file_type().is_symlink() => {}
            Ok(_) => return Ok(current),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error),
        }
        let target = fs::read_link(&current)?;
        current = link_target(&current, target);
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

fn write_then_rename(
    target: &Path,
    temporary: &Path,
    bytes: &[u8],
    mode: Option<u32>,
) -> io::Result<()> {
    fs::write(temporary, bytes)?;
    if let Some(bits) = mode {
        let permissions = fs::Permissions::from_mode(bits);
        fs::set_permissions(temporary, permissions)?;
    }
    fs::rename(temporary, target)
}

pub(crate) fn write_beside_then_rename(
    target: &Path,
    bytes: &[u8],
    mode: Option<u32>,
) -> io::Result<()> {
    let Some(name) = target.file_name() else {
        return Err(io::Error::other("it has no file name"));
    };
    let Some(directory) = target.parent() else {
        return Err(io::Error::other("it has no directory"));
    };
    let shown_name = name.to_string_lossy();
    let process = std::process::id();
    let counter = TEMPORARY_WRITES.fetch_add(1, Ordering::Relaxed);
    let temporary_name = format!(".{shown_name}.{process}.{counter}.taco-tmp");
    let temporary = directory.join(temporary_name);
    let replaced = write_then_rename(target, &temporary, bytes, mode);
    if replaced.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    replaced
}

pub(crate) fn replace_file(real: &Path, text: &str) -> io::Result<()> {
    let existing = fs::metadata(real);
    let mode = match existing {
        Ok(found) => Some(found.permissions().mode()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    write_beside_then_rename(real, text.as_bytes(), mode)
}

pub(crate) fn read_text(path: &Path, shown: &str) -> Result<Option<String>, String> {
    let read = fs::read_to_string(path);
    match read {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{shown} could not be read: {error}")),
    }
}

pub(crate) fn write_if_unchanged(
    path: &Path,
    shown: &str,
    original: Option<&str>,
    text: &str,
) -> Result<(), String> {
    let current = read_text(path, shown);
    let still_original = match &current {
        Ok(found) => found.as_deref() == original,
        Err(_) => false,
    };
    if !still_original {
        return Err(format!(
            "{shown} changed while it was being edited, so nothing was changed."
        ));
    }

    // Atomic replace beside the symlink's target, so the link stays a link.
    let real = real_file_of(path);
    let written = real.and_then(|real| replace_file(&real, text));
    match written {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("{shown} could not be written: {error}")),
    }
}
