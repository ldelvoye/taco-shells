use std::path::Path;

/// The config directory a build bakes in: none for a stable build whatever was asked, else the asked-for path, which must be absolute.
pub fn baked_config_directory(
    is_stable: bool,
    asked: Option<&str>,
) -> Result<Option<String>, String> {
    if is_stable {
        return Ok(None);
    }
    let Some(path) = asked else {
        return Ok(None);
    };
    if path.is_empty() {
        return Err("TACO_SHELLS_DEV_CONFIG_DIR is set but empty".to_string());
    }
    let is_absolute = Path::new(path).is_absolute();
    if !is_absolute {
        return Err(format!(
            "TACO_SHELLS_DEV_CONFIG_DIR must be an absolute path, not {path}"
        ));
    }
    Ok(Some(path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_development_build_bakes_in_an_absolute_directory() {
        let stable = baked_config_directory(true, Some("/tmp/rehearsal"));
        assert_eq!(stable, Ok(None));

        let development = baked_config_directory(false, Some("/tmp/rehearsal"));
        assert_eq!(development, Ok(Some("/tmp/rehearsal".to_string())));

        let unset = baked_config_directory(false, None);
        assert_eq!(unset, Ok(None));

        let relative = baked_config_directory(false, Some("rehearsal"));
        assert!(relative.is_err());
        let empty = baked_config_directory(false, Some(""));
        assert!(empty.is_err());
    }
}
