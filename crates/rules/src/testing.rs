use taco_protocol::{Arrangement, Dragged, DropPlace, Row, TabGroup, TabGroupId, TerminalId};

use crate::arrangement::{block_of, empty_arrangement, is_hidden};
use crate::changes::{add_row, add_row_to, close_row, fork_row, select_row};
use crate::drop::apply_drop;
use crate::tab_groups::{create_tab_group, toggle_tab_group, ungroup_tab_group};

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

struct Dice(u64);

impl Dice {
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let high = self.0 >> 33;
        high as usize % bound.max(1)
    }
}

#[test]
fn random_changes_keep_every_invariant_of_the_arrangement() {
    let mut dice = Dice(7);
    let mut arrangement = empty_arrangement();
    for step in 0..2_000 {
        let row_count = arrangement.rows.len();
        let picked_index = dice.below(row_count);
        let picked = arrangement
            .rows
            .get(picked_index)
            .map(|row| row.terminal.clone());
        let group_count = arrangement.tab_groups.len();
        let group_index = dice.below(group_count);
        let group = arrangement
            .tab_groups
            .get(group_index)
            .map(|record| record.id.clone());
        let fresh = id(&format!("t{step}"));

        let next = match (dice.below(9), picked, group) {
            (0, _, _) => add_row(&arrangement, fresh),
            (1, _, group) => add_row_to(&arrangement, fresh, group),
            (2, Some(row), _) => close_row(&arrangement, &row),
            (3, Some(row), _) => select_row(&arrangement, &row),
            (4, Some(row), _) => fork_row(&arrangement, &row, fresh),
            (5, Some(row), _) => create_tab_group(&arrangement, &row, tab(&format!("g{step}"))),
            (6, _, Some(group)) => toggle_tab_group(&arrangement, &group),
            (7, _, Some(group)) => ungroup_tab_group(&arrangement, &group),
            (8, Some(row), group) => {
                let dragged;
                if dice.below(2) == 0 {
                    dragged = Dragged::Row(row);
                } else {
                    match group.clone() {
                        Some(group) => dragged = Dragged::TabGroup(group),
                        None => dragged = Dragged::Row(row),
                    }
                }
                let place = DropPlace {
                    index: dice.below(row_count + 1),
                    tab_group: group.filter(|_| dice.below(2) == 0),
                };
                apply_drop(&arrangement, &dragged, &place)
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
