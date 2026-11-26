//! What a row in the list is called.

use std::path::Path;

use taco_settings::ShellRowName;

/// A row with no title, or a shell with no directory yet.
pub const UNTITLED_ROW: &str = "Terminal";
const HOME: &str = "~";
const ROOT: &str = "/";

/// A row's name: an agent's row keeps its title, and a plain shell's shows its folder, as the setting asks ("~" at home).
pub fn row_name(
    title: &str,
    has_agent: bool,
    directory: Option<&str>,
    home: Option<&Path>,
    shown: ShellRowName,
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
    match shown {
        ShellRowName::Folder => folder_name(folder),
        ShellRowName::Path => short_path(folder, home),
    }
}

fn folder_name(folder: &Path) -> String {
    let Some(last) = folder.file_name() else {
        return ROOT.to_string();
    };
    last.to_string_lossy().into_owned()
}

// Under home the path starts at "~"; anywhere else it is shown whole.
fn short_path(folder: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && let Ok(inside) = folder.strip_prefix(home)
    {
        let joined = Path::new(HOME).join(inside);
        return joined.to_string_lossy().into_owned();
    }
    let components = folder.components();
    let whole = components.as_path();
    whole.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use taco_settings::ShellRowName;

    use super::row_name;

    const SHELL_TITLE: &str = "someone@machine:~/code/taco-shells";

    #[test]
    fn a_plain_shell_is_named_by_its_folder_and_an_agent_by_its_title() {
        let home = Some(Path::new("/Users/someone"));
        let cases = [
            (Some("/Users/someone"), "~"),
            (Some("/Users/someone/code/taco-shells"), "taco-shells"),
            (Some("/Users/someone/code/taco-shells/"), "taco-shells"),
            (Some("/"), "/"),
            (None, "Terminal"),
        ];
        for (directory, expected) in cases {
            let named = row_name(SHELL_TITLE, false, directory, home, ShellRowName::Folder);
            assert_eq!(named, expected);
        }
        let agent = row_name(
            "claude: lock rework",
            true,
            Some("/Users/someone/code"),
            home,
            ShellRowName::Path,
        );
        assert_eq!(agent, "claude: lock rework");
    }

    #[test]
    fn a_short_path_starts_at_home_and_is_whole_outside_it() {
        let home = Some(Path::new("/Users/someone"));
        let cases = [
            (Some("/Users/someone"), "~"),
            (
                Some("/Users/someone/code/taco-shells"),
                "~/code/taco-shells",
            ),
            (
                Some("/Users/someone/code/taco-shells/"),
                "~/code/taco-shells",
            ),
            (Some("/Users/someoneelse/code"), "/Users/someoneelse/code"),
            (Some("/opt/homebrew/"), "/opt/homebrew"),
            (Some("/"), "/"),
        ];
        for (directory, expected) in cases {
            let named = row_name(SHELL_TITLE, false, directory, home, ShellRowName::Path);
            assert_eq!(named, expected, "{directory:?}");
        }
    }
}
