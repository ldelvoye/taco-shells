//! What the terminal view is told to show, and the bytes the line-editing commands type.

use taco_protocol::{DrawnState, TerminalId};
use taco_rules::selected_terminal;
use taco_settings::CommandId;

const KILL_LINE: &[u8] = b"\x15";
const LINE_START: &[u8] = b"\x01";
const LINE_END: &[u8] = b"\x05";
const WORD_LEFT: &[u8] = b"\x1bb";
const WORD_RIGHT: &[u8] = b"\x1bf";
const INSERT_NEWLINE: &[u8] = b"\x1b\r";

/// The terminals the view may attach to, and the one in the middle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attachable {
    pub open: Vec<TerminalId>,
    pub on_screen: Option<TerminalId>,
}

/// Only terminals the runtime has reported started: an attach sent before, on its own connection, could reach the runtime ahead of the open and be dropped.
pub fn attachable(drawn: &DrawnState) -> Attachable {
    let Some(arrangement) = &drawn.arrangement else {
        return Attachable::default();
    };
    let rows = arrangement.rows.iter().map(|row| &row.terminal);
    let started = rows.filter(|terminal| drawn.started.contains(*terminal));
    let open: Vec<TerminalId> = started.cloned().collect();
    let selected = selected_terminal(Some(arrangement));
    let mut on_screen = None;
    if let Some(selected) = selected
        && open.contains(selected)
    {
        on_screen = Some(selected.clone());
    }
    Attachable { open, on_screen }
}

/// The keys a line-editing command types: a shell's usual keys for it.
pub fn line_editing_bytes(command: CommandId) -> Option<&'static [u8]> {
    let bytes = match command {
        CommandId::TerminalKillLine => KILL_LINE,
        CommandId::TerminalLineStart => LINE_START,
        CommandId::TerminalLineEnd => LINE_END,
        CommandId::TerminalWordLeft => WORD_LEFT,
        CommandId::TerminalWordRight => WORD_RIGHT,
        CommandId::TerminalInsertNewline => INSERT_NEWLINE,
        _ => return None,
    };
    Some(bytes)
}

/// Whether a link may open from a terminal: the web and mail, never a file or an app's own scheme.
pub fn opens_link(url: &str) -> bool {
    let lowered = url.to_lowercase();
    let schemes = ["http://", "https://", "mailto:"];
    schemes.iter().any(|scheme| lowered.starts_with(scheme))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use taco_protocol::{Arrangement, DrawnState, Row, TerminalId};
    use taco_rules::default_panels;

    use super::{Attachable, attachable, opens_link};

    fn terminal(name: &str) -> TerminalId {
        TerminalId(name.to_string())
    }

    fn drawn_with(started: &[&str], selected: usize) -> DrawnState {
        let rows = ["a", "b", "c"].map(|name| Row {
            terminal: terminal(name),
            title: name.to_string(),
            tab_group: None,
        });
        let arrangement = Arrangement {
            rows: rows.to_vec(),
            selected,
            tab_groups: Vec::new(),
        };
        let directories = rows.map(|row| (row.terminal, "/".to_string()));
        let started_ids = started.iter().map(|name| terminal(name));
        let started: BTreeSet<TerminalId> = started_ids.collect();
        DrawnState {
            revision: 1,
            arrangement: Some(arrangement),
            agent_rows: BTreeMap::new(),
            directories: BTreeMap::from(directories),
            refocus: 0,
            focus_count: 0,
            agents: BTreeMap::new(),
            default_agent_name: String::new(),
            panels: default_panels(),
            problem: None,
            started,
        }
    }

    #[test]
    fn a_terminal_is_attached_only_once_the_runtime_has_started_it() {
        let restored_not_started = drawn_with(&["a", "c"], 1);
        let expected = Attachable {
            open: vec![terminal("a"), terminal("c")],
            on_screen: None,
        };
        assert_eq!(attachable(&restored_not_started), expected);

        let started = drawn_with(&["a", "b", "c"], 1);
        let attach = attachable(&started);
        assert_eq!(attach.open, [terminal("a"), terminal("b"), terminal("c")]);
        assert_eq!(attach.on_screen, Some(terminal("b")));
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert!(opens_link("https://sentry.io/issues/1"));
        assert!(opens_link("HTTP://example.com"));
        assert!(opens_link("mailto:someone@example.com"));
        assert!(!opens_link("file:///etc/passwd"));
        assert!(!opens_link("javascript:alert(1)"));
        assert!(!opens_link("vscode://open"));
    }
}
