// Hand-constructed ISO 8583 BitMap vectors. The five vectors below come from
// the standard's bit-layout table (field N -> byte (N-1)/8, bit 7 - (N-1)%8),
// not from a Wireforge encoder run — so they validate the implementation
// against the spec, not against itself.

#![allow(clippy::unwrap_used)]

use wf_bitmap::{Bitmap8583, BitmapError, PRIMARY_LEN, TOTAL_LEN};

fn hex(s: &str) -> Vec<u8> {
    assert!(
        s.len().is_multiple_of(2),
        "hex string must have even length"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn vector_1_empty_bitmap_is_eight_zero_bytes() {
    let bm = Bitmap8583::new();
    assert_eq!(bm.encode(), vec![0u8; PRIMARY_LEN]);
    assert!(!bm.has_secondary());
}

#[test]
fn vector_2_only_field_3_yields_0x20() {
    // field 3 -> byte 0, bit 7 - 2 = 5 -> mask 0x20
    let mut bm = Bitmap8583::new();
    bm.set(3).unwrap();
    assert_eq!(bm.encode(), hex("2000000000000000"));
}

#[test]
fn vector_3_fields_2_3_4_yield_0x70() {
    // bits 6 + 5 + 4 of byte 0 = 0x40 | 0x20 | 0x10 = 0x70
    let mut bm = Bitmap8583::new();
    for f in [2u16, 3, 4] {
        bm.set(f).unwrap();
    }
    assert_eq!(bm.encode(), hex("7000000000000000"));
}

#[test]
fn vector_4_field_70_activates_secondary_and_sets_bit_1() {
    // field 70 -> byte 8, bit 7 - ((70-1) % 8) = 7 - 5 = 2 -> mask 0x04
    // primary bits 2,3,4 + auto bit 1 = 0xF0
    let mut bm = Bitmap8583::new();
    for f in [2u16, 3, 4, 70] {
        bm.set(f).unwrap();
    }
    let encoded = bm.encode();
    assert_eq!(encoded.len(), TOTAL_LEN);
    assert_eq!(encoded, hex("F0000000000000000400000000000000"));
}

#[test]
fn vector_5_all_fields_2_through_128_yield_all_ones() {
    // setting fields 2..=128 leaves bit 1 (field-1 slot) off in the raw
    // bytes; encode() forces it on because has_secondary() == true.
    let mut bm = Bitmap8583::new();
    for f in 2u16..=128 {
        bm.set(f).unwrap();
    }
    assert_eq!(bm.encode(), hex("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"));
}

#[test]
fn roundtrip_decode_then_encode_preserves_bytes() {
    let original = hex("F0000000000000000400000000000000");
    let bm = Bitmap8583::decode(&original).unwrap();
    assert_eq!(bm.encode(), original);
    for f in [2u16, 3, 4, 70] {
        assert!(bm.is_set(f).unwrap(), "field {} should be set", f);
    }
    assert!(!bm.is_set(5).unwrap());
    let set: Vec<u16> = bm.iter_set_fields().collect();
    assert_eq!(set, vec![2, 3, 4, 70]);
}

#[test]
fn decode_eight_byte_only_when_bit_1_clear() {
    let input = hex("7000000000000000");
    let bm = Bitmap8583::decode(&input).unwrap();
    assert!(!bm.has_secondary());
    assert_eq!(bm.encode(), input);
}

#[test]
fn set_unset_round_trip() {
    let mut bm = Bitmap8583::new();
    bm.set(39).unwrap();
    assert!(bm.is_set(39).unwrap());
    bm.unset(39).unwrap();
    assert!(!bm.is_set(39).unwrap());
}

#[test]
fn out_of_range_field_errors() {
    let mut bm = Bitmap8583::new();
    assert_eq!(bm.set(0), Err(BitmapError::FieldOutOfRange(0)));
    assert_eq!(bm.set(129), Err(BitmapError::FieldOutOfRange(129)));
    assert_eq!(bm.is_set(200), Err(BitmapError::FieldOutOfRange(200)));
}

#[test]
fn field_1_secondary_indicator_is_rejected_not_silently_lost() {
    // Field 1 is the secondary-bitmap-present indicator, managed by
    // encode/decode. Accepting set(1) would store a bit those paths then
    // clear, so the value would round-trip to false with no error. It must
    // be rejected at the API instead of silently swallowed.
    let mut bm = Bitmap8583::new();
    assert_eq!(bm.set(1), Err(BitmapError::FieldOutOfRange(1)));
    assert_eq!(bm.is_set(1), Err(BitmapError::FieldOutOfRange(1)));
}

#[test]
fn decode_rejects_too_short_input() {
    assert_eq!(
        Bitmap8583::decode(&[0u8; 4]),
        Err(BitmapError::InsufficientBytes { got: 4, need: 8 })
    );
}

#[test]
fn decode_rejects_secondary_indicator_without_secondary_bytes() {
    let truncated = vec![0x80, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        Bitmap8583::decode(&truncated),
        Err(BitmapError::InsufficientBytes { got: 8, need: 16 })
    );
}
