//! The slot that opens in the list where a dragged row or group will land.

use std::time::Duration;

use taco_rules::SlotLine;

/// How long the slot takes to open: the mock's 110 ms.
pub const OPEN_TIME: Duration = Duration::from_millis(110);
/// The space between entries in the list and in a tray, which the slot adds to everything after it.
pub const ENTRY_GAP: f32 = 2.;

/// The slot as drawn this frame: where it is and how tall it has opened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub line: SlotLine,
    pub height: f32,
}

impl Slot {
    // Everything after the slot is pushed down by it and the gap it brings.
    fn push(self) -> f32 {
        self.height + ENTRY_GAP
    }

    /// How far the row at `index` is drawn below where it would be with no slot.
    pub fn shift_of_row(self, index: usize) -> f32 {
        if index >= self.line.at {
            return self.push();
        }
        0.
    }

    /// How far the header of the tab group whose first row is `first` is drawn below where it would be with no slot.
    pub fn shift_of_header(self, first: usize) -> f32 {
        let after = first > self.line.at || (first == self.line.at && !self.line.header_before);
        if after {
            return self.push();
        }
        0.
    }

    /// The space after the last entry is always below the slot.
    pub fn shift_of_end(self) -> f32 {
        self.push()
    }
}

/// The slot's height `elapsed` after it opened, easing out to the full row.
pub fn opened_height(elapsed: Duration, full: f32) -> f32 {
    let progress = elapsed.as_secs_f32() / OPEN_TIME.as_secs_f32();
    let done = progress.min(1.);
    let left = 1. - done;
    full * (1. - left * left)
}
