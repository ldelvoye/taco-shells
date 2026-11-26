//! What a row in the list is called.

use std::path::Path;

/// A row with no title, or a shell with no directory yet.
pub const UNTITLED_ROW: &str = "Terminal";
const HOME: &str = "~";
const ROOT: &str = "/";

/// A row's name: an agent's row keeps its title, and a plain shell's shows the last part of its folder ("~" at home).
pub fn row_name(
    title: &str,
    has_agent: bool,
    directory: Option<&str>,
    home: Option<&Path>,
) -> String {
    if has_agent {
        if title.is_empty() {
            return UNTITLED_ROW.to_string();
        }
        return title.to_string();
    }
    let Some(directory) = directory else {
        return UNTITLED_ROW.to_string();
    };
    let folder = Path::new(directory);
    if Some(folder) == home {
        return HOME.to_string();
    }
    let Some(last) = folder.file_name() else {
        return ROOT.to_string();
    };
    last.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::row_name;

    #[test]
    fn a_plain_shell_is_named_by_its_folder_and_an_agent_by_its_title() {
        let home = Some(Path::new("/Users/someone"));
        let shell_title = "someone@machine:~/code/taco-shells";
        let cases = [
            (Some("/Users/someone"), "~"),
            (Some("/Users/someone/code/taco-shells"), "taco-shells"),
            (Some("/Users/someone/code/taco-shells/"), "taco-shells"),
            (Some("/"), "/"),
            (None, "Terminal"),
        ];
        for (directory, expected) in cases {
            assert_eq!(row_name(shell_title, false, directory, home), expected);
        }
        let agent = row_name("claude: lock rework", true, Some("/Users/someone"), home);
        assert_eq!(agent, "claude: lock rework");
    }
}
