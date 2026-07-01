//! Structural tests for [`wf_codec::swift::parse`].
//!
//! Vectors are hand-written against the SWIFT MT user handbook layout
//! (FIN message blocks 1-5, MT103 customer credit transfer). They cover
//! the wrapper structure only; semantic field decoding lives in a
//! separate test file once a typed application layer lands.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use wf_codec::swift::{build, parse, Block, MtBuildError, MtField, MtMessage, MtParseError};

/// Standard MT103 skeleton: blocks 1 + 2 + 3 + 4 (multi-field) + 5.
/// Multi-line `:50K:` and `:59:` field values exercise the embedded-CRLF
/// path that real MT103 messages always trigger.
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
fn full_mt103_parses_all_five_blocks() {
    let msg = parse(MT103_FULL).unwrap();
    assert_eq!(msg.blocks.len(), 5);
    for id in 1..=5u8 {
        assert!(msg.blocks.contains_key(&id), "missing block {}", id);
    }
}

#[test]
fn block_1_2_preserved_verbatim() {
    let msg = parse(MT103_FULL).unwrap();
    match msg.blocks.get(&1).unwrap() {
        Block::Raw(s) => assert_eq!(s, "F01BANKBEBBAXXX1234567890"),
        _ => panic!("block 1 must be Raw"),
    }
    match msg.blocks.get(&2).unwrap() {
        Block::Raw(s) => assert_eq!(s, "I103BANKDEFFXXXXN"),
        _ => panic!("block 2 must be Raw"),
    }
}

#[test]
fn block_3_parses_user_header_subblocks() {
    let msg = parse(MT103_FULL).unwrap();
    let subs = match msg.blocks.get(&3).unwrap() {
        Block::Tagged(s) => s,
        _ => panic!("block 3 must be Tagged"),
    };
    assert_eq!(subs.len(), 1);
    assert_eq!(subs[0].tag, "108");
    assert_eq!(subs[0].value, "MYREF12345");
}

#[test]
fn block_4_parses_all_fields_in_order() {
    let msg = parse(MT103_FULL).unwrap();
    let fields = msg.text().expect("block 4 must be Text");
    let tags: Vec<&str> = fields.iter().map(|f| f.tag.as_str()).collect();
    assert_eq!(tags, vec!["20", "23B", "32A", "50K", "59", "71A"]);
    assert_eq!(msg.field("20").unwrap().value, "REFERENCE001");
    assert_eq!(msg.field("23B").unwrap().value, "CRED");
    assert_eq!(msg.field("32A").unwrap().value, "240520USD1000,00");
    assert_eq!(msg.field("71A").unwrap().value, "SHA");
}

#[test]
fn block_4_preserves_multiline_field_values() {
    let msg = parse(MT103_FULL).unwrap();
    let f50k = msg.field("50K").expect("50K present");
    // The :50K: value continues across three physical lines in the
    // wire format. All three must round-trip into one value string,
    // CRLFs preserved so a future builder can re-emit byte-exactly.
    assert!(f50k.value.starts_with("/12345678"));
    assert!(f50k.value.contains("ACME CORP"));
    assert!(f50k.value.contains("123 MAIN ST"));
    assert_eq!(f50k.value.matches('\r').count(), 2);
}

#[test]
fn block_5_parses_trailer_subblocks() {
    let msg = parse(MT103_FULL).unwrap();
    let subs = match msg.blocks.get(&5).unwrap() {
        Block::Tagged(s) => s,
        _ => panic!("block 5 must be Tagged"),
    };
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0].tag, "MAC");
    assert_eq!(subs[0].value, "12345678");
    assert_eq!(subs[1].tag, "CHK");
    assert_eq!(subs[1].value, "ABCDEF123456");
}

// --- error surfaces ------------------------------------------------------

#[test]
fn missing_text_terminator_rejected() {
    let wire = "{4:\r\n:20:REFERENCE}"; // missing "\r\n-"
    let err = parse(wire).unwrap_err();
    assert_eq!(err, MtParseError::MissingTextTerminator);
}

#[test]
fn unexpected_trailing_bytes_rejected() {
    let wire = "{1:HEADER}garbage";
    let err = parse(wire).unwrap_err();
    assert!(matches!(err, MtParseError::UnexpectedTrailingBytes { .. }));
}

#[test]
fn whitespace_between_blocks_is_tolerated() {
    let wire = "{1:HEADER}\r\n{2:I103XXXN}";
    let msg = parse(wire).unwrap();
    assert_eq!(msg.blocks.len(), 2);
}

#[test]
fn empty_block_4_with_only_terminator() {
    let wire = "{1:H}{4:\r\n-}";
    let msg = parse(wire).unwrap();
    let fields = msg.text().expect("block 4 must be Text");
    assert!(fields.is_empty());
}

#[test]
fn lone_lf_text_terminator_accepted() {
    // Permissive: some Linux-side toolchains strip the \r before piping
    // MT messages through; we accept "\n-" as equivalent to "\r\n-".
    let wire = "{4:\n:20:REF\n-}";
    let msg = parse(wire).unwrap();
    let fields = msg.text().unwrap();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].tag, "20");
    assert_eq!(fields[0].value, "REF");
}

#[test]
fn duplicate_block_4_rejected() {
    let wire = "{4:\r\n:20:A\r\n-}{4:\r\n:21:B\r\n-}";
    let err = parse(wire).unwrap_err();
    assert_eq!(err, MtParseError::DuplicateBlock { id: 4 });
}

#[test]
fn unbalanced_brace_inside_tagged_block_rejected() {
    let wire = "{5:{MAC:12345678";
    let err = parse(wire).unwrap_err();
    assert!(matches!(err, MtParseError::UnbalancedBrace { .. }));
}

#[test]
fn block_id_with_extra_digits_rejected() {
    // "10" is two digits, not a valid block id.
    let wire = "{10:nope}";
    let err = parse(wire).unwrap_err();
    assert!(matches!(err, MtParseError::InvalidBlockId { .. }));
}

#[test]
fn block_4_tag_with_letters_only_parses() {
    // Some MT vendors emit `:NS:` (NotSpecified) — make sure non-numeric
    // tags that are still alphanumeric uppercase parse cleanly.
    let wire = "{4:\r\n:NS:CUSTOM-PAYLOAD\r\n-}";
    let msg = parse(wire).unwrap();
    let f = &msg.text().unwrap()[0];
    assert_eq!(f.tag, "NS");
    assert_eq!(f.value, "CUSTOM-PAYLOAD");
}

// --- builder + round-trip ------------------------------------------------

#[test]
fn round_trip_full_mt103_byte_exact() {
    // Per the project's test-independence policy ("measurement and subject
    // must not share a source") — the wire vector here is the same
    // MT103_FULL constant fed into parse; build then verifies exact byte
    // recovery. Because MT103_FULL is hand-written longhand (NOT
    // regenerated from build()), this is parser-build correctness verified
    // against an externally-stated spec layout, not a tautology.
    let msg = parse(MT103_FULL).unwrap();
    let rebuilt = build(&msg).unwrap();
    assert_eq!(rebuilt, MT103_FULL);
}

#[test]
fn build_emits_canonical_crlf_in_block_4() {
    let wire_lf = "{4:\n:20:REF\n-}";
    let msg = parse(wire_lf).unwrap();
    let rebuilt = build(&msg).unwrap();
    // LF-only input → canonical CRLF output. Same meaning, different bytes.
    assert_eq!(rebuilt, "{4:\r\n:20:REF\r\n-}");
}

#[test]
fn build_skips_omitted_blocks() {
    // Construct a message that has only blocks 1 and 4 — blocks 3 and 5
    // omitted entirely. Builder must NOT emit empty `{3:}` placeholders.
    let mut blocks = BTreeMap::new();
    blocks.insert(1u8, Block::Raw("HEADER".into()));
    blocks.insert(
        4u8,
        Block::Text(vec![MtField {
            tag: "20".into(),
            value: "REF".into(),
        }]),
    );
    let msg = MtMessage { blocks };
    let s = build(&msg).unwrap();
    assert_eq!(s, "{1:HEADER}{4:\r\n:20:REF\r\n-}");
}

#[test]
fn build_rejects_invalid_field_tag() {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        4u8,
        Block::Text(vec![MtField {
            tag: "bad-tag!".into(),
            value: "ignored".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(matches!(err, MtBuildError::InvalidFieldTag { .. }));
}

#[test]
fn build_rejects_empty_subblock_tag() {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        3u8,
        Block::Tagged(vec![wf_codec::swift::MtSubBlock {
            tag: String::new(),
            value: "V".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert_eq!(err, MtBuildError::EmptySubBlockTag);
}

#[test]
fn build_rejects_block_kind_id_mismatch() {
    // Block id 1 with Block::Text content — caller built an inconsistent
    // MtMessage. Surface as InvalidBlockId rather than silently mis-emit.
    let mut blocks = BTreeMap::new();
    blocks.insert(1u8, Block::Text(Vec::new()));
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert_eq!(err, MtBuildError::InvalidBlockId { id: 1 });
}

// --- FIX C: sub-block tag validation (block 3 / 5) -----------------------

/// A block-3 sub-block whose tag contains `:` would produce wire like
/// `{3:{1:0:value}}`. The receiver parses this as tag="1", value="0:value" —
/// the original tag "1:0" is lost. The builder must reject it.
#[test]
fn build_rejects_subblock_tag_with_colon() {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        3u8,
        Block::Tagged(vec![wf_codec::swift::MtSubBlock {
            tag: "1:0".into(),
            value: "SOMEREF".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(
        matches!(err, MtBuildError::InvalidSubBlockTag { ref tag } if tag == "1:0"),
        "expected InvalidSubBlockTag {{ tag: \"1:0\" }}, got {err:?}",
    );
}

/// A block-5 sub-block tag containing `{` or `}` is likewise a wire delimiter
/// and must be rejected.
#[test]
fn build_rejects_subblock_tag_with_braces() {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        5u8,
        Block::Tagged(vec![wf_codec::swift::MtSubBlock {
            tag: "A{B".into(),
            value: "VAL".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(
        matches!(err, MtBuildError::InvalidSubBlockTag { .. }),
        "expected InvalidSubBlockTag, got {err:?}",
    );
}

// --- FIX D: block-4 field value misparse guards --------------------------

/// A block-4 field value containing `\n:` would cause the receiver to split
/// it into two fields. The builder must reject it.
#[test]
fn build_rejects_block4_value_with_newline_colon() {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        4u8,
        Block::Text(vec![MtField {
            tag: "20".into(),
            // The \n: sequence is the field-separator on the receiver side.
            value: "PART1\n:FAKE_TAG".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(
        matches!(err, MtBuildError::ValueWouldMisparse { ref tag } if tag == "20"),
        "expected ValueWouldMisparse {{ tag: \"20\" }}, got {err:?}",
    );
}

/// A block-4 field value with a line that is exactly `-` would collide with
/// the block terminator and truncate the message on the receiver side.
#[test]
fn build_rejects_block4_value_with_lone_dash_line() {
    let mut blocks = BTreeMap::new();
    blocks.insert(
        4u8,
        Block::Text(vec![MtField {
            tag: "20".into(),
            value: "FIRST LINE\n-\nTHIRD LINE".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(
        matches!(err, MtBuildError::ValueWouldMisparse { ref tag } if tag == "20"),
        "expected ValueWouldMisparse {{ tag: \"20\" }}, got {err:?}",
    );
}

#[test]
fn build_rejects_block4_value_with_brace() {
    // A `{` in a block-4 value is a FIN brace delimiter: it would make the
    // receiver's find_matching_brace consume the block-4 close, leaving the
    // message unbalanced on re-parse. (Found by the round-trip proptest.)
    let mut blocks = BTreeMap::new();
    blocks.insert(
        4u8,
        Block::Text(vec![MtField {
            tag: "70".into(),
            value: "INVOICE {123}".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(
        matches!(err, MtBuildError::ValueWouldMisparse { ref tag } if tag == "70"),
        "expected ValueWouldMisparse {{ tag: \"70\" }}, got {err:?}",
    );
}

#[test]
fn build_rejects_subblock_value_with_brace() {
    // A `}` in a block-3/5 sub-block value would close the sub-block early
    // and corrupt framing on re-parse.
    let mut blocks = BTreeMap::new();
    blocks.insert(
        3u8,
        Block::Tagged(vec![wf_codec::swift::MtSubBlock {
            tag: "108".into(),
            value: "REF}END".into(),
        }]),
    );
    let err = build(&MtMessage { blocks }).unwrap_err();
    assert!(
        matches!(err, MtBuildError::ValueWouldMisparse { ref tag } if tag == "108"),
        "expected ValueWouldMisparse {{ tag: \"108\" }}, got {err:?}",
    );
}
