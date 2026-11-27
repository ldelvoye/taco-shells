use std::path::Path;

use taco_protocol::TOKEN_FILE_NAME;

pub fn read_token(port_path: &Path) -> Option<String> {
    let token_path = port_path.with_file_name(TOKEN_FILE_NAME);
    let content = std::fs::read_to_string(token_path).ok()?;
    let first_line = content.lines().next()?;
    let trimmed = first_line.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(String::from(trimmed))
}

pub fn read_port(path: &Path) -> Option<u16> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    let first_line = lines.next()?;
    let trimmed = first_line.trim();
    trimmed.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use taco_protocol::PORT_FILE_NAME;

    #[test]
    fn the_token_is_the_first_line_of_the_file_beside_the_port_file() {
        let directory = tempfile::tempdir().unwrap();
        let port_path = directory.path().join(PORT_FILE_NAME);
        assert_eq!(read_token(&port_path), None);

        let token_path = directory.path().join(TOKEN_FILE_NAME);
        std::fs::write(token_path, "abc123\nignored\n").unwrap();
        assert_eq!(read_token(&port_path), Some(String::from("abc123")));
    }
}
