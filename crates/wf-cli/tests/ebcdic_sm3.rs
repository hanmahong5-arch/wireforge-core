//! Integration tests for the `wf ebcdic` and `wf sm3` entry points.
//!
//! These call the pure lib.rs entry points directly (the binary is a thin
//! dispatcher over them). Anchors against external facts where possible:
//! EBCDIC CP037 0xC1 = 'A', and the GM/T 0004-2012 SM3 vector for "abc".

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_cli::{ebcdic_decode_hex, ebcdic_encode_text, sm3_digest};

#[test]
fn ebcdic_decode_c1c2c3_is_abc() {
    // External fact: in EBCDIC CP037, 0xC1/0xC2/0xC3 map to 'A'/'B'/'C'.
    let out = ebcdic_decode_hex("C1C2C3", "037").unwrap();
    assert_eq!(out, "ABC");
}

#[test]
fn ebcdic_decode_defaults_and_cp500_agree_on_letters() {
    // Uppercase Latin letters share the same code points in CP037 and CP500.
    let cp037 = ebcdic_decode_hex("c1c2c3", "037").unwrap();
    let cp500 = ebcdic_decode_hex("c1c2c3", "500").unwrap();
    assert_eq!(cp037, "ABC");
    assert_eq!(cp500, "ABC");
}

#[test]
fn ebcdic_decode_ignores_whitespace_in_hex() {
    let out = ebcdic_decode_hex("C1 C2 C3", "037").unwrap();
    assert_eq!(out, "ABC");
}

#[test]
fn ebcdic_encode_abc_round_trips() {
    // encode -> decode must return the original text.
    let hex = ebcdic_encode_text("ABC", "037").unwrap();
    let back = ebcdic_decode_hex(&hex, "037").unwrap();
    assert_eq!(back, "ABC");
}

#[test]
fn ebcdic_encode_known_bytes_for_abc() {
    // External fact: "ABC" encodes to 0xC1C2C3 in CP037.
    let hex = ebcdic_encode_text("ABC", "037").unwrap();
    assert_eq!(hex, "c1c2c3");
}

#[test]
fn ebcdic_round_trip_mixed_text() {
    let text = "Hello, World 123";
    let hex = ebcdic_encode_text(text, "037").unwrap();
    let back = ebcdic_decode_hex(&hex, "037").unwrap();
    assert_eq!(back, text);
}

#[test]
fn ebcdic_encode_unrepresentable_char_errors() {
    // A non-Latin character has no EBCDIC CP037 representation; encode must
    // return Err (the binary turns this into a non-zero exit), not panic.
    let result = ebcdic_encode_text("héllo中", "037");
    assert!(result.is_err(), "expected Err for unrepresentable char");
    let msg = result.unwrap_err();
    // Error message must be informative (names the char and position).
    assert!(
        msg.contains("not representable"),
        "error message should explain the failure, got: {msg}"
    );
    assert!(
        msg.contains("position"),
        "error message should name the position, got: {msg}"
    );
}

#[test]
fn ebcdic_unknown_code_page_errors() {
    let result = ebcdic_decode_hex("C1", "999");
    assert!(result.is_err(), "expected Err for unknown code page");
    assert!(result.unwrap_err().contains("code page"));
}

#[test]
fn ebcdic_decode_bad_hex_errors() {
    let result = ebcdic_decode_hex("ZZ", "037");
    assert!(result.is_err(), "expected Err for non-hex input");
}

#[test]
fn sm3_of_abc_matches_known_vector() {
    // External standards anchor: the GM/T 0004-2012 SM3 digest of the ASCII
    // string "abc" is the published reference value below.
    let digest = sm3_digest("abc", true).unwrap();
    assert_eq!(
        digest,
        "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0"
    );
}

#[test]
fn sm3_of_empty_input_matches_known_vector() {
    // GM/T 0004-2012 SM3 digest of the empty input.
    let digest = sm3_digest("", true).unwrap();
    assert_eq!(
        digest,
        "1ab21d8355cfa17f8e61194831e81a8f22bec8c728fefb747ed035eb5082aa2b"
    );
}

#[test]
fn sm3_hex_input_of_616263_matches_abc() {
    // Hex input 0x616263 is the ASCII bytes of "abc"; hashing it as hex must
    // give the same digest as hashing the text "abc".
    let from_hex = sm3_digest("616263", false).unwrap();
    let from_text = sm3_digest("abc", true).unwrap();
    assert_eq!(from_hex, from_text);
}

#[test]
fn sm3_hex_input_ignores_whitespace() {
    let spaced = sm3_digest("61 62 63", false).unwrap();
    let tight = sm3_digest("616263", false).unwrap();
    assert_eq!(spaced, tight);
}

#[test]
fn sm3_bad_hex_input_errors() {
    let result = sm3_digest("XYZ", false);
    assert!(result.is_err(), "expected Err for non-hex input");
}
