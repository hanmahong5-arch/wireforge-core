//! End-to-end semantic tests: parse a real MT103 skeleton through the
//! structural layer, then decode block-4 fields via
//! [`MtMessage::decode_field`].
//!
//! Vectors come from the SWIFT MT User Handbook canonical MT103 example
//! (the same one used in `tests/swift_structure.rs`). Per the project's
//! test-independence policy, the test inputs are not regenerated from the
//! decoder's own output — the wire is written longhand and the decoder is
//! asked to agree with it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_codec::swift::{parse, DecodeError, FieldSemantic};

const MT103_FULL: &str = "{1:F01BANKBEBBAXXX1234567890}\
{2:I103BANKDEFFXXXXN}\
{3:{108:MYREF12345}}\
{4:\r
:20:REFERENCE001\r
:23B:CRED\r
:32A:240520USD1000,00\r
:50K:/12345678\r
ACME CORP\r
123 MAIN ST\r
:59:/87654321\r
BENEFICIARY LTD\r
:71A:SHA\r
-}{5:{MAC:12345678}{CHK:ABCDEF123456}}";

#[test]
fn decode_field_20_lifts_to_reference() {
    let msg = parse(MT103_FULL).unwrap();
    let decoded = msg.decode_field("20").expect("tag 20 present").unwrap();
    assert_eq!(
        decoded,
        FieldSemantic::Reference("REFERENCE001".to_string())
    );
}

#[test]
fn decode_field_32a_splits_date_currency_amount() {
    let msg = parse(MT103_FULL).unwrap();
    let decoded = msg.decode_field("32A").expect("tag 32A present").unwrap();
    match decoded {
        FieldSemantic::ValueDateAmount {
            date,
            currency,
            amount,
        } => {
            assert_eq!(date, "240520");
            assert_eq!(currency, "USD");
            assert_eq!(amount, "1000,00");
        }
        other => panic!("expected ValueDateAmount, got {other:?}"),
    }
}

#[test]
fn decode_field_50k_lifts_account_and_name_lines() {
    let msg = parse(MT103_FULL).unwrap();
    let decoded = msg.decode_field("50K").expect("tag 50K present").unwrap();
    match decoded {
        FieldSemantic::Party { account, lines } => {
            assert_eq!(account.as_deref(), Some("12345678"));
            assert_eq!(
                lines,
                vec!["ACME CORP".to_string(), "123 MAIN ST".to_string()]
            );
        }
        other => panic!("expected Party, got {other:?}"),
    }
}

#[test]
fn decode_field_returns_none_for_absent_tag() {
    let msg = parse(MT103_FULL).unwrap();
    assert!(msg.decode_field("99Z").is_none());
}

#[test]
fn decode_field_returns_raw_for_unknown_present_tag() {
    // 23B is present in the wire but no decoder is registered for it —
    // contract: caller still gets Some(Ok(Raw)) so a downstream diff
    // layer can compare by string verbatim.
    let msg = parse(MT103_FULL).unwrap();
    let decoded = msg.decode_field("23B").expect("23B present").unwrap();
    assert_eq!(decoded, FieldSemantic::Raw("CRED".to_string()));
}

#[test]
fn decode_field_returns_error_for_malformed_known_tag() {
    // Surgically corrupt the 32A amount component — `,,` is not a valid
    // SWIFT decimal. The decoder must surface a DecodeError rather than
    // silently degrade to Raw.
    let bad = MT103_FULL.replace("1000,00", "1000,,0");
    let msg = parse(&bad).unwrap();
    let err = msg
        .decode_field("32A")
        .expect("32A present")
        .expect_err("malformed amount must fail");
    assert!(matches!(err, DecodeError::InvalidAmount { tag: "32A", .. }));
}

#[test]
fn decode_field_71a_falls_through_to_raw_unknown_tag() {
    // 71A is in the wire but not in our MVP decoder set — verify it
    // routes through registry's fall-through path rather than panicking.
    let msg = parse(MT103_FULL).unwrap();
    let decoded = msg.decode_field("71A").expect("71A present").unwrap();
    assert_eq!(decoded, FieldSemantic::Raw("SHA".to_string()));
}
