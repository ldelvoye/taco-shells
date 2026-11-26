use taco_protocol::{Arrangement, TabGroup, TabGroupId, TerminalId};

use crate::arrangement::{locate_row, reselect, selected_terminal, settled_if_changed};

pub const NEW_TAB_GROUP_NAME: &str = "New group";

// A row that leaves a tab group to start its own lands right after the rest of it, so both stay in one piece.
// `without_row` is the arrangement with the row already lifted out.
fn slot_after_old_tab_group(
    without_row: &Arrangement,
    old: Option<&TabGroupId>,
    from: usize,
) -> usize {
    let Some(old) = old else {
        return from;
    };
    let mut rows = without_row.rows.iter();
    let last = rows.rposition(|row| row.tab_group.as_ref() == Some(old));
    match last {
        Some(last) => last + 1,
        None => from,
    }
}

/// A new tab group holding this terminal's row, taken out of the tab group it was in.
pub fn create_tab_group(
    arrangement: &Arrangement,
    terminal: &TerminalId,
    id: TabGroupId,
) -> Option<Arrangement> {
    let from = locate_row(arrangement, terminal)?;
    let selected = selected_terminal(Some(arrangement)).cloned();

    let mut after = arrangement.clone();
    let mut lifted = after.rows.remove(from);
    let slot = slot_after_old_tab_group(&after, lifted.tab_group.as_ref(), from);
    lifted.tab_group = Some(id.clone());
    after.rows.insert(slot, lifted);

    let record = TabGroup {
        id,
        name: NEW_TAB_GROUP_NAME.to_string(),
        collapsed: false,
    };
    after.tab_groups.push(record);
    reselect(&mut after, selected.as_ref());
    settled_if_changed(arrangement, after)
}

fn with_record_changed(
    arrangement: &Arrangement,
    id: &TabGroupId,
    change: impl FnOnce(&mut TabGroup),
) -> Option<Arrangement> {
    let mut after = arrangement.clone();
    let mut records = after.tab_groups.iter_mut();
    let record = records.find(|record| record.id == *id)?;
    change(record);
    settled_if_changed(arrangement, after)
}

pub fn rename_tab_group(
    arrangement: &Arrangement,
    id: &TabGroupId,
    name: &str,
) -> Option<Arrangement> {
    with_record_changed(arrangement, id, |record| record.name = name.to_string())
}

/// Collapses an open tab group or opens a collapsed one; a selection inside it moves to the nearest row still showing.
pub fn toggle_tab_group(arrangement: &Arrangement, id: &TabGroupId) -> Option<Arrangement> {
    with_record_changed(arrangement, id, |record| {
        record.collapsed = !record.collapsed
    })
}

/// The arrangement with this tab group gone and its rows left where they are, in no group.
pub fn ungroup_tab_group(arrangement: &Arrangement, id: &TabGroupId) -> Option<Arrangement> {
    let mut after = arrangement.clone();
    for row in &mut after.rows {
        if row.tab_group.as_ref() == Some(id) {
            row.tab_group = None;
        }
    }
    settled_if_changed(arrangement, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{built, id, shown, tab};

    #[test]
    fn a_new_tab_group_takes_its_row_out_of_the_group_it_was_in_and_keeps_the_selection() {
        let list = built("a g:b g:c g:d e", "e");

        let from_middle = create_tab_group(&list, &id("c"), tab("h")).unwrap();
        assert_eq!(shown(&from_middle), "a g:b g:d h:c [e]");
        let from_none = create_tab_group(&list, &id("a"), tab("h")).unwrap();
        assert_eq!(shown(&from_none), "h:a g:b g:c g:d [e]");
        let alone = create_tab_group(&built("g:b", "g:b"), &id("b"), tab("h")).unwrap();
        assert_eq!(shown(&alone), "[h:b]");
        assert_eq!(alone.tab_groups.len(), 1);
        let selection_moved = create_tab_group(&built("a g:b g:c g:d", "g:d"), &id("c"), tab("h"));
        assert_eq!(shown(&selection_moved.unwrap()), "a g:b [g:d] h:c");

        let collapsed = toggle_tab_group(&list, &tab("g")).unwrap();
        let reopened = toggle_tab_group(&collapsed, &tab("g")).unwrap();
        assert_eq!(shown(&reopened), shown(&list));

        let ungrouped = ungroup_tab_group(&list, &tab("g")).unwrap();
        assert_eq!(shown(&ungrouped), "a b c d [e]");
        assert!(ungrouped.tab_groups.is_empty());
    }
}
