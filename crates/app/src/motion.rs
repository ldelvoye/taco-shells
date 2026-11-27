//! How the pages move: a switch's knob, a segment's chosen card and a page coming or going, each over a few frames on
//! the window's clock and not at all under Reduce Motion.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{Bounds, Canvas, Pixels, SharedString, Styled, canvas};
use taco_settings::{SettingChange, Settings};

pub const SWITCH_TIME: Duration = Duration::from_millis(120);
pub const SEGMENT_TIME: Duration = Duration::from_millis(160);
pub const PAGE_TIME: Duration = Duration::from_millis(150);

/// Where each segment and segment track was last drawn, by name, so a chosen card can slide between two of them.
pub type DrawnBounds = Rc<RefCell<HashMap<SharedString, Bounds<Pixels>>>>;

/// A setting the last config changed, with the value it moved from.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub from: SettingChange,
    pub since: Instant,
}

/// What one drawing of the page needs to place things that are moving.
pub struct Motion<'a> {
    pub moved: &'a HashMap<&'static str, Moved>,
    pub now: Instant,
    pub reduce_motion: bool,
    pub bounds: &'a DrawnBounds,
}

impl Motion<'_> {
    /// The value a setting is moving from and how far it has come, while it is still moving.
    pub fn moving(&self, key: &str, length: Duration) -> Option<(&SettingChange, f32)> {
        let moved = self.moved.get(key)?;
        let done = progress(moved.since, self.now, length, self.reduce_motion);
        if done >= 1. {
            return None;
        }
        Some((&moved.from, done))
    }
}

/// Records where its parent is drawn under `name`, for the next frame's slide.
pub fn recorder(bounds: &DrawnBounds, name: SharedString) -> Canvas<()> {
    let table = Rc::clone(bounds);
    let record = move |drawn: Bounds<Pixels>, _: &mut gpui::Window, _: &mut gpui::App| {
        table.borrow_mut().insert(name.clone(), drawn);
    };
    canvas(record, |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// How far a move started at `since` has come by `now`, eased out: 0 at the start, 1 once done or under Reduce Motion.
pub fn progress(since: Instant, now: Instant, length: Duration, reduce_motion: bool) -> f32 {
    if reduce_motion {
        return 1.;
    }
    let elapsed = now.saturating_duration_since(since);
    let linear = elapsed.as_secs_f32() / length.as_secs_f32();
    let done = linear.min(1.);
    let left = 1. - done;
    1. - left * left * left
}

pub fn lerp(from: f32, to: f32, amount: f32) -> f32 {
    from + (to - from) * amount
}

/// The switches and segments whose value differs between two configs, each with its old value.
pub fn moved_settings(before: &Settings, after: &Settings) -> Vec<SettingChange> {
    let pairs = [
        (
            SettingChange::Theme(before.theme),
            SettingChange::Theme(after.theme),
        ),
        (
            SettingChange::CursorStyle(before.cursor_style),
            SettingChange::CursorStyle(after.cursor_style),
        ),
        (
            SettingChange::ShellRowName(before.shell_row_name),
            SettingChange::ShellRowName(after.shell_row_name),
        ),
        (
            SettingChange::CursorBlink(before.cursor_blink),
            SettingChange::CursorBlink(after.cursor_blink),
        ),
        (
            SettingChange::RestoreOnLaunch(before.restore_on_launch),
            SettingChange::RestoreOnLaunch(after.restore_on_launch),
        ),
        (
            SettingChange::ManageAgentHooks(before.manage_agent_hooks),
            SettingChange::ManageAgentHooks(after.manage_agent_hooks),
        ),
    ];
    let mut moved = Vec::new();
    for (old, new) in pairs {
        if old != new {
            moved.push(old);
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use taco_settings::{CursorStyle, ThemeChoice};

    use super::*;

    #[test]
    fn a_move_eases_out_to_its_end_and_reduce_motion_skips_it() {
        let since = Instant::now();
        let length = Duration::from_millis(100);
        let at = |millis: u64| since + Duration::from_millis(millis);
        assert_eq!(progress(since, at(0), length, false), 0.);
        let halfway = progress(since, at(50), length, false);
        assert!(halfway > 0.5 && halfway < 1., "{halfway}");
        assert_eq!(progress(since, at(100), length, false), 1.);
        assert_eq!(progress(since, at(400), length, false), 1.);
        assert_eq!(progress(since, at(0), length, true), 1.);
    }

    #[test]
    fn only_the_switches_and_segments_that_changed_move_and_from_their_old_value() {
        let before = Settings::default();
        let mut after = before.clone();
        after.theme = ThemeChoice::Dark;
        after.cursor_blink = !before.cursor_blink;
        after.font_size = before.font_size + 1.;
        let moved = moved_settings(&before, &after);
        let expected = vec![
            SettingChange::Theme(before.theme),
            SettingChange::CursorBlink(before.cursor_blink),
        ];
        assert_eq!(moved, expected);
        let unchanged = moved_settings(&before, &before);
        assert!(unchanged.is_empty());
        let _ = CursorStyle::Block;
    }
}
