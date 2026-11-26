//! Pure functions over an arrangement: questions about it, and the changes the window and the workspace make to it.
//!
//! Every change takes the arrangement as it is and gives back the one it becomes, or none
//! when the change would do nothing or is refused.

mod arrangement;
mod changes;
mod drop;
mod fork_name;
mod panels;
mod rows;
mod tab_groups;

#[cfg(test)]
mod testing;

pub use arrangement::{block_of, empty_arrangement, is_hidden, locate_row, selected_terminal};
pub use changes::{add_row, add_row_to, close_row, focus_target, fork_row, rename_row, select_row};
pub use drop::{DropMark, DropPoint, MarkEdge, MarkTarget, apply_drop, drop_for, drop_mark_for};
pub use fork_name::fork_name_for;
pub use panels::{close_panel, default_panels, move_panel, open_panel, resize_panel};
pub use rows::{can_fork, summary_state, waiting_count};
pub use tab_groups::{
    NEW_TAB_GROUP_NAME, create_tab_group, rename_tab_group, toggle_tab_group, ungroup_tab_group,
};
