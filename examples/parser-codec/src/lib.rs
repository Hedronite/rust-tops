//! Minimal length-prefixed frame codec used as a Rust-TOPS fixture.
//!
//! Contract:
//! - A frame is `[len:u8][payload]` where `len == payload.len()` and `len > 0`.
//! - `len == 0` is invalid.
//! - Payload is opaque bytes; the codec does not interpret it.
//! - `decode` returns the payload and any remaining unparsed input.

mod varint;

pub use varint::{decode_varint, encode_varint, VarIntError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    Empty,
    Truncated { expected: usize, actual: usize },
}

/// Decode one frame. `Ok((payload, rest))` borrows from `input`.
pub fn decode_frame(input: &[u8]) -> Result<(&[u8], &[u8]), FrameError> {
    let (len, rest) = input.split_first().ok_or(FrameError::Empty)?;
    let len = *len as usize;
    if len == 0 {
        return Err(FrameError::Empty);
    }
    if rest.len() < len {
        return Err(FrameError::Truncated {
            expected: len,
            actual: rest.len(),
        });
    }
    let (payload, rest) = rest.split_at(len);
    Ok((payload, rest))
}

/// Encode `payload` as a single frame into `out`.
///
/// Returns `FrameError::Empty` if `payload` is empty or longer than 255 bytes.
pub fn encode_frame(out: &mut Vec<u8>, payload: &[u8]) -> Result<(), FrameError> {
    if payload.is_empty() || payload.len() > 255 {
        return Err(FrameError::Empty);
    }
    out.push(payload.len() as u8);
    out.extend_from_slice(payload);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_one_byte() {
        let mut buf = Vec::new();
        encode_frame(&mut buf, b"A").unwrap();
        let (payload, rest) = decode_frame(&buf).unwrap();
        assert_eq!(payload, b"A");
        assert!(rest.is_empty());
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(decode_frame(&[]), Err(FrameError::Empty));
        assert_eq!(decode_frame(&[0]), Err(FrameError::Empty));
    }

    #[test]
    fn rejects_truncated() {
        assert_eq!(
            decode_frame(&[3, b'a', b'b']),
            Err(FrameError::Truncated {
                expected: 3,
                actual: 2
            })
        );
    }
}
