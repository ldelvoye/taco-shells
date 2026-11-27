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

/// The move that takes longest, so no frame is wanted once it has run.
pub const LONGEST_SETTING_MOVE: Duration = SEGMENT_TIME;

/// A setting the last config changed, with the value it moved from.
#[derive(Clone, Debug)]
pub struct Moved {
    pub from: SettingChange,
    pub since: Instant,
    /// Where a switch was drawn when this move began part way through another; it starts there, not at the far end.
    pub start_amount: Option<f32>,
    /// Where a segment's card was drawn when this move began part way through another.
    pub start_bounds: Option<Bounds<Pixels>>,
}

/// What one drawing of the page needs to place things that are moving.
pub struct Motion<'a> {
    pub moved: &'a HashMap<&'static str, Moved>,
    pub now: Instant,
    pub reduce_motion: bool,
    pub bounds: &'a DrawnBounds,
}

impl Motion<'_> {
    /// A setting's move and how far it has come, while it is still moving.
    pub fn moving(&self, key: &str, length: Duration) -> Option<(&Moved, f32)> {
        let moved = self.moved.get(key)?;
        let done = progress(moved.since, self.now, length, self.reduce_motion);
        if done >= 1. {
            return None;
        }
        Some((moved, done))
    }
}

/// How far on a switch is drawn, 0 off to 1 on, eased from where it started toward its value.
pub fn switch_amount(on: bool, start: Option<f32>, done: f32) -> f32 {
    let mut target = 0.;
    if on {
        target = 1.;
    }
    let mut from = 1. - target;
    if let Some(started) = start {
        from = started;
    }
    lerp(from, target, done)
}

/// A switch setting's value; none for a setting that is not a switch.
pub fn switch_value(setting: &SettingChange) -> Option<bool> {
    match setting {
        SettingChange::CursorBlink(on)
        | SettingChange::RestoreOnLaunch(on)
        | SettingChange::AgentActivity(on)
        | SettingChange::AskForActivityNotes(on) => Some(*on),
        _ => None,
    }
}

/// The name a segmented control's card is recorded under, by its setting's name.
pub fn card_name(key: &str) -> SharedString {
    SharedString::from(format!("card:{key}"))
}

/// A canvas that records its parent's bounds under `name`.
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

/// The settings whose value differs between two configs, each with its old value; a control animates only its own.
pub fn moved_settings(before: &Settings, after: &Settings) -> Vec<SettingChange> {
    let olds = SettingChange::all(before);
    let news = SettingChange::all(after);
    let mut moved = Vec::new();
    for (old, new) in olds.into_iter().zip(news) {
        if old != new {
            moved.push(old);
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use taco_settings::{CursorStyle, ShellRowName, ThemeChoice};

    use super::*;

    #[test]
    fn a_move_eases_out_to_its_end_and_reduce_motion_skips_it() {
        let since = Instant::now();
        let length = Duration::from_millis(100);
        let at = |millis: u64| since + Duration::from_millis(millis);
        assert_eq!(progress(since, at(100), length, false), 1.);
        assert_eq!(progress(since, at(0), length, true), 1.);
    }

    #[test]
    fn every_setting_that_changed_moves_from_its_old_value_and_nothing_else_does() {
        let before = Settings::default();
        let mut after = before.clone();
        after.theme = ThemeChoice::Dark;
        after.cursor_style = CursorStyle::Bar;
        after.shell_row_name = ShellRowName::Path;
        after.cursor_blink = !before.cursor_blink;
        after.restore_on_launch = !before.restore_on_launch;
        after.agent_activity = !before.agent_activity;
        after.ask_for_activity_notes = !before.ask_for_activity_notes;
        after.font_size = before.font_size + 1.;
        let moved = moved_settings(&before, &after);
        let expected = vec![
            SettingChange::Theme(before.theme),
            SettingChange::FontSize(before.font_size),
            SettingChange::CursorBlink(before.cursor_blink),
            SettingChange::CursorStyle(before.cursor_style),
            SettingChange::RestoreOnLaunch(before.restore_on_launch),
            SettingChange::AgentActivity(before.agent_activity),
            SettingChange::AskForActivityNotes(before.ask_for_activity_notes),
            SettingChange::ShellRowName(before.shell_row_name),
        ];
        assert_eq!(moved, expected);
        let unchanged = moved_settings(&before, &before);
        assert!(unchanged.is_empty());
    }

    #[test]
    fn a_switch_turned_back_mid_move_carries_on_from_where_it_was_drawn() {
        let turning_on = switch_amount(true, None, 0.3);
        // Turned off again at that point, it starts where it was rather than at fully on.
        let turned_back = switch_amount(false, Some(turning_on), 0.);
        assert_eq!(turned_back, turning_on);
        assert_eq!(switch_amount(false, Some(turning_on), 1.), 0.);
    }
}
