use taco_protocol::{Arrangement, FocusMove, Row, TabGroupId, TerminalId};

use crate::arrangement::{block_of, changed, is_hidden, locate_row, open_at};

fn new_row(terminal: TerminalId, tab_group: Option<TabGroupId>) -> Row {
    Row {
        terminal,
        title: String::new(),
        tab_group,
    }
}

/// A new selected row, last in the selected row's tab group, or last in the list when it has none.
pub fn add_row(arrangement: &Arrangement, terminal: TerminalId) -> Option<Arrangement> {
    let selected = arrangement.rows.get(arrangement.selected);
    let tab_group = selected.and_then(|row| row.tab_group.clone());
    add_row_to(arrangement, terminal, tab_group)
}

/// A new selected row, last in this tab group, or last and in none when the group is none or gone.
pub fn add_row_to(
    arrangement: &Arrangement,
    terminal: TerminalId,
    tab_group: Option<TabGroupId>,
) -> Option<Arrangement> {
    let mut placed_in = None;
    let mut index = arrangement.rows.len();
    if let Some(tab_group) = tab_group
        && let Some((_, last)) = block_of(arrangement, &tab_group)
    {
        placed_in = Some(tab_group);
        index = last + 1;
    }

    let mut after = arrangement.clone();
    after.rows.insert(index, new_row(terminal, placed_in));
    after.selected = index;
    open_at(&mut after, index);
    changed(arrangement, after)
}

// Where a selected index lands once the row at `removed` is taken out.
fn index_after_removal(selected: usize, removed: usize, remaining: usize) -> usize {
    if removed < selected {
        return selected - 1;
    }
    if removed > selected {
        return selected;
    }
    let last = remaining.saturating_sub(1);
    selected.min(last)
}

/// The arrangement without this terminal's row. The selection moves to the row after it, or the one before at the end.
pub fn close_row(arrangement: &Arrangement, terminal: &TerminalId) -> Option<Arrangement> {
    let index = locate_row(arrangement, terminal)?;
    let mut after = arrangement.clone();
    after.rows.remove(index);
    after.selected = index_after_removal(arrangement.selected, index, after.rows.len());
    changed(arrangement, after)
}

pub fn rename_row(
    arrangement: &Arrangement,
    terminal: &TerminalId,
    title: &str,
) -> Option<Arrangement> {
    let index = locate_row(arrangement, terminal)?;
    let mut after = arrangement.clone();
    after.rows[index].title = title.to_string();
    changed(arrangement, after)
}

/// The arrangement with this terminal's row selected, its tab group opened if it was collapsed.
pub fn select_row(arrangement: &Arrangement, terminal: &TerminalId) -> Option<Arrangement> {
    let index = locate_row(arrangement, terminal)?;
    let mut after = arrangement.clone();
    after.selected = index;
    open_at(&mut after, index);
    changed(arrangement, after)
}

/// A fork's row, right under its source in the source's tab group, and selected.
pub fn fork_row(
    arrangement: &Arrangement,
    source: &TerminalId,
    fork: TerminalId,
) -> Option<Arrangement> {
    let index = locate_row(arrangement, source)?;
    let tab_group = arrangement.rows[index].tab_group.clone();
    let fork_index = index + 1;

    let mut after = arrangement.clone();
    after.rows.insert(fork_index, new_row(fork, tab_group));
    after.selected = fork_index;
    open_at(&mut after, fork_index);
    changed(arrangement, after)
}

/// The terminal a focus move goes to, skipping rows hidden in collapsed tab groups and wrapping at the ends.
pub fn focus_target(arrangement: &Arrangement, focus_move: FocusMove) -> Option<TerminalId> {
    let count = arrangement.rows.len();
    for step in 1..count {
        let ahead = match focus_move {
            FocusMove::NextRow => step,
            FocusMove::PreviousRow => count - step,
        };
        let index = (arrangement.selected + ahead) % count;
        if !is_hidden(arrangement, index) {
            return Some(arrangement.rows[index].terminal.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab_groups::toggle_tab_group;
    use crate::testing::{built, id, shown, tab};

    #[test]
    fn closing_collapsing_and_selecting_keep_the_selection_on_a_showing_row() {
        let list = built("a b g:c g:d e", "b");

        let closed_middle = close_row(&list, &id("b")).unwrap();
        assert_eq!(shown(&closed_middle), "a [g:c] g:d e");
        let closed_last = close_row(&built("a b", "b"), &id("b")).unwrap();
        assert_eq!(shown(&closed_last), "[a]");
        let closed_before = close_row(&list, &id("a")).unwrap();
        assert_eq!(shown(&closed_before), "[b] g:c g:d e");
        let emptied = close_row(&built("a", "a"), &id("a")).unwrap();
        assert_eq!(shown(&emptied), "");

        let in_group = built("a g:c g:d e", "g:d");
        let collapsed = toggle_tab_group(&in_group, &tab("g")).unwrap();
        assert_eq!(shown(&collapsed), "a g-:c g-:d [e]");
        let collapsed_at_end = toggle_tab_group(&built("a g:c", "g:c"), &tab("g")).unwrap();
        assert_eq!(shown(&collapsed_at_end), "[a] g-:c");

        let reopened = select_row(&collapsed, &id("c")).unwrap();
        assert_eq!(shown(&reopened), "a [g:c] g:d e");
        assert_eq!(select_row(&list, &id("b")), None);
        assert_eq!(close_row(&list, &id("gone")), None);
    }

    #[test]
    fn a_new_row_joins_the_tab_group_it_was_opened_from_and_a_fork_sits_under_its_source() {
        let list = built("a g:b g:c d", "g:b");

        let added = add_row(&list, id("n")).unwrap();
        assert_eq!(shown(&added), "a g:b g:c [g:n] d");
        let added_outside = add_row(&built("a g:b", "a"), id("n")).unwrap();
        assert_eq!(shown(&added_outside), "a g:b [n]");
        let added_to_gone = add_row_to(&list, id("n"), Some(tab("gone"))).unwrap();
        assert_eq!(shown(&added_to_gone), "a g:b g:c d [n]");

        let forked = fork_row(&list, &id("b"), id("f")).unwrap();
        assert_eq!(shown(&forked), "a g:b [g:f] g:c d");
        let collapsed = toggle_tab_group(&built("g:b x", "x"), &tab("g")).unwrap();
        let forked_hidden = fork_row(&collapsed, &id("b"), id("f")).unwrap();
        assert_eq!(shown(&forked_hidden), "g:b [g:f] x");
    }

    #[test]
    fn focus_moves_wrap_and_skip_rows_in_a_collapsed_group() {
        let list = toggle_tab_group(&built("a g:b g:c d", "d"), &tab("g")).unwrap();

        assert_eq!(focus_target(&list, FocusMove::NextRow), Some(id("a")));
        assert_eq!(focus_target(&list, FocusMove::PreviousRow), Some(id("a")));
        let from_a = select_row(&list, &id("a")).unwrap();
        assert_eq!(focus_target(&from_a, FocusMove::NextRow), Some(id("d")));
        assert_eq!(focus_target(&built("a", "a"), FocusMove::NextRow), None);
    }
}
