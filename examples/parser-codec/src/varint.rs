//! Unsigned LEB128-style varint. Minimal encoder/decoder for the fixture.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarIntError {
    Truncated,
    Overflow,
    NonCanonical,
}

pub fn encode_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

pub fn decode_varint(input: &[u8]) -> Result<(u64, &[u8]), VarIntError> {
    let mut value = 0u64;
    let mut shift = 0;
    for (i, byte) in input.iter().copied().enumerate() {
        if shift >= 64 {
            return Err(VarIntError::Overflow);
        }
        let piece = u64::from(byte & 0x7f);
        if shift == 63 && piece > 1 {
            return Err(VarIntError::Overflow);
        }
        value |= piece << shift;
        if byte & 0x80 == 0 {
            if byte == 0 && i > 0 {
                return Err(VarIntError::NonCanonical);
            }
            return Ok((value, &input[i + 1..]));
        }
        shift += 7;
    }
    Err(VarIntError::Truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &[(u64, &[u8])] = &[
        (0, &[0x00]),
        (1, &[0x01]),
        (127, &[0x7f]),
        (128, &[0x80, 0x01]),
        (300, &[0xac, 0x02]),
    ];

    #[test]
    fn canonical_table() {
        for &(n, bytes) in CASES {
            let mut out = Vec::new();
            encode_varint(&mut out, n);
            assert_eq!(out, bytes, "encode {n}");
            let (got, rest) = decode_varint(bytes).unwrap();
            assert_eq!(got, n);
            assert!(rest.is_empty());
        }
    }

    #[test]
    fn truncated() {
        assert_eq!(decode_varint(&[0x80]), Err(VarIntError::Truncated));
    }

    #[test]
    fn non_canonical_zero() {
        assert_eq!(decode_varint(&[0x80, 0x00]), Err(VarIntError::NonCanonical));
    }
}
