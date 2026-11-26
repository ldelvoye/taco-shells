use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_SYMLINK_HOPS: usize = 16;

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

fn write_then_rename(real: &Path, temporary: &Path, text: &str) -> io::Result<()> {
    fs::write(temporary, text)?;
    let existing = fs::metadata(real);
    match existing {
        Ok(found) => fs::set_permissions(temporary, found.permissions())?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(temporary, real)
}

pub(crate) fn replace_file(real: &Path, text: &str) -> io::Result<()> {
    let Some(name) = real.file_name() else {
        return Err(io::Error::other("it has no file name"));
    };
    let Some(directory) = real.parent() else {
        return Err(io::Error::other("it has no directory"));
    };
    let shown_name = name.to_string_lossy();
    let process = std::process::id();
    let temporary_name = format!(".{shown_name}.{process}.taco-tmp");
    let temporary = directory.join(temporary_name);
    let replaced = write_then_rename(real, &temporary, text);
    if replaced.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    replaced
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

    // Written to a temporary file beside the real file (a symlink's target) and renamed over it, so a crash never leaves it half-written and a symlink stays one.
    let real = real_file_of(path);
    let written = real.and_then(|real| replace_file(&real, text));
    match written {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("{shown} could not be written: {error}")),
    }
}
