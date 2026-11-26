use taco_protocol::{Arrangement, TabGroupId, TerminalId};

pub fn empty_arrangement() -> Arrangement {
    Arrangement {
        rows: Vec::new(),
        selected: 0,
        tab_groups: Vec::new(),
    }
}

/// The index of the row holding this terminal, or none when it is not open.
pub fn locate_row(arrangement: &Arrangement, terminal: &TerminalId) -> Option<usize> {
    let mut rows = arrangement.rows.iter();
    rows.position(|row| row.terminal == *terminal)
}

/// The first and last index of a tab group's rows, or none when it has none.
pub fn block_of(arrangement: &Arrangement, tab_group: &TabGroupId) -> Option<(usize, usize)> {
    let mut first = None;
    let mut last = None;
    for (index, row) in arrangement.rows.iter().enumerate() {
        if row.tab_group.as_ref() == Some(tab_group) {
            if first.is_none() {
                first = Some(index);
            }
            last = Some(index);
        }
    }
    Some((first?, last?))
}

/// Whether the row at this index sits in a collapsed tab group.
pub fn is_hidden(arrangement: &Arrangement, index: usize) -> bool {
    let Some(row) = arrangement.rows.get(index) else {
        return false;
    };
    let Some(tab_group) = &row.tab_group else {
        return false;
    };
    let mut records = arrangement.tab_groups.iter();
    let record = records.find(|record| record.id == *tab_group);
    record.is_some_and(|record| record.collapsed)
}

/// The terminal that has the keyboard, or none before there is one.
pub fn selected_terminal(arrangement: Option<&Arrangement>) -> Option<&TerminalId> {
    let arrangement = arrangement?;
    let row = arrangement.rows.get(arrangement.selected)?;
    Some(&row.terminal)
}

/// The arrangement with the tab group of the row at this index opened, if it was collapsed.
pub(crate) fn open_at(arrangement: &mut Arrangement, index: usize) {
    if !is_hidden(arrangement, index) {
        return;
    }
    let tab_group = arrangement.rows[index].tab_group.clone();
    for record in &mut arrangement.tab_groups {
        if Some(&record.id) == tab_group.as_ref() {
            record.collapsed = false;
        }
    }
}

fn nearest_showing(arrangement: &Arrangement) -> Option<usize> {
    let after = (arrangement.selected + 1)..arrangement.rows.len();
    let before = (0..arrangement.selected).rev();
    let mut candidates = after.chain(before);
    candidates.find(|index| !is_hidden(arrangement, *index))
}

/// Empty tab groups dropped and the selection moved off a hidden row. Every change ends here.
pub(crate) fn settle(arrangement: &mut Arrangement) {
    let rows = &arrangement.rows;
    arrangement.tab_groups.retain(|record| {
        let mut holders = rows.iter();
        holders.any(|row| row.tab_group.as_ref() == Some(&record.id))
    });

    if !is_hidden(arrangement, arrangement.selected) {
        return;
    }
    if let Some(showing) = nearest_showing(arrangement) {
        arrangement.selected = showing;
    }
}

/// The settled result of a change, or none when it changed nothing.
pub(crate) fn changed(before: &Arrangement, mut after: Arrangement) -> Option<Arrangement> {
    settle(&mut after);
    if after == *before {
        return None;
    }
    Some(after)
}
