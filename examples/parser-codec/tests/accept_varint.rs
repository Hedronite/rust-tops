use parser_codec::{
    decode_frame, decode_varint, encode_frame, encode_varint, FrameError, VarIntError,
};

#[test]
fn acceptance_varint_max() {
    let mut buf = Vec::new();
    encode_varint(&mut buf, u64::MAX);
    let (got, rest) = decode_varint(&buf).expect("decode max");
    assert_eq!(got, u64::MAX);
    assert!(rest.is_empty());
}

#[test]
fn acceptance_frame_then_rest() {
    let mut buf = Vec::new();
    encode_frame(&mut buf, b"ab").unwrap();
    buf.extend_from_slice(&[0xff]);
    let (payload, rest) = decode_frame(&buf).unwrap();
    assert_eq!(payload, b"ab");
    assert_eq!(rest, &[0xff]);
}

#[test]
fn acceptance_rejects_bad_frames() {
    assert_eq!(decode_frame(&[]), Err(FrameError::Empty));
    assert_eq!(decode_varint(&[]), Err(VarIntError::Truncated));
}

use proptest::prelude::*;

proptest! {
    #[test]
    fn property_varint_roundtrip(n in any::<u64>()) {
        let mut buf = Vec::new();
        encode_varint(&mut buf, n);
        let (got, rest) = decode_varint(&buf).unwrap();
        prop_assert_eq!(got, n);
        prop_assert!(rest.is_empty());
    }

    #[test]
    fn property_frame_roundtrip(payload in prop::collection::vec(any::<u8>(), 1..=64)) {
        let mut buf = Vec::new();
        encode_frame(&mut buf, &payload).unwrap();
        let (got, rest) = decode_frame(&buf).unwrap();
        prop_assert_eq!(got, payload.as_slice());
        prop_assert!(rest.is_empty());
    }
}
