use std::env;
use std::path::PathBuf;
use std::process::Command;

pub fn root() -> Result<PathBuf, String> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    match manifest_dir.parent() {
        Some(parent) => Ok(parent.to_path_buf()),
        None => Err(format!(
            "{} has no parent directory",
            manifest_dir.display()
        )),
    }
}

pub fn target_dir() -> Result<PathBuf, String> {
    let configured = env::var_os("CARGO_TARGET_DIR");
    let repo_root = root()?;
    match configured {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if dir.is_absolute() {
                return Ok(dir);
            }
            Ok(repo_root.join(dir))
        }
        None => Ok(repo_root.join("target")),
    }
}

pub fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|error| format!("could not start {program}: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!("{program} failed: {status}"))
}

pub fn capture(command: &mut Command) -> Option<String> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(stdout.trim().to_string())
}
