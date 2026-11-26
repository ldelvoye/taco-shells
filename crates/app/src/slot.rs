//! The slot that opens in the list where a dragged row or group will land.

use std::time::Duration;

use gpui::{Bounds, Pixels, Point};
use taco_protocol::Arrangement;
use taco_rules::{DropMark, MarkEdge, MarkTarget, block_of};

/// How long the slot takes to open: the mock's 110 ms.
pub const OPEN_TIME: Duration = Duration::from_millis(110);
/// The space between entries in the list and in a tray, which the slot adds to everything after it.
pub const ENTRY_GAP: f32 = 2.;

/// Where the slot sits: before the row at `at`, and before or after the header of a tab group that starts there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotLine {
    pub at: usize,
    /// The slot is the first thing inside that group, under its header.
    pub header_before: bool,
}

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

/// Where the slot goes for a drop mark; none for a mark over a whole thing, as a collapsed group is.
pub fn slot_line_for(arrangement: &Arrangement, mark: &DropMark) -> Option<SlotLine> {
    let before = |at| SlotLine {
        at,
        header_before: false,
    };
    match (&mark.target, mark.edge) {
        (_, MarkEdge::Whole) => None,
        (MarkTarget::Row(index), MarkEdge::Above) => Some(before(*index)),
        (MarkTarget::Row(index), MarkEdge::Below) => Some(before(index + 1)),
        (MarkTarget::Wrapper(tab_group), edge) => {
            let (first, last) = block_of(arrangement, tab_group)?;
            match edge {
                MarkEdge::Above => Some(before(first)),
                _ => Some(before(last + 1)),
            }
        }
        (MarkTarget::Header(tab_group), MarkEdge::Above) => {
            let (first, _) = block_of(arrangement, tab_group)?;
            Some(before(first))
        }
        (MarkTarget::Header(tab_group), MarkEdge::Below) => {
            let (first, _) = block_of(arrangement, tab_group)?;
            let inside = SlotLine {
                at: first,
                header_before: true,
            };
            Some(inside)
        }
    }
}

/// How far down a thing the pointer is, 0 to 1, taking it where it would be with no slot: the slot moving things must not move the drop.
pub fn fraction_at(bounds: Bounds<Pixels>, pointer: Point<Pixels>, shift: f32) -> Option<f32> {
    let height = f32::from(bounds.size.height);
    let top = f32::from(bounds.top()) - shift;
    let left = f32::from(bounds.left());
    let right = f32::from(bounds.right());
    let x = f32::from(pointer.x);
    let y = f32::from(pointer.y);
    let inside = height > 0. && x >= left && x <= right && y >= top && y < top + height;
    if !inside {
        return None;
    }
    let fraction = (y - top) / height;
    Some(fraction.clamp(0., 1.))
}

/// The slot's height `elapsed` after it opened, easing out to the full row.
pub fn opened_height(elapsed: Duration, full: f32) -> f32 {
    let progress = elapsed.as_secs_f32() / OPEN_TIME.as_secs_f32();
    let done = progress.min(1.);
    let left = 1. - done;
    full * (1. - left * left)
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{ENTRY_GAP, Slot, SlotLine, fraction_at};

    const ROW: f32 = 30.;
    const PITCH: f32 = ROW + ENTRY_GAP;

    // Which row and how far down it the pointer is, over rows laid out from the top, the slot's push already applied.
    fn under(pointer_y: f32, rows: usize, slot: Option<Slot>) -> Option<(usize, f32)> {
        let pointer = point(px(10.), px(pointer_y));
        for index in 0..rows {
            let mut shift = 0.;
            if let Some(slot) = slot {
                shift = slot.shift_of_row(index);
            }
            let top = index as f32 * PITCH + shift;
            let bounds = Bounds::new(point(px(0.), px(top)), size(px(200.), px(ROW)));
            if let Some(fraction) = fraction_at(bounds, pointer, shift) {
                return Some((index, fraction));
            }
        }
        None
    }

    #[test]
    fn the_drop_under_the_pointer_is_the_same_with_the_slot_open_as_without_it() {
        let line = SlotLine {
            at: 2,
            header_before: false,
        };
        let opening = Slot { line, height: 12. };
        let open = Slot { line, height: ROW };
        let rows = 5;
        let mut y = 0.;
        while y < rows as f32 * PITCH {
            let without = under(y, rows, None);
            assert_eq!(under(y, rows, Some(opening)), without, "opening, at {y}");
            assert_eq!(under(y, rows, Some(open)), without, "open, at {y}");
            y += 1.;
        }
    }
}
