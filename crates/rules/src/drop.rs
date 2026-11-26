use taco_protocol::{Arrangement, Dragged, DropPlace, TabGroupId};

use crate::arrangement::{block_of, changed, locate_row};

/// The band at the top and bottom of a tab group's header that counts as outside the group.
pub const EDGE_BAND: f32 = 0.25;

/// What the pointer is over during a drag. `fraction` is how far down the row or header it is: 0 at its top, 1 at its bottom.
#[derive(Clone, Debug, PartialEq)]
pub enum DropPoint {
    Row {
        index: usize,
        fraction: f32,
    },
    Header {
        tab_group: TabGroupId,
        fraction: f32,
    },
    End,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MarkTarget {
    Row(usize),
    /// The tray drawn around a tab group's rows.
    Wrapper(TabGroupId),
    Header(TabGroupId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkEdge {
    Above,
    Below,
    Whole,
}

/// Where the window draws the line for a drop, and the tab group the dropped thing would end up in.
#[derive(Clone, Debug, PartialEq)]
pub struct DropMark {
    pub target: MarkTarget,
    pub edge: MarkEdge,
    pub tab_group: Option<TabGroupId>,
}

fn place(index: usize, tab_group: Option<TabGroupId>) -> DropPlace {
    DropPlace { index, tab_group }
}

fn is_collapsed(arrangement: &Arrangement, tab_group: &TabGroupId) -> Option<bool> {
    let mut records = arrangement.tab_groups.iter();
    let record = records.find(|record| record.id == *tab_group)?;
    Some(record.collapsed)
}

fn gap_inside_a_tab_group(arrangement: &Arrangement, gap: usize) -> bool {
    let mut records = arrangement.tab_groups.iter();
    records.any(|record| {
        let block = block_of(arrangement, &record.id);
        block.is_some_and(|(first, last)| first < gap && gap <= last)
    })
}

fn place_is_allowed(arrangement: &Arrangement, place: &DropPlace) -> bool {
    if place.index > arrangement.rows.len() {
        return false;
    }
    let Some(tab_group) = &place.tab_group else {
        return !gap_inside_a_tab_group(arrangement, place.index);
    };
    let Some((first, last)) = block_of(arrangement, tab_group) else {
        return false;
    };
    first <= place.index && place.index <= last + 1
}

// A row dropped where it already is, in the group it is already in, is no move, so it does not take the selection either.
fn drop_row(arrangement: &Arrangement, from: usize, place: &DropPlace) -> Option<Arrangement> {
    let mut to = place.index;
    if place.index > from {
        to -= 1;
    }
    let same_group = arrangement.rows[from].tab_group == place.tab_group;
    if to == from && same_group {
        return None;
    }

    let mut after = arrangement.clone();
    let mut row = after.rows.remove(from);
    row.tab_group = place.tab_group.clone();
    after.rows.insert(to, row);
    after.selected = to;
    Some(after)
}

fn drop_tab_group(
    arrangement: &Arrangement,
    id: &TabGroupId,
    place: &DropPlace,
) -> Option<Arrangement> {
    if place.tab_group.is_some() {
        return None;
    }
    let (first, last) = block_of(arrangement, id)?;
    let on_its_own_block_or_edge = first <= place.index && place.index <= last + 1;
    if on_its_own_block_or_edge {
        return None;
    }

    let selected = arrangement.rows[arrangement.selected].terminal.clone();
    let mut after = arrangement.clone();
    let block: Vec<_> = after.rows.drain(first..=last).collect();
    let mut to = place.index;
    if place.index > last {
        to -= block.len();
    }
    after.rows.splice(to..to, block);
    if let Some(index) = locate_row(&after, &selected) {
        after.selected = index;
    }
    Some(after)
}

/// The arrangement after a drop, or none when the drop is refused or changes nothing. A dropped row is selected.
pub fn apply_drop(
    arrangement: &Arrangement,
    dragged: &Dragged,
    place: &DropPlace,
) -> Option<Arrangement> {
    if !place_is_allowed(arrangement, place) {
        return None;
    }
    let after = match dragged {
        Dragged::Row(terminal) => {
            let from = locate_row(arrangement, terminal)?;
            drop_row(arrangement, from, place)?
        }
        Dragged::TabGroup(id) => drop_tab_group(arrangement, id, place)?,
    };
    changed(arrangement, after)
}

fn by_half(arrangement: &Arrangement, index: usize, fraction: f32) -> DropPlace {
    let tab_group = arrangement.rows[index].tab_group.clone();
    if fraction < 0.5 {
        return place(index, tab_group);
    }
    place(index + 1, tab_group)
}

// A header's top band is outside its group. The rest is inside: first when the group is open,
// last when it is collapsed, where the bottom band is outside again.
fn row_over_header(
    arrangement: &Arrangement,
    tab_group: &TabGroupId,
    fraction: f32,
) -> Option<DropPlace> {
    let collapsed = is_collapsed(arrangement, tab_group)?;
    let (first, last) = block_of(arrangement, tab_group)?;
    if fraction < EDGE_BAND {
        return Some(place(first, None));
    }
    if !collapsed {
        return Some(place(first, Some(tab_group.clone())));
    }
    if fraction > 1.0 - EDGE_BAND {
        return Some(place(last + 1, None));
    }
    Some(place(last + 1, Some(tab_group.clone())))
}

fn candidate_for_row(arrangement: &Arrangement, point: &DropPoint) -> Option<DropPlace> {
    match point {
        DropPoint::End => Some(place(arrangement.rows.len(), None)),
        DropPoint::Header {
            tab_group,
            fraction,
        } => row_over_header(arrangement, tab_group, *fraction),
        DropPoint::Row { index, fraction } => {
            arrangement.rows.get(*index)?;
            Some(by_half(arrangement, *index, *fraction))
        }
    }
}

// A dragged tab group lands above or below the whole group under the pointer, by which half of it the pointer is in.
fn above_or_below_tab_group(
    arrangement: &Arrangement,
    tab_group: &TabGroupId,
    line: usize,
    fraction: f32,
) -> Option<DropPlace> {
    let collapsed = is_collapsed(arrangement, tab_group)?;
    let (first, last) = block_of(arrangement, tab_group)?;
    let mut line_count = 1;
    if !collapsed {
        line_count += last - first + 1;
    }

    let position = (line as f32 + fraction) / line_count as f32;
    if position < 0.5 {
        return Some(place(first, None));
    }
    Some(place(last + 1, None))
}

fn candidate_for_tab_group(arrangement: &Arrangement, point: &DropPoint) -> Option<DropPlace> {
    match point {
        DropPoint::End => Some(place(arrangement.rows.len(), None)),
        DropPoint::Header {
            tab_group,
            fraction,
        } => above_or_below_tab_group(arrangement, tab_group, 0, *fraction),
        DropPoint::Row { index, fraction } => {
            let row = arrangement.rows.get(*index)?;
            let Some(tab_group) = &row.tab_group else {
                return Some(by_half(arrangement, *index, *fraction));
            };
            let (first, _) = block_of(arrangement, tab_group)?;
            let line = 1 + index - first;
            above_or_below_tab_group(arrangement, tab_group, line, *fraction)
        }
    }
}

/// What dropping at this point would do, or none when it would do nothing.
pub fn drop_for(
    arrangement: &Arrangement,
    dragged: &Dragged,
    point: &DropPoint,
) -> Option<DropPlace> {
    let candidate = match dragged {
        Dragged::Row(_) => candidate_for_row(arrangement, point),
        Dragged::TabGroup(_) => candidate_for_tab_group(arrangement, point),
    }?;
    apply_drop(arrangement, dragged, &candidate)?;
    Some(candidate)
}

fn mark(target: MarkTarget, edge: MarkEdge, tab_group: Option<TabGroupId>) -> DropMark {
    DropMark {
        target,
        edge,
        tab_group,
    }
}

fn mark_in_no_group(arrangement: &Arrangement, index: usize) -> Option<DropMark> {
    let rows = &arrangement.rows;
    if index == rows.len() {
        let last_index = rows.len().checked_sub(1)?;
        let mark_at = match &rows[last_index].tab_group {
            Some(tab_group) => MarkTarget::Wrapper(tab_group.clone()),
            None => MarkTarget::Row(last_index),
        };
        return Some(mark(mark_at, MarkEdge::Below, None));
    }

    if let Some(tab_group) = &rows[index].tab_group
        && block_of(arrangement, tab_group).is_some_and(|(first, _)| first == index)
    {
        return Some(mark(
            MarkTarget::Wrapper(tab_group.clone()),
            MarkEdge::Above,
            None,
        ));
    }
    Some(mark(MarkTarget::Row(index), MarkEdge::Above, None))
}

fn mark_in_tab_group(
    arrangement: &Arrangement,
    tab_group: &TabGroupId,
    index: usize,
) -> Option<DropMark> {
    let collapsed = is_collapsed(arrangement, tab_group)?;
    let (first, last) = block_of(arrangement, tab_group)?;
    let inside = Some(tab_group.clone());
    let header = MarkTarget::Header(tab_group.clone());

    if collapsed {
        return Some(mark(header, MarkEdge::Whole, inside));
    }
    if index == first {
        return Some(mark(header, MarkEdge::Below, inside));
    }
    if index > last {
        return Some(mark(MarkTarget::Row(last), MarkEdge::Below, inside));
    }
    Some(mark(MarkTarget::Row(index), MarkEdge::Above, inside))
}

pub fn drop_mark_for(arrangement: &Arrangement, place: &DropPlace) -> Option<DropMark> {
    match &place.tab_group {
        None => mark_in_no_group(arrangement, place.index),
        Some(tab_group) => mark_in_tab_group(arrangement, tab_group, place.index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab_groups::toggle_tab_group;
    use crate::testing::{built, id, shown, tab};

    fn row_at(index: usize, fraction: f32) -> DropPoint {
        DropPoint::Row { index, fraction }
    }

    fn header(name: &str, fraction: f32) -> DropPoint {
        DropPoint::Header {
            tab_group: tab(name),
            fraction,
        }
    }

    #[test]
    fn a_dragged_row_lands_by_the_half_under_the_pointer_and_a_header_decides_its_group() {
        let open = built("a g:b g:c d", "a");
        let collapsed = toggle_tab_group(&open, &tab("g")).unwrap();
        let dragging_d = Dragged::Row(id("d"));
        let dropped = |list: &Arrangement, point: DropPoint| {
            let place = drop_for(list, &dragging_d, &point)?;
            let after = apply_drop(list, &dragging_d, &place)?;
            Some(shown(&after))
        };

        assert_eq!(dropped(&open, row_at(0, 0.2)), Some("[d] a g:b g:c".into()));
        assert_eq!(
            dropped(&open, row_at(1, 0.7)),
            Some("a g:b [g:d] g:c".into())
        );
        assert_eq!(
            dropped(&open, header("g", 0.1)),
            Some("a [d] g:b g:c".into())
        );
        assert_eq!(
            dropped(&open, header("g", 0.5)),
            Some("a [g:d] g:b g:c".into())
        );
        assert_eq!(
            dropped(&collapsed, header("g", 0.1)),
            Some("a [d] g-:b g-:c".into())
        );
        // landing in a collapsed group moves the selection to a row still showing
        assert_eq!(
            dropped(&collapsed, header("g", 0.5)),
            Some("[a] g-:b g-:c g-:d".into())
        );
        assert_eq!(dropped(&collapsed, header("g", 0.9)), None);
        assert_eq!(dropped(&open, DropPoint::End), None);
        assert_eq!(dropped(&open, row_at(3, 0.2)), None);
    }

    #[test]
    fn a_dragged_tab_group_moves_whole_and_never_into_another_group() {
        let list = built("a g:b g:c h:d h:e f", "f");
        let dragging_g = Dragged::TabGroup(tab("g"));
        let dropped = |point: DropPoint| {
            let place = drop_for(&list, &dragging_g, &point)?;
            let after = apply_drop(&list, &dragging_g, &place)?;
            Some(shown(&after))
        };

        assert_eq!(
            dropped(DropPoint::End),
            Some("a h:d h:e [f] g:b g:c".into())
        );
        assert_eq!(dropped(row_at(3, 0.2)), None);
        assert_eq!(
            dropped(row_at(4, 0.6)),
            Some("a h:d h:e g:b g:c [f]".into())
        );
        assert_eq!(
            dropped(row_at(5, 0.2)),
            Some("a h:d h:e g:b g:c [f]".into())
        );
        assert_eq!(dropped(header("h", 0.9)), None);
        assert_eq!(dropped(row_at(0, 0.6)), None);

        let into_h = place(4, Some(tab("h")));
        assert_eq!(apply_drop(&list, &dragging_g, &into_h), None);
        let splitting_h = place(4, None);
        assert_eq!(
            apply_drop(&list, &Dragged::Row(id("a")), &splitting_h),
            None
        );
    }

    #[test]
    fn a_drop_line_sits_on_the_row_tray_or_header_the_drop_lands_by() {
        let list = built("a g:b g:c d", "a");
        let collapsed = toggle_tab_group(&list, &tab("g")).unwrap();
        let line = |list: &Arrangement, index: usize, tab_group: Option<&str>| {
            let target = place(index, tab_group.map(tab));
            drop_mark_for(list, &target)
        };
        let tray = || MarkTarget::Wrapper(tab("g"));
        let head = || MarkTarget::Header(tab("g"));
        let inside = || Some(tab("g"));

        assert_eq!(
            line(&list, 0, None),
            Some(mark(MarkTarget::Row(0), MarkEdge::Above, None))
        );
        assert_eq!(
            line(&list, 1, None),
            Some(mark(tray(), MarkEdge::Above, None))
        );
        assert_eq!(
            line(&list, 4, None),
            Some(mark(MarkTarget::Row(3), MarkEdge::Below, None))
        );
        assert_eq!(
            line(&built("a g:b", "a"), 2, None),
            Some(mark(tray(), MarkEdge::Below, None))
        );
        assert_eq!(line(&built("", ""), 0, None), None);
        assert_eq!(
            line(&list, 1, Some("g")),
            Some(mark(head(), MarkEdge::Below, inside()))
        );
        assert_eq!(
            line(&list, 2, Some("g")),
            Some(mark(MarkTarget::Row(2), MarkEdge::Above, inside()))
        );
        assert_eq!(
            line(&list, 3, Some("g")),
            Some(mark(MarkTarget::Row(2), MarkEdge::Below, inside()))
        );
        assert_eq!(
            line(&collapsed, 3, Some("g")),
            Some(mark(head(), MarkEdge::Whole, inside()))
        );
    }
}
