//! Dialect-coverage tests: FullAscii parse/build/round-trip + auto-sniff.
//!
//! Vectors are hand-constructed against the spec convention as observed in
//! the public OSS corpus (jpos `ISO87A`/`ISO93A` packagers, moov-io Go test
//! literals). Per CLAUDE.md §4.1 ③ "measurement and subject must not share
//! a source", these vectors are NOT regenerated from `build_with` and then
//! fed back to `parse_with` in the same test — the wire bytes are written
//! out longhand so a parser regression that re-encodes wrong is detected
//! against the spec, not against itself.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use wf_codec::iso8583::{build_with, parse_any, parse_with, Dialect, Iso8583Message, ParseError};

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

fn msg(mti: &[u8; 4], entries: &[(u8, &[u8])]) -> Iso8583Message {
    let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for (n, data) in entries {
        fields.insert(*n, data.to_vec());
    }
    Iso8583Message { mti: *mti, fields }
}

// ---------------------------------------------------------------------------
// FullAscii spec vectors
// ---------------------------------------------------------------------------

/// F1. FullAscii minimal: MTI 0800, empty primary bitmap (16 ASCII zero chars).
#[test]
fn full_ascii_parse_mti_only_empty_bitmap() {
    let wire = b"08000000000000000000";
    assert_eq!(wire.len(), 4 + 16);

    let got = parse_with(wire, Dialect::FullAscii).expect("FullAscii minimal message must parse");
    assert_eq!(&got.mti, b"0800");
    assert!(got.fields.is_empty(), "got fields = {:?}", got.fields);
}

/// F2. FullAscii auth skeleton: same field set as iso8583_message::A2 but with
/// the bitmap rendered as ASCII hex. Bitmap byte 0 = 0x32 → "32"; rest zeros.
#[test]
fn full_ascii_parse_mti_0200_fields_3_4_7() {
    let wire = cat(&[
        b"0200",
        b"3200000000000000",
        b"000000",
        b"000000010000",
        b"1130120000",
    ]);

    let got = parse_with(&wire, Dialect::FullAscii).expect("FullAscii skeleton must parse");
    assert_eq!(&got.mti, b"0200");
    assert_eq!(got.fields.len(), 3);
    assert_eq!(got.fields.get(&3).unwrap().as_slice(), b"000000");
    assert_eq!(got.fields.get(&4).unwrap().as_slice(), b"000000010000");
    assert_eq!(got.fields.get(&7).unwrap().as_slice(), b"1130120000");
}

/// F3. FullAscii LLVAR PAN — same as iso8583_message::A3 but ASCII-hex bitmap.
/// Primary bitmap byte 0 = 0x40 (f2), rest zeros → "4000000000000000".
#[test]
fn full_ascii_parse_llvar_field_2_pan() {
    let wire = cat(&[b"0200", b"4000000000000000", b"16", b"4111111111111111"]);

    let got = parse_with(&wire, Dialect::FullAscii).expect("FullAscii LLVAR PAN must parse");
    assert_eq!(got.fields.len(), 1);
    assert_eq!(got.fields.get(&2).unwrap().as_slice(), b"4111111111111111");
}

/// F4. FullAscii secondary-bitmap message. Primary byte 0 = 0x80 ("80"),
/// secondary byte 0 = 0x04 ("04"); rest zeros. Full 32-char bitmap.
#[test]
fn full_ascii_parse_secondary_bitmap_field_70() {
    let wire = cat(&[b"0200", b"80000000000000000400000000000000", b"301"]);

    let got = parse_with(&wire, Dialect::FullAscii).expect("secondary-bitmap must parse");
    assert_eq!(got.fields.get(&70).unwrap().as_slice(), b"301");
    assert!(!got.fields.contains_key(&1));
}

/// F5. Lowercase hex acceptance — the parser must treat `'a'..='f'`
/// identically to `'A'..='F'`. Builder always emits uppercase; the parser
/// has to accept either because real-world senders mix the two.
///
/// Bitmap construction: primary byte 0 = `0xA0` → bit 1 (secondary present)
/// AND bit 3 (field 3, processing code). Secondary = all zeros. Then a
/// 6-byte processing-code payload. The hex letter exercises case handling.
#[test]
fn full_ascii_parse_accepts_lowercase_hex_bitmap() {
    let wire_upper = cat(&[b"0200", b"A000000000000000", b"0000000000000000", b"123456"]);
    let wire_lower = cat(&[b"0200", b"a000000000000000", b"0000000000000000", b"123456"]);
    let up = parse_with(&wire_upper, Dialect::FullAscii).expect("uppercase parses");
    let lo = parse_with(&wire_lower, Dialect::FullAscii).expect("lowercase parses");
    assert_eq!(up, lo, "case must not affect parsed message");
    assert_eq!(up.fields.get(&3).unwrap().as_slice(), b"123456");
}

// ---------------------------------------------------------------------------
// FullAscii build + round-trip
// ---------------------------------------------------------------------------

/// F6. build_with(FullAscii) is the byte-exact inverse of F2.
#[test]
fn full_ascii_build_matches_f2_wire() {
    let m = msg(
        b"0200",
        &[(3, b"000000"), (4, b"000000010000"), (7, b"1130120000")],
    );
    let expected = cat(&[
        b"0200",
        b"3200000000000000",
        b"000000",
        b"000000010000",
        b"1130120000",
    ]);
    let got = build_with(&m, Dialect::FullAscii).expect("build_with FullAscii must succeed");
    assert_eq!(got, expected);
}

/// F7. Round-trip F4 (secondary bitmap) end-to-end.
#[test]
fn full_ascii_roundtrip_secondary_bitmap() {
    let wire = cat(&[b"0200", b"80000000000000000400000000000000", b"301"]);
    let parsed = parse_with(&wire, Dialect::FullAscii).unwrap();
    let rebuilt = build_with(&parsed, Dialect::FullAscii).unwrap();
    assert_eq!(rebuilt, wire);
}

// ---------------------------------------------------------------------------
// Auto-sniff (parse_any) coverage
// ---------------------------------------------------------------------------

/// S1. HybridAscii input must be detected as HybridAscii (back-compat: every
/// historical caller relied on this implicitly).
#[test]
fn sniff_hybrid_ascii_input() {
    let wire = cat(&[
        b"0200",
        b"\x40\x00\x00\x00\x00\x00\x00\x00",
        b"16",
        b"4111111111111111",
    ]);
    let (msg, dialect) = parse_any(&wire).expect("must parse");
    assert_eq!(dialect, Dialect::HybridAscii);
    assert_eq!(msg.fields.get(&2).unwrap().as_slice(), b"4111111111111111");
}

/// S2. FullAscii input must be detected as FullAscii (the entire point of
/// dialect support — moov-io / jpos-87A samples now flow through `parse`).
#[test]
fn sniff_full_ascii_input() {
    let wire = cat(&[b"0200", b"4000000000000000", b"16", b"4111111111111111"]);
    let (msg, dialect) = parse_any(&wire).expect("must parse");
    assert_eq!(dialect, Dialect::FullAscii);
    assert_eq!(msg.fields.get(&2).unwrap().as_slice(), b"4111111111111111");
}

/// S3. Sniff result is the dialect that build_with reproduces — so the
/// "parse_any → redact → build_with(same dialect)" loop in `sample-sanitize`
/// is byte-faithful regardless of which dialect arrived.
#[test]
fn sniff_then_build_roundtrips_each_dialect() {
    for &dialect in Dialect::ALL {
        let m = msg(
            b"0200",
            &[(3, b"000000"), (4, b"000000010000"), (7, b"1130120000")],
        );
        let wire = build_with(&m, dialect).expect("build_with must succeed");
        let (parsed, detected) = parse_any(&wire).expect("parse_any must succeed");
        assert_eq!(
            detected, dialect,
            "build_with({dialect:?}) produced bytes that sniff thinks are {detected:?}"
        );
        let rebuilt = build_with(&parsed, detected).expect("build_with reverse must succeed");
        assert_eq!(rebuilt, wire, "round-trip mismatch for {dialect:?}");
    }
}

/// S4. When neither dialect matches, the error returned is the HybridAscii
/// error (back-compat contract documented on `parse_any`).
#[test]
fn sniff_no_match_returns_first_dialect_error() {
    let err = parse_any(b"XX").expect_err("2-byte garbage must fail");
    // First-dialect = HybridAscii. Its error on a 2-byte input is
    // InsufficientBytes — the MTI check runs before any dialect-specific work.
    assert!(
        matches!(
            err,
            ParseError::InsufficientBytes { .. } | ParseError::InvalidMti(_)
        ),
        "expected HybridAscii's MTI-stage error, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// FullBinary spec vectors
//
// Vectors are hand-constructed against the jpos `ISO87BPackager` convention
// (2-byte BCD MTI, raw-binary bitmap, BCD length prefixes, BCD-packed Numeric
// payloads, raw bytes for non-numeric data types). Per CLAUDE.md §4.1 ③ none
// of these are regenerated from `build_with(FullBinary)` then fed back to
// `parse_with(FullBinary)` — the wire bytes are written longhand so a parser
// regression that re-encodes wrong is detected against the spec convention,
// not against itself.
// ---------------------------------------------------------------------------

/// B1. FullBinary minimal: MTI 0800 (= 0x08 0x00) + empty 8-byte bitmap.
#[test]
fn full_binary_parse_mti_only_empty_bitmap() {
    let wire = cat(&[b"\x08\x00", b"\x00\x00\x00\x00\x00\x00\x00\x00"]);
    assert_eq!(wire.len(), 2 + 8);

    let got =
        parse_with(&wire, Dialect::FullBinary).expect("FullBinary minimal message must parse");
    assert_eq!(&got.mti, b"0800");
    assert!(got.fields.is_empty(), "got fields = {:?}", got.fields);
}

/// B2. FullBinary auth skeleton: MTI 0200 + bitmap (fields 3, 4, 7) + BCD
/// payloads. Bitmap byte 0 = 0x32 (bits 3, 4, 7 → fields 3, 4, 7).
#[test]
fn full_binary_parse_mti_0200_fields_3_4_7() {
    let wire = cat(&[
        b"\x02\x00",                         // MTI 0200
        b"\x32\x00\x00\x00\x00\x00\x00\x00", // bitmap: fields 3, 4, 7
        b"\x00\x00\x00",                     // field 3 (6 digits) = "000000"
        b"\x00\x00\x00\x01\x00\x00",         // field 4 (12 digits) = "000000010000"
        b"\x11\x30\x12\x00\x00",             // field 7 (10 digits) = "1130120000"
    ]);

    let got = parse_with(&wire, Dialect::FullBinary).expect("FullBinary skeleton must parse");
    assert_eq!(&got.mti, b"0200");
    assert_eq!(got.fields.len(), 3);
    assert_eq!(got.fields.get(&3).unwrap().as_slice(), b"000000");
    assert_eq!(got.fields.get(&4).unwrap().as_slice(), b"000000010000");
    assert_eq!(got.fields.get(&7).unwrap().as_slice(), b"1130120000");
}

/// B3. FullBinary LLVAR PAN. Length prefix is 1 BCD byte (0x16 = 16), data
/// is 8 BCD bytes encoding 16 digits.
#[test]
fn full_binary_parse_llvar_field_2_pan() {
    let wire = cat(&[
        b"\x02\x00",                         // MTI 0200
        b"\x40\x00\x00\x00\x00\x00\x00\x00", // bitmap: field 2
        b"\x16",                             // LLVAR length 16
        b"\x41\x11\x11\x11\x11\x11\x11\x11", // PAN "4111111111111111"
    ]);

    let got = parse_with(&wire, Dialect::FullBinary).expect("FullBinary LLVAR PAN must parse");
    assert_eq!(got.fields.len(), 1);
    assert_eq!(got.fields.get(&2).unwrap().as_slice(), b"4111111111111111");
}

/// B4. FullBinary secondary bitmap. Primary byte 0 = 0x80 (field 1 set →
/// secondary present); secondary byte 0 = 0x04 (field 70). Field 70 is
/// Numeric Fixed(3) so wire data = 2 BCD bytes ("301" → 0x03 0x01).
#[test]
fn full_binary_parse_secondary_bitmap_field_70() {
    let wire = cat(&[
        b"\x02\x00",
        b"\x80\x00\x00\x00\x00\x00\x00\x00", // primary, secondary-present
        b"\x04\x00\x00\x00\x00\x00\x00\x00", // secondary, field 70
        b"\x03\x01",                         // BCD for "301"
    ]);

    let got = parse_with(&wire, Dialect::FullBinary).expect("secondary-bitmap must parse");
    assert_eq!(got.fields.get(&70).unwrap().as_slice(), b"301");
    assert!(!got.fields.contains_key(&1));
}

/// B5. Invalid BCD nibble detection: byte 0xCA has a high nibble of 12 — must
/// surface as a BCD error rather than silently mis-decoding.
#[test]
fn full_binary_rejects_invalid_bcd_nibble_in_mti() {
    // 0xCA — nibbles 0xC, 0xA both > 9. Followed by valid-looking padding.
    let wire = cat(&[b"\xca\x00", b"\x00\x00\x00\x00\x00\x00\x00\x00"]);
    let err = parse_with(&wire, Dialect::FullBinary).expect_err("invalid BCD must fail");
    assert!(
        matches!(err, ParseError::InvalidBcdNibble { .. }),
        "expected InvalidBcdNibble, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// FullBinary build + round-trip
// ---------------------------------------------------------------------------

/// B6. build_with(FullBinary) is the byte-exact inverse of B2.
#[test]
fn full_binary_build_matches_b2_wire() {
    let m = msg(
        b"0200",
        &[(3, b"000000"), (4, b"000000010000"), (7, b"1130120000")],
    );
    let expected = cat(&[
        b"\x02\x00",
        b"\x32\x00\x00\x00\x00\x00\x00\x00",
        b"\x00\x00\x00",
        b"\x00\x00\x00\x01\x00\x00",
        b"\x11\x30\x12\x00\x00",
    ]);
    let got = build_with(&m, Dialect::FullBinary).expect("build_with FullBinary must succeed");
    assert_eq!(got, expected);
}

/// B7. Round-trip B4 (secondary bitmap) end-to-end.
#[test]
fn full_binary_roundtrip_secondary_bitmap() {
    let wire = cat(&[
        b"\x02\x00",
        b"\x80\x00\x00\x00\x00\x00\x00\x00",
        b"\x04\x00\x00\x00\x00\x00\x00\x00",
        b"\x03\x01",
    ]);
    let parsed = parse_with(&wire, Dialect::FullBinary).unwrap();
    let rebuilt = build_with(&parsed, Dialect::FullBinary).unwrap();
    assert_eq!(rebuilt, wire);
}

/// B8. FullBinary auto-sniff: wire whose MTI starts with a non-ASCII-digit
/// byte (e.g. 0x02) is ambiguous-free and must resolve to FullBinary.
#[test]
fn sniff_full_binary_input() {
    let wire = cat(&[
        b"\x02\x00",
        b"\x40\x00\x00\x00\x00\x00\x00\x00",
        b"\x16",
        b"\x41\x11\x11\x11\x11\x11\x11\x11",
    ]);
    let (msg, dialect) = parse_any(&wire).expect("must parse");
    assert_eq!(dialect, Dialect::FullBinary);
    assert_eq!(msg.fields.get(&2).unwrap().as_slice(), b"4111111111111111");
}

/// B9. Refresh the sniff-then-build round-trip to cover all three dialects.
#[test]
fn sniff_then_build_roundtrips_full_binary_too() {
    let m = msg(
        b"0200",
        &[(3, b"000000"), (4, b"000000010000"), (7, b"1130120000")],
    );
    let wire = build_with(&m, Dialect::FullBinary).expect("build_with must succeed");
    let (parsed, detected) = parse_any(&wire).expect("parse_any must succeed");
    assert_eq!(detected, Dialect::FullBinary);
    let rebuilt = build_with(&parsed, detected).expect("build_with reverse must succeed");
    assert_eq!(rebuilt, wire);
}
