use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::file_write::write_beside_then_rename;

const PROGRAM_MODE: u32 = 0o755;

fn problem(path: &Path, action: &str, error: io::Error) -> String {
    format!("could not {action} {}: {error}", path.display())
}

fn create_parent(target: &Path) -> Result<(), String> {
    let Some(parent) = target.parent() else {
        return Err(format!("{} has no folder", target.display()));
    };
    let created = fs::create_dir_all(parent);
    created.map_err(|error| problem(parent, "create", error))
}

fn differs(target: &Path, wanted: &[u8]) -> Result<bool, String> {
    let existing = fs::read(target);
    match existing {
        Ok(found) => Ok(found != wanted),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(problem(target, "read", error)),
    }
}

fn install_file(target: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), String> {
    create_parent(target)?;
    // Written beside and renamed over, never in place: a running server may hold the old program, and macOS kills later launches of a signed program rewritten in place.
    let written = write_beside_then_rename(target, bytes, mode);
    written.map_err(|error| problem(target, "write", error))
}

fn refresh_text_file(target: &Path, content: &str) -> Result<bool, String> {
    let changed = differs(target, content.as_bytes())?;
    if !changed {
        return Ok(false);
    }
    install_file(target, content.as_bytes(), None)?;
    Ok(true)
}

fn repair_mode(target: &Path) -> Result<bool, String> {
    let found = fs::metadata(target).map_err(|error| problem(target, "read", error))?;
    let permissions = found.permissions();
    let mode = permissions.mode() & 0o777;
    if mode == PROGRAM_MODE {
        return Ok(false);
    }
    let executable = fs::Permissions::from_mode(PROGRAM_MODE);
    let marked = fs::set_permissions(target, executable);
    marked.map_err(|error| problem(target, "make executable", error))?;
    Ok(true)
}

fn refresh_program(target: &Path, program: &Path) -> Result<bool, String> {
    let read = fs::read(program);
    let wanted = read.map_err(|error| problem(program, "read", error))?;
    let changed = differs(target, &wanted)?;
    if !changed {
        return repair_mode(target);
    }
    install_file(target, &wanted, Some(PROGRAM_MODE))?;
    Ok(true)
}

pub fn refresh_plugin_folder(folder: &Path, program: &Path, version: &str) -> Result<bool, String> {
    let mut changed = false;
    for (relative, content) in taco_plugin::plugin_files(version) {
        let target = folder.join(relative);
        let written = refresh_text_file(&target, &content)?;
        if written {
            changed = true;
        }
    }
    let program_path = taco_plugin::program_path();
    let program_target = folder.join(program_path);
    let copied = refresh_program(&program_target, program)?;
    if copied {
        changed = true;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Read;
    use std::path::PathBuf;

    use super::*;

    const MANIFEST: &str = ".claude-plugin/plugin.json";
    const VERSION: &str = "2026.11.26";

    fn fake_program(directory: &Path, bytes: &[u8]) -> PathBuf {
        let path = directory.join("source-program");
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_fresh_folder_gets_the_files_and_an_executable_program() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_program(directory.path(), b"program one");
        let folder = directory.path().join("plugin");

        let changed = refresh_plugin_folder(&folder, &program, VERSION).unwrap();

        assert!(changed);
        assert!(folder.join(MANIFEST).is_file());
        assert!(folder.join(".mcp.json").is_file());
        assert!(folder.join("hooks/hooks.json").is_file());
        let program_path = taco_plugin::program_path();
        let installed = folder.join(program_path);
        assert_eq!(fs::read(&installed).unwrap(), b"program one");
        let mode = fs::metadata(&installed).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn a_second_call_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_program(directory.path(), b"program one");
        let folder = directory.path().join("plugin");
        refresh_plugin_folder(&folder, &program, VERSION).unwrap();

        let changed = refresh_plugin_folder(&folder, &program, VERSION).unwrap();

        assert!(!changed);
    }

    #[test]
    fn a_program_with_the_right_bytes_but_the_wrong_mode_is_made_executable() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_program(directory.path(), b"program one");
        let folder = directory.path().join("plugin");
        refresh_plugin_folder(&folder, &program, VERSION).unwrap();
        let program_path = taco_plugin::program_path();
        let installed = folder.join(program_path);
        fs::set_permissions(&installed, fs::Permissions::from_mode(0o644)).unwrap();

        let changed = refresh_plugin_folder(&folder, &program, VERSION).unwrap();

        assert!(changed);
        let mode = fs::metadata(&installed).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn a_changed_program_is_replaced_by_rename() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_program(directory.path(), b"old program");
        let folder = directory.path().join("plugin");
        refresh_plugin_folder(&folder, &program, VERSION).unwrap();
        let program_path = taco_plugin::program_path();
        let installed = folder.join(program_path);
        let mut running = File::open(&installed).unwrap();

        let newer = fake_program(directory.path(), b"new program, longer");
        let changed = refresh_plugin_folder(&folder, &newer, VERSION).unwrap();

        assert!(changed);
        let mut seen_by_handle = Vec::new();
        running.read_to_end(&mut seen_by_handle).unwrap();
        assert_eq!(seen_by_handle, b"old program");
        assert_eq!(fs::read(&installed).unwrap(), b"new program, longer");
    }

    #[test]
    fn an_edited_manifest_is_restored() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_program(directory.path(), b"program one");
        let folder = directory.path().join("plugin");
        refresh_plugin_folder(&folder, &program, VERSION).unwrap();
        let manifest = folder.join(MANIFEST);
        let original = fs::read_to_string(&manifest).unwrap();
        fs::write(&manifest, "{}").unwrap();

        let changed = refresh_plugin_folder(&folder, &program, VERSION).unwrap();

        assert!(changed);
        assert_eq!(fs::read_to_string(&manifest).unwrap(), original);
    }
}
