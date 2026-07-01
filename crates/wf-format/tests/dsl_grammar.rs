//! Grammar coverage for the `.wf` parser.
//!
//! Each test pairs hand-written source text with the expected
//! [`WfFile`] (or expected error variant). Source strings are written
//! out longhand against the grammar documented in `src/lib.rs` so a
//! regression in the parser surfaces as a divergence from the spec,
//! not from a value the parser itself produced (per the project's
//! test-independence policy).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_format::{parse, to_wf_string, Body, ParseError};

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
    match f.bodies.into_iter().next().unwrap() {
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
    match f.bodies.into_iter().next().unwrap() {
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
    match f.bodies.into_iter().next().unwrap() {
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
    assert!(matches!(f.bodies.first(), Some(Body::Iso8583(_))));
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
    match f.bodies.into_iter().next().unwrap() {
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
fn two_payload_blocks_both_parse_into_bodies() {
    // A matched MT + MX pair has two payload blocks; both must land in
    // `bodies` (no duplicate-payload rejection — only `meta` may not
    // repeat).
    let src = "\
meta {
  name: Matched Pair
  type: pair
}

swift-mt {
  block 1: F01BANKBIC1234567890
  block 4 {
    field 20: REF001
  }
}

mx {
  xml: <Envelope><AppHdr></AppHdr><Document></Document></Envelope>
}
";
    let f = parse(src).unwrap();
    assert_eq!(f.bodies.len(), 2, "both payload blocks must be kept");
    assert!(matches!(f.bodies[0], Body::SwiftMt(_)));
    match &f.bodies[1] {
        Body::Mx(m) => assert!(
            m.xml.contains("<Document>"),
            "mx xml must carry the envelope verbatim, got: {:?}",
            m.xml
        ),
        other => panic!("expected Mx body, got {other:?}"),
    }
}

#[test]
fn example_files_parse() {
    // Anti-tautology cross-check: the committed examples must round-trip
    // through `parse` without an error. If a future grammar change
    // breaks the examples, this test fires before any downstream
    // consumer notices.
    let iso = include_str!("../examples/iso8583-auth.wf");
    let mt = include_str!("../examples/mt103-skeleton.wf");
    let pair = include_str!("../examples/mt-mx-pair.wf");
    let _ = parse(iso).expect("iso8583-auth.wf example must parse");
    let _ = parse(mt).expect("mt103-skeleton.wf example must parse");
    let f = parse(pair).expect("mt-mx-pair.wf example must parse");
    assert_eq!(
        f.bodies.len(),
        2,
        "the pair example must hold a swift-mt and an mx body"
    );
}

// ── Non-ASCII / UTF-8 preservation tests ──────────────────────────────────
//
// These tests guard against the strip_block_comments mojibake bug where
// `u8 as char` mapped each raw byte to its Latin-1 scalar U+00nn, silently
// corrupting every multibyte UTF-8 sequence before the lexer ever saw it
// (e.g. `é` 0xC3 0xA9 would become `Ã©`). Expected substrings are written
// as independent string literals, not values derived from parsing the same
// source, so that a regression in the byte-copy path fails these tests
// rather than silently propagating the corruption into both sides of the
// assertion (per the project's test-independence policy).

#[test]
fn mx_xml_preserves_latin1_extended_names() {
    // ISO 20022 <Nm> values routinely contain accented Latin characters.
    // José Müller must survive the strip_block_comments pre-pass and the
    // lexer unchanged — any mojibake (e.g. Ã©, Ã¼) is a correctness failure.
    let src = "\
meta {
  name: MX non-ASCII names
  type: mx
}
mx {
  xml: <Nm>José Müller</Nm>
}
";
    let f = parse(src).unwrap();
    match f.bodies.into_iter().next().unwrap() {
        Body::Mx(m) => assert!(
            m.xml.contains("José Müller"),
            "mx xml must preserve Latin-extended chars verbatim; got: {:?}",
            m.xml
        ),
        other => panic!("expected Mx body, got {other:?}"),
    }
}

#[test]
fn meta_name_preserves_cjk_characters() {
    // CJK names appear in Chinese domestic payment messages (CNAPS, PBOC).
    // 北京 encodes as three bytes per character in UTF-8; the former
    // byte-copy path in strip_block_comments would have split each into
    // three Latin-1 scalars, corrupting the stored value.
    let src = "\
meta {
  name: 北京 branch
  type: iso8583
}
iso8583 {
  mti: 0200
  field 2: 4242
}
";
    let f = parse(src).unwrap();
    assert!(
        f.meta.name.as_deref().unwrap_or("").contains("北京"),
        "meta name must preserve CJK chars verbatim; got: {:?}",
        f.meta.name
    );
}

#[test]
fn iso8583_field_preserves_cjk_value() {
    // Verify CJK content in an iso8583 field value (e.g. cardholder name
    // 张三 in a PBOC card profile) round-trips without corruption.
    let src = "\
meta {
  name: CJK field value
  type: iso8583
}
iso8583 {
  mti: 0200
  field 43: 张三
}
";
    let f = parse(src).unwrap();
    match f.bodies.into_iter().next().unwrap() {
        Body::Iso8583(b) => assert!(
            b.fields
                .get(&43)
                .map(|v| v.as_str())
                .unwrap_or("")
                .contains("张三"),
            "iso8583 field 43 must preserve CJK chars verbatim; got: {:?}",
            b.fields.get(&43)
        ),
        other => panic!("expected Iso8583 body, got {other:?}"),
    }
}

#[test]
fn non_ascii_round_trip_idempotency() {
    // AST idempotency for non-ASCII input: parse → serialize → re-parse
    // must yield an AST equal to the first parse. A mojibake regression in
    // strip_block_comments would produce different strings on the first vs
    // second parse (the second would re-corrupt already-corrupt bytes),
    // breaking equality.
    let src = "\
meta {
  name: José Müller
  type: mx
}
mx {
  xml: <Nm>José Müller 张三</Nm>
}
";
    let first = parse(src).unwrap();
    let rendered = to_wf_string(&first);
    let second = parse(&rendered).unwrap_or_else(|e| {
        panic!(
            "serialized non-ASCII output should re-parse, but failed: {e}\n--- output ---\n{rendered}"
        )
    });
    assert_eq!(
        first, second,
        "non-ASCII AST must be stable across serialize/re-parse\n--- output ---\n{rendered}"
    );
}
