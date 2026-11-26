use serde::{Deserialize, Serialize};

use crate::ids::{TabGroupId, TerminalId};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub terminal: TerminalId,
    pub title: String,
    pub tab_group: Option<TabGroupId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabGroup {
    pub id: TabGroupId,
    pub name: String,
    pub collapsed: bool,
}

/// The rows of one tab group sit next to each other in `rows`, every tab group has a row,
/// and the selected row is not in a collapsed tab group unless every row is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Arrangement {
    pub rows: Vec<Row>,
    pub selected: usize,
    pub tab_groups: Vec<TabGroup>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Dragged {
    Row(TerminalId),
    TabGroup(TabGroupId),
}

/// Where a drag lands. `index` counts gaps in the list as it is before the move, and
/// `tab_group` is the tab group the dropped thing ends up in, or none.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DropPlace {
    pub index: usize,
    pub tab_group: Option<TabGroupId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusMove {
    NextRow,
    PreviousRow,
}
