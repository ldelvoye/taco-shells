use std::ffi::c_void;

use crate::sys;
use crate::terminal::{Error, Terminal, check};

// The format's layout, from `snapshot.ksy` in the library's source. It is versioned apart from the ABI: check it on a version bump.
/// The format's ten-byte header: the magic `GHOSTSNP` and a version.
const ENVELOPE_BYTES: usize = 10;
/// Each record's framing: a u16 tag, a u32 payload length and a u32 CRC32C.
const RECORD_HEADER_BYTES: usize = 10;
const READY_TAG: u16 = 5;
const FINISH_TAG: u16 = 6;
/// About how large a history part grows before the next one starts. A part never splits a record.
const PART_BYTES: usize = 1024 * 1024;
const MALFORMED: Error = Error {
    code: sys::GhosttyResult_GHOSTTY_INVALID_VALUE,
    call: "reading the snapshot's records",
};

/// A terminal's whole state, cut where a view needs it: the screen first, then history in parts of whole records.
pub struct Snapshot {
    bytes: Vec<u8>,
    screen_end: usize,
    parts: Vec<(usize, usize)>,
}

impl Snapshot {
    pub fn take(terminal: &Terminal) -> Result<Snapshot, Error> {
        let mut bytes: Vec<u8> = Vec::new();
        let userdata = &mut bytes as *mut Vec<u8> as *mut c_void;
        let writer = sys::GhosttyWriter {
            write: Some(append),
            userdata,
        };
        let encoded = unsafe { sys::ghostty_snapshot_encode(terminal.as_raw(), writer) };
        check(encoded, "ghostty_snapshot_encode")?;
        Snapshot::cut(bytes)
    }

    /// Enough to draw at once: the envelope through the READY record.
    pub fn screen(&self) -> &[u8] {
        &self.bytes[..self.screen_end]
    }

    /// Never empty: the last part ends with the FINISH record.
    pub fn history(&self) -> impl Iterator<Item = &[u8]> {
        self.parts
            .iter()
            .map(|&(start, end)| &self.bytes[start..end])
    }

    fn cut(bytes: Vec<u8>) -> Result<Snapshot, Error> {
        let records = record_ends(&bytes)?;
        let Some(&(last_tag, _)) = records.last() else {
            return Err(MALFORMED);
        };
        if last_tag != FINISH_TAG {
            return Err(MALFORMED);
        }

        let mut screen_end: Option<usize> = None;
        let mut parts = Vec::new();
        let mut part_start = 0;
        for (tag, end) in records {
            if screen_end.is_none() {
                if tag == READY_TAG {
                    screen_end = Some(end);
                    part_start = end;
                }
                continue;
            }
            let part_bytes = end - part_start;
            if part_bytes >= PART_BYTES || tag == FINISH_TAG {
                parts.push((part_start, end));
                part_start = end;
            }
        }

        let Some(screen_end) = screen_end else {
            return Err(MALFORMED);
        };
        Ok(Snapshot {
            bytes,
            screen_end,
            parts,
        })
    }
}

/// Each record's tag and the offset just past it.
fn record_ends(bytes: &[u8]) -> Result<Vec<(u16, usize)>, Error> {
    let mut records = Vec::new();
    let mut at = ENVELOPE_BYTES;
    while at < bytes.len() {
        let header_end = at + RECORD_HEADER_BYTES;
        let Some(header) = bytes.get(at..header_end) else {
            return Err(MALFORMED);
        };
        let tag = u16::from_le_bytes([header[0], header[1]]);
        let length_bytes = [header[2], header[3], header[4], header[5]];
        let length = u32::from_le_bytes(length_bytes) as usize;
        let end = header_end + length;
        if end > bytes.len() {
            return Err(MALFORMED);
        }
        records.push((tag, end));
        at = end;
    }
    Ok(records)
}

unsafe extern "C" fn append(userdata: *mut c_void, data: *const u8, len: usize) -> bool {
    let bytes = unsafe { &mut *(userdata as *mut Vec<u8>) };
    let piece = unsafe { std::slice::from_raw_parts(data, len) };
    bytes.extend_from_slice(piece);
    true
}
