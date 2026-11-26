use taco_protocol::{Arrangement, Dragged, DropPlace, Row, TabGroup, TabGroupId, TerminalId};

use crate::arrangement::{block_of, empty_arrangement, is_hidden};
use crate::changes::{add_row, add_row_to, close_row, fork_row, select_row};
use crate::drop::{DropPoint, apply_drop, drop_for, drop_mark_for};
use crate::tab_groups::{create_tab_group, toggle_tab_group, ungroup_tab_group};

/// A seeded generator, so a failing run of the random test repeats exactly.
struct Dice(u64);

impl Dice {
    fn below(&mut self, bound: usize) -> usize {
        let multiplied = self.0.wrapping_mul(6364136223846793005);
        self.0 = multiplied.wrapping_add(1442695040888963407);
        let high = self.0 >> 33;
        high as usize % bound.max(1)
    }
}

fn dragged_for(drags_row: bool, row: TerminalId, group: Option<TabGroupId>) -> Dragged {
    match (drags_row, group) {
        (false, Some(group)) => Dragged::TabGroup(group),
        _ => Dragged::Row(row),
    }
}

pub fn id(name: &str) -> TerminalId {
    TerminalId(name.to_string())
}

pub fn tab(name: &str) -> TabGroupId {
    TabGroupId(name.to_string())
}

/// An arrangement written as rows in order: "a g:b g:c d" puts b and c in tab group g. `selected` names a row.
pub fn built(spec: &str, selected: &str) -> Arrangement {
    let mut arrangement = empty_arrangement();
    for token in spec.split_whitespace() {
        let (tab_group, name) = match token.split_once(':') {
            Some((group, name)) => (Some(tab(group)), name),
            None => (None, token),
        };
        if let Some(group) = &tab_group
            && !arrangement
                .tab_groups
                .iter()
                .any(|record| record.id == *group)
        {
            let record = TabGroup {
                id: group.clone(),
                name: group.0.clone(),
                collapsed: false,
            };
            arrangement.tab_groups.push(record);
        }
        let row = Row {
            terminal: id(name),
            title: name.to_string(),
            tab_group,
        };
        arrangement.rows.push(row);
    }

    let selected_name = match selected.split_once(':') {
        Some((_, name)) => name,
        None => selected,
    };
    let mut rows = arrangement.rows.iter();
    if let Some(index) = rows.position(|row| row.terminal == id(selected_name)) {
        arrangement.selected = index;
    }
    arrangement
}

/// The arrangement written back: a collapsed group's rows as "g-:b", the selected row in brackets.
pub fn shown(arrangement: &Arrangement) -> String {
    let mut tokens = Vec::new();
    for (index, row) in arrangement.rows.iter().enumerate() {
        let mut token = String::new();
        if let Some(group) = &row.tab_group {
            token.push_str(&group.0);
            if is_hidden(arrangement, index) {
                token.push('-');
            }
            token.push(':');
        }
        token.push_str(&row.terminal.0);
        if index == arrangement.selected {
            token = format!("[{token}]");
        }
        tokens.push(token);
    }
    tokens.join(" ")
}

fn invariant_breaks(arrangement: &Arrangement) -> Option<&'static str> {
    let rows = &arrangement.rows;
    if !rows.is_empty() && arrangement.selected >= rows.len() {
        return Some("the selection is past the end");
    }
    for record in &arrangement.tab_groups {
        let Some((first, last)) = block_of(arrangement, &record.id) else {
            return Some("a tab group has no rows");
        };
        let block = &rows[first..=last];
        if block
            .iter()
            .any(|row| row.tab_group.as_ref() != Some(&record.id))
        {
            return Some("a tab group's rows are apart");
        }
    }
    for row in rows {
        let Some(group) = &row.tab_group else {
            continue;
        };
        if !arrangement
            .tab_groups
            .iter()
            .any(|record| record.id == *group)
        {
            return Some("a row names a tab group that is gone");
        }
    }
    let all_hidden = (0..rows.len()).all(|index| is_hidden(arrangement, index));
    if is_hidden(arrangement, arrangement.selected) && !all_hidden {
        return Some("the selection is hidden while a row shows");
    }
    None
}

#[test]
fn random_changes_keep_every_invariant_of_the_arrangement() {
    let mut dice = Dice(7);
    let mut arrangement = empty_arrangement();
    for step in 0..2_000 {
        let row_count = arrangement.rows.len();
        let picked_index = dice.below(row_count + 1);
        let picked_row = arrangement.rows.get(picked_index);
        // one pick in a few is a terminal that is not open
        let picked = match picked_row {
            Some(row) => row.terminal.clone(),
            None => id("gone"),
        };
        let group_index = dice.below(arrangement.tab_groups.len());
        let group_record = arrangement.tab_groups.get(group_index);
        let group = group_record.map(|record| record.id.clone());
        let fresh = id(&format!("t{step}"));
        let drags_row = dice.below(2) == 0;
        let names_group = dice.below(2) == 0;
        let gap = dice.below(row_count + 2);
        let fraction = dice.below(101) as f32 / 100.0;

        let next = match (dice.below(10), group) {
            (0, _) => add_row(&arrangement, fresh),
            (1, group) => add_row_to(&arrangement, fresh, group),
            (2, _) => close_row(&arrangement, &picked),
            (3, _) => select_row(&arrangement, &picked),
            (4, _) => fork_row(&arrangement, &picked, fresh),
            (5, _) => create_tab_group(&arrangement, &picked, tab(&format!("g{step}"))),
            (6, Some(group)) => toggle_tab_group(&arrangement, &group),
            (7, Some(group)) => ungroup_tab_group(&arrangement, &group),
            (8, group) => {
                let dragged = dragged_for(drags_row, picked, group.clone());
                let mut tab_group = None;
                if names_group {
                    tab_group = group;
                }
                let place = DropPlace {
                    index: gap,
                    tab_group,
                };
                drop_mark_for(&arrangement, &place);
                apply_drop(&arrangement, &dragged, &place)
            }
            (9, group) => {
                let dragged = dragged_for(drags_row, picked, group);
                let point = DropPoint::Row {
                    index: gap,
                    fraction,
                };
                let offered = drop_for(&arrangement, &dragged, &point);
                let Some(place) = offered else { continue };
                assert!(drop_mark_for(&arrangement, &place).is_some());
                let applied = apply_drop(&arrangement, &dragged, &place);
                assert!(applied.is_some(), "an offered drop applies");
                applied
            }
            _ => None,
        };
        if let Some(next) = next {
            arrangement = next;
        }
        let broken = invariant_breaks(&arrangement);
        assert_eq!(broken, None, "after step {step}: {}", shown(&arrangement));
    }
}
