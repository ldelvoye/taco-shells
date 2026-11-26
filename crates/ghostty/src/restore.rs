use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr;

use crate::sys;
use crate::terminal::{Error, check};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The last part has not arrived yet.
    Waiting,
    /// One history page went into the terminal (or the pixels of one Kitty graphics image).
    Applied,
    Finished,
}

/// The snapshot bytes not yet read by the decoder, a part at a time, so each part is freed once it has been read.
struct Source {
    parts: VecDeque<Vec<u8>>,
    /// How far into the front part the decoder has read.
    offset: usize,
}

/// The history of a restored terminal, applied a page per `step` once every part is in: the decoder reads past the page it applies, and fails if it runs dry.
pub(crate) struct Restore {
    decoder: sys::GhosttySnapshotDecoder,
    /// Owned, from `Box::into_raw`, and shared with the decoder's reader, which may read it until the decoder is freed.
    source: *mut Source,
    complete: bool,
}

// Like the terminal it belongs to: no thread affinity, and `&mut` keeps it to one thread at a time.
unsafe impl Send for Restore {}

impl Restore {
    /// The decoder over the screen part, and the terminal it made from it. The terminal must outlive the decoder.
    pub(crate) fn begin(screen: &[u8]) -> Result<(Restore, sys::GhosttyTerminal), Error> {
        let first = vec![screen.to_vec()];
        let boxed = Box::new(Source {
            parts: VecDeque::from(first),
            offset: 0,
        });
        let source = Box::into_raw(boxed);
        let reader = sys::GhosttyReader {
            read: Some(read_source),
            userdata: source as *mut c_void,
        };

        let mut decoder: sys::GhosttySnapshotDecoder = ptr::null_mut();
        let created =
            unsafe { sys::ghostty_snapshot_decoder_new(ptr::null(), &mut decoder, reader) };
        let restore = Restore {
            decoder,
            source,
            complete: false,
        };
        check(created, "ghostty_snapshot_decoder_new")?;

        let mut raw: sys::GhosttyTerminal = ptr::null_mut();
        let ready = unsafe { sys::ghostty_snapshot_decoder_ready(decoder, &mut raw) };
        check(ready, "ghostty_snapshot_decoder_ready")?;
        Ok((restore, raw))
    }

    pub(crate) fn push(&mut self, part: &[u8], last: bool) {
        if self.complete {
            return;
        }
        let source = unsafe { &mut *self.source };
        source.parts.push_back(part.to_vec());
        self.complete = last;
    }

    pub(crate) fn step(&mut self) -> Result<Step, Error> {
        if !self.complete {
            return Ok(Step::Waiting);
        }
        let result = unsafe { sys::ghostty_snapshot_decoder_next(self.decoder) };
        if result == sys::GhosttyResult_GHOSTTY_NO_VALUE {
            return Ok(Step::Finished);
        }
        check(result, "ghostty_snapshot_decoder_next")?;
        Ok(Step::Applied)
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        if !self.decoder.is_null() {
            unsafe { sys::ghostty_snapshot_decoder_free(self.decoder) };
        }
        let source = unsafe { Box::from_raw(self.source) };
        drop(source);
    }
}

unsafe extern "C" fn read_source(
    userdata: *mut c_void,
    buffer: *mut u8,
    capacity: usize,
    out_read: *mut usize,
) -> bool {
    let source = unsafe { &mut *(userdata as *mut Source) };
    let mut copied = 0;
    while copied < capacity {
        let Some(front) = source.parts.front() else {
            break;
        };
        let unread = &front[source.offset..];
        let count = unread.len().min(capacity - copied);
        let target = unsafe { buffer.add(copied) };
        unsafe { ptr::copy_nonoverlapping(unread.as_ptr(), target, count) };
        copied += count;
        source.offset += count;
        if source.offset == front.len() {
            source.parts.pop_front();
            source.offset = 0;
        }
    }
    unsafe { *out_read = copied };
    true
}
