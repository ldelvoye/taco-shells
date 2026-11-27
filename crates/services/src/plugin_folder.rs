use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const PROGRAM_PATH: &str = "libexec/taco-shells-plugin";

const PROGRAM_MODE: u32 = 0o755;

fn problem(path: &Path, action: &str, error: io::Error) -> String {
    format!("could not {action} {}: {error}", path.display())
}

fn temporary_beside(target: &Path) -> Result<PathBuf, String> {
    let Some(name) = target.file_name() else {
        return Err(format!("{} has no file name", target.display()));
    };
    let shown_name = name.to_string_lossy();
    let process = std::process::id();
    let temporary_name = format!(".{shown_name}.{process}.taco-tmp");
    Ok(target.with_file_name(temporary_name))
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

fn rename_over(temporary: &Path, target: &Path) -> Result<(), String> {
    // Written beside and renamed over, never in place: a running server may hold the old program, and macOS kills later launches of a signed program rewritten in place.
    let renamed = fs::rename(temporary, target);
    if let Err(error) = renamed {
        let _ = fs::remove_file(temporary);
        return Err(problem(target, "replace", error));
    }
    Ok(())
}

fn refresh_text_file(target: &Path, content: &str) -> Result<bool, String> {
    let changed = differs(target, content.as_bytes())?;
    if !changed {
        return Ok(false);
    }
    create_parent(target)?;
    let temporary = temporary_beside(target)?;
    let written = fs::write(&temporary, content);
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(problem(&temporary, "write", error));
    }
    rename_over(&temporary, target)?;
    Ok(true)
}

fn refresh_program(target: &Path, program: &Path) -> Result<bool, String> {
    let wanted = fs::read(program).map_err(|error| problem(program, "read", error))?;
    let changed = differs(target, &wanted)?;
    if !changed {
        return Ok(false);
    }
    create_parent(target)?;
    let temporary = temporary_beside(target)?;
    let copied = fs::copy(program, &temporary);
    if let Err(error) = copied {
        let _ = fs::remove_file(&temporary);
        return Err(problem(&temporary, "copy the program to", error));
    }
    let executable = fs::Permissions::from_mode(PROGRAM_MODE);
    let marked = fs::set_permissions(&temporary, executable);
    if let Err(error) = marked {
        let _ = fs::remove_file(&temporary);
        return Err(problem(&temporary, "make executable", error));
    }
    rename_over(&temporary, target)?;
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
    let program_target = folder.join(PROGRAM_PATH);
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
    use std::thread;
    use std::time::Duration;

    use super::*;

    const MANIFEST: &str = ".claude-plugin/plugin.json";
    const VERSION: &str = "2026.11.26";

    fn fake_program(directory: &Path, bytes: &[u8]) -> PathBuf {
        let path = directory.join("source-program");
        fs::write(&path, bytes).unwrap();
        path
    }

    fn modified(path: &Path) -> std::time::SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
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
        let installed = folder.join(PROGRAM_PATH);
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
        let paths = [
            folder.join(MANIFEST),
            folder.join(".mcp.json"),
            folder.join("hooks/hooks.json"),
            folder.join(PROGRAM_PATH),
        ];
        let before: Vec<_> = paths.iter().map(|path| modified(path)).collect();
        thread::sleep(Duration::from_millis(20));

        let changed = refresh_plugin_folder(&folder, &program, VERSION).unwrap();

        assert!(!changed);
        let after: Vec<_> = paths.iter().map(|path| modified(path)).collect();
        assert_eq!(before, after);
    }

    #[test]
    fn a_changed_program_is_replaced_by_rename() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_program(directory.path(), b"old program");
        let folder = directory.path().join("plugin");
        refresh_plugin_folder(&folder, &program, VERSION).unwrap();
        let installed = folder.join(PROGRAM_PATH);
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
