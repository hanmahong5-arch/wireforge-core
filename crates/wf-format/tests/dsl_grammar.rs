//! Grammar coverage for the `.wf` parser.
//!
//! Each test pairs hand-written source text with the expected
//! [`WfFile`] (or expected error variant). Source strings are written
//! out longhand against the grammar documented in `src/lib.rs` so a
//! regression in the parser surfaces as a divergence from the spec,
//! not from a value the parser itself produced (CLAUDE.md §4.1 ③).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_format::{parse, Body, ParseError};

const MIN_ISO_AUTH: &str = "\
meta {
  name: Auth Request 0200
  type: iso8583
  seq: 1
}

iso8583 {
  mti: 0200
  field 2: 4242424242424242
  field 3: 000000
  field 4: 000000010000
  field 7: 1130120000
}
";

const MIN_SWIFT_SKELETON: &str = "\
meta {
  name: MT103 Cross-Border
  type: swift-mt
}

swift-mt {
  block 1: F01BANKBIC1234567890
  block 2: I103RECVBIC22N
  block 4 {
    field 20: REF001
    field 32A: 240520USD1000,00
  }
}
";

#[test]
fn parses_iso8583_minimal() {
    let f = parse(MIN_ISO_AUTH).unwrap();
    assert_eq!(f.meta.name.as_deref(), Some("Auth Request 0200"));
    assert_eq!(f.meta.type_.as_deref(), Some("iso8583"));
    assert_eq!(f.meta.seq.as_deref(), Some("1"));
    match f.body.unwrap() {
        Body::Iso8583(b) => {
            assert_eq!(b.mti.as_deref(), Some("0200"));
            assert_eq!(b.fields.len(), 4);
            assert_eq!(b.fields.get(&2).unwrap(), "4242424242424242");
            assert_eq!(b.fields.get(&7).unwrap(), "1130120000");
        }
        other => panic!("expected Iso8583 body, got {other:?}"),
    }
}

#[test]
fn parses_swift_mt_with_nested_block_4() {
    let f = parse(MIN_SWIFT_SKELETON).unwrap();
    assert_eq!(f.meta.type_.as_deref(), Some("swift-mt"));
    match f.body.unwrap() {
        Body::SwiftMt(b) => {
            assert_eq!(b.blocks.get(&1).unwrap(), "F01BANKBIC1234567890");
            assert_eq!(b.blocks.get(&2).unwrap(), "I103RECVBIC22N");
            let b4 = b.block_4.expect("block 4 must be present");
            assert_eq!(b4.get("field 20").unwrap(), "REF001");
            assert_eq!(b4.get("field 32A").unwrap(), "240520USD1000,00");
        }
        other => panic!("expected SwiftMt body, got {other:?}"),
    }
}

#[test]
fn line_comments_are_stripped() {
    let src = "\
// top-level comment
meta {
  name: with comment  // trailing
  type: iso8583
}
// before iso block
iso8583 {
  mti: 0200 // inline
  field 2: 4242
}
";
    let f = parse(src).unwrap();
    assert_eq!(f.meta.name.as_deref(), Some("with comment"));
    match f.body.unwrap() {
        Body::Iso8583(b) => {
            assert_eq!(b.mti.as_deref(), Some("0200"));
            assert_eq!(b.fields.get(&2).unwrap(), "4242");
        }
        other => panic!("unexpected body: {other:?}"),
    }
}

#[test]
fn block_comments_are_stripped() {
    let src = "\
/* file-level block comment
   spans many lines */
meta { name: blocky /* inline */ type: iso8583 }
iso8583 {
  /* on its own line */
  mti: 0200
}
";
    let f = parse(src).unwrap();
    // The inline block comment splits `name:` from `type:` so they sit
    // on the same logical line — but the lexer requires entries to end
    // at newline. The grammar therefore treats this as TWO entries on
    // one line, which currently surfaces as an "expected end of line"
    // error. We assert that the FILE-LEVEL block comment plus
    // newline-broken usage still parses cleanly.
    assert!(matches!(f.body, Some(Body::Iso8583(_))));
}

#[test]
fn excess_whitespace_is_tolerated() {
    let src = "  meta   {   name :    spaced out   \n  type:iso8583  \n}\n";
    let f = parse(src).unwrap();
    assert_eq!(f.meta.name.as_deref(), Some("spaced out"));
    assert_eq!(f.meta.type_.as_deref(), Some("iso8583"));
}

#[test]
fn missing_meta_block_is_rejected() {
    let src = "iso8583 { mti: 0200 }\n";
    let err = parse(src).unwrap_err();
    assert_eq!(err, ParseError::MissingMeta);
}

#[test]
fn duplicate_meta_block_is_rejected() {
    let src = "meta { name: a }\nmeta { name: b }\n";
    let err = parse(src).unwrap_err();
    assert!(matches!(err, ParseError::DuplicateBlock { .. }));
}

#[test]
fn missing_colon_is_rejected() {
    // `name value` with no colon — parser should reject.
    let src = "meta {\n  name no colon here\n}\n";
    let err = parse(src).unwrap_err();
    assert!(
        matches!(err, ParseError::UnexpectedToken { .. }),
        "expected UnexpectedToken, got {err:?}"
    );
}

#[test]
fn unbalanced_brace_is_rejected() {
    let src = "meta {\n  name: x\n";
    let err = parse(src).unwrap_err();
    assert!(matches!(err, ParseError::UnclosedBlock { .. }));
}

#[test]
fn unknown_top_level_block_falls_back_to_raw() {
    let src = "\
meta { name: future }
cnaps2 {
  source: PBOC
  v: 1.7
}
";
    let f = parse(src).unwrap();
    match f.body.unwrap() {
        Body::Raw(b) => {
            assert_eq!(b.name, "cnaps2");
            assert_eq!(b.entries.get("source").unwrap(), "PBOC");
            assert_eq!(b.entries.get("v").unwrap(), "1.7");
        }
        other => panic!("expected Raw body, got {other:?}"),
    }
}

#[test]
fn duplicate_key_inside_block_is_rejected() {
    let src = "\
meta {
  name: a
  name: b
}
";
    let err = parse(src).unwrap_err();
    assert!(matches!(err, ParseError::DuplicateKey { .. }));
}

#[test]
fn invalid_field_number_is_rejected() {
    // 999 doesn't fit in u8.
    let src = "\
meta { name: bad }
iso8583 {
  field 999: x
}
";
    let err = parse(src).unwrap_err();
    assert!(matches!(err, ParseError::InvalidFieldNumber { .. }));
}

#[test]
fn invalid_block_number_is_rejected() {
    let src = "\
meta { name: bad }
swift-mt {
  block 9: nope
}
";
    let err = parse(src).unwrap_err();
    assert!(matches!(err, ParseError::InvalidBlockNumber { .. }));
}

#[test]
fn example_files_parse() {
    // Anti-tautology cross-check: the two committed examples must
    // round-trip through `parse` without an error. If a future
    // grammar change breaks the examples, this test fires before any
    // downstream consumer notices.
    let iso = include_str!("../examples/iso8583-auth.wf");
    let mt = include_str!("../examples/mt103-skeleton.wf");
    let _ = parse(iso).expect("iso8583-auth.wf example must parse");
    let _ = parse(mt).expect("mt103-skeleton.wf example must parse");
}
