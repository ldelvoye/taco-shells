//! The framing between the app and the runtime: each message is a little-endian `u32` length, then the message in postcard.

use std::io::{self, ErrorKind, Read, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The largest frame either end accepts. Anything larger is a bug, and the reader refuses it before allocating.
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

const LENGTH_BYTES: usize = 4;

/// One message as one frame, sent in a single write.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let placeholder = vec![0u8; LENGTH_BYTES];
    let mut frame = postcard::to_extend(message, placeholder).map_err(invalid_data)?;

    let length = frame.len() - LENGTH_BYTES;
    if length > MAX_FRAME {
        let reason = format!("a {length}-byte message is over the {MAX_FRAME}-byte frame limit");
        return Err(io::Error::new(ErrorKind::InvalidInput, reason));
    }

    let header = (length as u32).to_le_bytes();
    frame[..LENGTH_BYTES].copy_from_slice(&header);
    writer.write_all(&frame)
}

/// The next frame's message: `None` when the stream ends cleanly between frames, an error when it ends inside one.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> io::Result<Option<T>> {
    let Some(header) = read_header(reader)? else {
        return Ok(None);
    };

    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_FRAME {
        let reason = format!("a frame announced {length} bytes, over the {MAX_FRAME}-byte limit");
        return Err(io::Error::new(ErrorKind::InvalidData, reason));
    }

    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    let message = postcard::from_bytes(&payload).map_err(invalid_data)?;
    Ok(Some(message))
}

fn read_header(reader: &mut impl Read) -> io::Result<Option<[u8; LENGTH_BYTES]>> {
    let mut header = [0u8; LENGTH_BYTES];
    let mut filled = 0;
    while filled < LENGTH_BYTES {
        let read = match reader.read(&mut header[filled..]) {
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 && filled == 0 {
            return Ok(None);
        }
        if read == 0 {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        filled += read;
    }
    Ok(Some(header))
}

fn invalid_data(error: postcard::Error) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Message {
        Output { id: String, data: Vec<u8> },
        Stop,
    }

    #[test]
    fn frames_keep_their_boundaries_and_a_stream_ends_cleanly_only_between_them() {
        let messages = vec![
            Message::Output {
                id: "a".into(),
                data: b"\x1b[31mred\r\n".to_vec(),
            },
            Message::Stop,
            Message::Output {
                id: "b".into(),
                data: vec![0xff; 70_000],
            },
        ];
        let mut stream = Vec::new();
        for message in &messages {
            write_frame(&mut stream, message).unwrap();
        }

        let mut reader = stream.as_slice();
        let mut read_back = Vec::new();
        while let Some(message) = read_frame::<Message>(&mut reader).unwrap() {
            read_back.push(message);
        }
        assert_eq!(read_back, messages);

        let cut_inside_header = &stream[..2];
        let error = read_frame::<Message>(&mut &cut_inside_header[..]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);

        let cut_inside_payload = &stream[..LENGTH_BYTES + 3];
        let error = read_frame::<Message>(&mut &cut_inside_payload[..]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
    }

    #[test]
    fn an_oversized_length_is_refused_before_anything_is_allocated() {
        let announced = (MAX_FRAME as u32 + 1).to_le_bytes();
        let error = read_frame::<Message>(&mut &announced[..]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::InvalidData);
    }
}
