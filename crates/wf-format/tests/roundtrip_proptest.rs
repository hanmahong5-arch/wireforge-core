//! Property-based round-trip tests for the wf-format AST serializer/parser.
//!
//! Property: `parse(to_wf_string(f)) == Ok(f)` for every valid `WfFile`.
//!
//! Generator design — each string is restricted to the format's
//! "single-line carryable value" contract:
//!   • No `\n`, `\r` (lexer breaks on newlines)
//!   • No `{`, `}` at the outer value level (lexer uses braces as structure)
//!   • No `//` adjacent pair except inside `mx { xml: }` (which uses the
//!     opaque reader) — `//` in a regular value would be stripped as a comment
//!   • No leading/trailing ASCII whitespace — the writer/reader both trim, so
//!     a generated value with leading spaces would legitimately not round-trip
//!   • Non-empty after trimming, except where the empty-value path is tested
//!
//! The `mx { xml: }` value is intentionally allowed `//` (covered by the
//! `//`-not-truncated fix), `<`, `>`, `:`, but still no `\n`/`\r`/`{`/`}`.
//!
//! Non-ASCII (Latin-1-extended é,ü,ñ and CJK 北,京,张) is included to lock the
//! mojibake fix; these must round-trip intact.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use std::collections::BTreeMap;
use wf_format::{
    ast::{Body, Iso8583Body, Meta, MxBody, RawBody, SwiftMtBody, WfFile},
    parse, to_wf_string,
};

// ---------------------------------------------------------------------------
// Proptest config
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // =======================================================================
    // TEST 1 — wf-format AST round-trip idempotency
    // =======================================================================

    #[test]
    fn prop_roundtrip_meta_only(meta in arb_meta()) {
        let f = WfFile { meta, bodies: vec![] };
        assert_round_trips(&f);
    }

    #[test]
    fn prop_roundtrip_with_iso8583_body(
        meta in arb_meta(),
        body in arb_iso8583_body(),
    ) {
        let f = WfFile { meta, bodies: vec![Body::Iso8583(body)] };
        assert_round_trips(&f);
    }

    #[test]
    fn prop_roundtrip_with_swift_mt_body(
        meta in arb_meta(),
        body in arb_swift_mt_body(),
    ) {
        let f = WfFile { meta, bodies: vec![Body::SwiftMt(body)] };
        assert_round_trips(&f);
    }

    #[test]
    fn prop_roundtrip_with_mx_body(
        meta in arb_meta(),
        body in arb_mx_body(),
    ) {
        let f = WfFile { meta, bodies: vec![Body::Mx(body)] };
        assert_round_trips(&f);
    }

    #[test]
    fn prop_roundtrip_with_raw_body(
        meta in arb_meta(),
        body in arb_raw_body(),
    ) {
        let f = WfFile { meta, bodies: vec![Body::Raw(body)] };
        assert_round_trips(&f);
    }

    #[test]
    fn prop_roundtrip_multi_body(
        meta in arb_meta(),
        bodies in prop::collection::vec(arb_body(), 0..=3),
    ) {
        let f = WfFile { meta, bodies };
        assert_round_trips(&f);
    }

    /// Extra: mx values with `//` substrings (the http:// namespace-URI fix).
    #[test]
    fn prop_roundtrip_mx_with_double_slash(
        meta in arb_meta(),
        prefix in arb_xml_value_string(),
        suffix in arb_xml_value_string(),
    ) {
        // Construct an xml value that contains http:// — locks the
        // //‐not‐truncated fix.
        let xml = format!("{prefix}http://example.com/schema{suffix}");
        // Reject if forbidden chars snuck in through the composed string.
        prop_assume!(
            !xml.contains('\n')
                && !xml.contains('\r')
                && !xml.contains('{')
                && !xml.contains('}')
                // `/*` / `*/` are stripped by the source pre-pass (see MxBody);
                // they are outside the opaque value's carryable set.
                && !xml.contains("/*")
                && !xml.contains("*/")
        );
        let xml = xml.trim().to_string();
        let body = MxBody { xml };
        let f = WfFile { meta, bodies: vec![Body::Mx(body)] };
        assert_round_trips(&f);
    }
}

// ---------------------------------------------------------------------------
// Core assertion
// ---------------------------------------------------------------------------

fn assert_round_trips(f: &WfFile) {
    let serialized = to_wf_string(f);
    let reparsed = parse(&serialized).unwrap_or_else(|e| {
        panic!(
            "serialized output failed to re-parse: {e}\n\
             --- original AST ---\n{f:#?}\n\
             --- serialized ---\n{serialized}"
        )
    });
    assert_eq!(
        *f, reparsed,
        "AST changed after serialize → re-parse\n\
         --- serialized ---\n{serialized}\n\
         --- original ---\n{f:#?}\n\
         --- reparsed ---\n{reparsed:#?}"
    );
    // Second serialization must be byte-identical (writer is pure/deterministic).
    let serialized2 = to_wf_string(&reparsed);
    assert_eq!(
        serialized, serialized2,
        "second serialization is not byte-identical to the first"
    );
}

// ---------------------------------------------------------------------------
// Charset strategies
// ---------------------------------------------------------------------------

/// Non-ASCII code points included deliberately to lock the mojibake fix.
/// These are carried verbatim; any encoding corruption would be caught here.
const NON_ASCII_EXTRAS: &[char] = &['é', 'ü', 'ñ', '北', '京', '张'];

/// Generate a single safe character for use in a value.
/// Avoids `{`, `}`, `\n`, `\r`.
fn arb_safe_char() -> impl Strategy<Value = char> {
    prop_oneof![
        // Printable ASCII that are safe (no {, }, \n, \r)
        (0x21u32..=0x7Eu32)
            .prop_filter("not brace", |&c| c != b'{' as u32 && c != b'}' as u32)
            .prop_map(|c| char::from_u32(c).unwrap()),
        // Space is safe in the middle of a value (not at start/end after trim)
        Just(' '),
        // Non-ASCII round-trip chars
        prop::sample::select(NON_ASCII_EXTRAS),
    ]
}

/// Generate a string from safe characters. The resulting string:
///   • Contains no `\n`, `\r`, `{`, `}`
///   • Contains no `//` (would be treated as a line comment by the value reader)
///   • Contains no `/*` (the block-comment pre-pass strips `/* ... */` from the
///     entire source before the lexer runs — any `/*` in a value would be eaten)
///   • Is trimmed of leading/trailing whitespace (to stay within writer contract)
///   • The empty string is allowed (maps to the empty-value path in the writer)
fn arb_value_string() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_safe_char(), 0..=40)
        .prop_map(|chars| {
            let s: String = chars.into_iter().collect();
            // Replace `//` and `/*` with `/ ` (and `/ ` respectively) so the
            // lexer and block-comment stripper don't treat them as comments.
            let s = replace_comment_sequences(&s);
            // Trim leading/trailing whitespace — the writer/reader both trim.
            s.trim().to_string()
        })
        .prop_filter("no comment sequences", |s| {
            !s.contains("//") && !s.contains("/*")
        })
}

/// Replace `//` with `/ ` and `/*` with `/ ` to avoid comment-like sequences.
///
/// The `strip_block_comments` pre-pass treats `/*` anywhere in source text as
/// the start of a block comment — it does NOT restrict this to outside-value
/// positions. So any value containing `/*` would be stripped on re-parse.
/// Similarly `//` in a regular-mode value is treated as a line comment.
/// We replace both to keep generated values within the carryable-value contract.
fn replace_comment_sequences(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    out.push('/');
                    out.push(' ');
                    chars.next();
                }
                Some('*') => {
                    out.push('/');
                    out.push(' ');
                    chars.next();
                }
                _ => {
                    out.push('/');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Generate an identifier-like string: `[A-Za-z][A-Za-z0-9_-]*`
/// (matches the lexer's Ident rule and is safe as a meta key).
fn arb_ident() -> impl Strategy<Value = String> {
    (
        prop::char::ranges(vec!['A'..='Z', 'a'..='z'].into()),
        prop::collection::vec(
            prop::char::ranges(vec!['A'..='Z', 'a'..='z', '0'..='9', '_'..='_', '-'..='-'].into()),
            0..=10,
        ),
    )
        .prop_map(|(first, rest)| {
            let mut s = String::new();
            s.push(first);
            s.extend(rest);
            s
        })
}

/// Generate a value string suitable for the `mx { xml: }` block.
/// Allowed: `//`, `<`, `>`, `:` — but still no `\n`/`\r`/`{`/`}`.
/// Values are trimmed of leading/trailing whitespace.
fn arb_xml_value_string() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            // Normal printable ASCII excluding braces and newlines
            (0x20u32..=0x7Eu32)
                .prop_filter("not brace or newline", |&c| {
                    c != b'{' as u32 && c != b'}' as u32
                })
                .prop_map(|c| char::from_u32(c).unwrap()),
            // Non-ASCII round-trip chars
            prop::sample::select(NON_ASCII_EXTRAS),
        ],
        0..=40,
    )
    .prop_map(|chars| chars.into_iter().collect::<String>().trim().to_string())
    // The opaque `mx` value carries `//` (e.g. `http://` namespace URIs)
    // verbatim, but NOT the C-style block-comment delimiters `/*` / `*/`:
    // those are stripped by the whole-source `strip_block_comments` pre-pass
    // that runs before lexing. ISO 20022 XML uses `<!-- -->`, never `/* */`,
    // so this is a documented non-issue (see `MxBody`) — exclude them here so
    // the generator stays within the format's carryable set.
    .prop_filter("mx opaque value cannot carry /* or */", |s| {
        !s.contains("/*") && !s.contains("*/")
    })
}

// ---------------------------------------------------------------------------
// Meta strategy
// ---------------------------------------------------------------------------

fn arb_meta() -> impl Strategy<Value = Meta> {
    (
        prop::option::of(arb_value_string()),
        prop::option::of(arb_ident()),
        prop::option::of(arb_value_string()),
        arb_extra_map(),
    )
        .prop_map(|(name, type_, seq, extra)| {
            // `type_` is lower-cased on parse so generate it lower-case already.
            let type_ = type_.map(|s| s.to_ascii_lowercase());
            Meta {
                name,
                type_,
                seq,
                extra,
            }
        })
}

/// An extra map for meta/iso8583/swift/raw blocks.
/// Keys are identifiers; values are safe strings.
fn arb_extra_map() -> impl Strategy<Value = BTreeMap<String, String>> {
    prop::collection::btree_map(arb_ident(), arb_value_string(), 0..=4)
}

// ---------------------------------------------------------------------------
// Body strategies
// ---------------------------------------------------------------------------

fn arb_iso8583_body() -> impl Strategy<Value = Iso8583Body> {
    (
        prop::option::of(arb_value_string()),
        // Field numbers 2..=255 (valid as u8, the parser accepts 0..=255 but
        // field 0 is reserved; we use 2..=200 to stay clearly in-contract).
        prop::collection::btree_map(2u8..=200u8, arb_value_string(), 0..=4),
        arb_extra_map(),
    )
        .prop_map(|(mti, fields, extra)| Iso8583Body { mti, fields, extra })
}

/// Generate block strings for swift-mt `blocks` map (single-line block values).
/// These are the `block 1:`, `block 2:`, etc. values.
/// Block ids 1..=5; we generate a subset using 1..=3 and 5 for the single-line
/// form (4 may conflict with block_4 nested form — handled by mutual exclusion).
fn arb_block_string() -> impl Strategy<Value = String> {
    // Block strings are opaque — use the value string strategy.
    // The lexer accepts balanced braces inside a value (it tracks depth),
    // so a value like `{108:REF}` is fine. However generating balanced
    // arbitrary braces is complex; we simply exclude braces from block values
    // to stay clearly in-contract.
    arb_value_string()
}

/// Generate a swift-mt block_4 map (nested form): keys are tag strings like
/// `field 32A` (the parser reads `field <TAG>` where TAG is `[A-Z0-9]{1..=4}`).
fn arb_block_4_map() -> impl Strategy<Value = BTreeMap<String, String>> {
    let arb_tag =
        prop::collection::vec(prop::char::ranges(vec!['A'..='Z', '0'..='9'].into()), 1..=4)
            .prop_map(|chars| chars.into_iter().collect::<String>());

    // The key stored in block_4 is the full `field TAG` form as parsed.
    // Looking at parse_block_4: the key is assembled as
    //   key.name + (if arg: " " + arg else "")
    // Since `field 32A` enters as name="field", arg="32A", full = "field 32A".
    // But for the round-trip test we must reconstruct exactly what the parser
    // stores. The writer emits: `INDENT2 full: value`, and the parser re-reads
    // it as: name=first_ident, arg=second_token (ident or number), full=concat.
    //
    // To avoid key format issues in the round-trip: we store the key exactly
    // as the parser would produce it — a single identifier or "ident arg" pair.
    // Since the writer writes the key verbatim and the parser reads it the same
    // way, any key that is either a plain ident or "ident number/ident" form
    // will round-trip. We generate simple alpha-only tags (no spaces) and
    // prefix with "field " to match the typical SWIFT form.
    prop::collection::btree_map(
        arb_tag.prop_map(|t| format!("field {t}")),
        arb_value_string(),
        0..=4,
    )
}

fn arb_swift_mt_body() -> impl Strategy<Value = SwiftMtBody> {
    // The two block-4 forms (single-line `blocks[4]` and nested `block_4`) are
    // mutually exclusive. We generate one of three variants:
    //   1. Only single-line blocks (block_4 = None)
    //   2. Single-line blocks 1..3,5 + nested block_4 (Some)
    //   3. Single-line blocks 1..3,5 only, block_4 = None
    prop_oneof![
        // Variant A: single-line blocks only (ids 1..=3, 5), no nested block_4
        (
            prop::collection::btree_map(1u8..=3u8, arb_block_string(), 0..=3),
            prop::option::of(arb_block_string()),
            arb_extra_map(),
        )
            .prop_map(|(mut blocks, b5, extra)| {
                if let Some(v) = b5 {
                    blocks.insert(5, v);
                }
                SwiftMtBody {
                    blocks,
                    block_4: None,
                    extra,
                }
            }),
        // Variant B: nested block_4 (Some), single-line blocks 1..=3,5 only
        (
            prop::collection::btree_map(1u8..=3u8, arb_block_string(), 0..=3),
            prop::option::of(arb_block_string()),
            prop::option::of(arb_block_4_map()),
            arb_extra_map(),
        )
            .prop_map(|(mut blocks, b5, block_4, extra)| {
                if let Some(v) = b5 {
                    blocks.insert(5, v);
                }
                // block_4 is Some → must NOT have blocks[4].
                // The BTreeMap strategy already uses keys 1..=3, so blocks[4]
                // won't appear there. Safe.
                SwiftMtBody {
                    blocks,
                    block_4,
                    extra,
                }
            }),
        // Variant C: single-line block 4 (inside blocks map), no nested block_4.
        (
            prop::collection::btree_map(1u8..=3u8, arb_block_string(), 0..=3),
            arb_block_string(),                   // block 4 value (single-line)
            prop::option::of(arb_block_string()), // block 5 value
            arb_extra_map(),
        )
            .prop_map(|(mut blocks, b4_val, b5, extra)| {
                blocks.insert(4, b4_val);
                if let Some(v) = b5 {
                    blocks.insert(5, v);
                }
                SwiftMtBody {
                    blocks,
                    block_4: None, // single-line form lives in blocks, not block_4
                    extra,
                }
            }),
    ]
}

fn arb_mx_body() -> impl Strategy<Value = MxBody> {
    // The xml value uses the opaque reader (no // stripping), so // is allowed.
    arb_xml_value_string().prop_map(|xml| MxBody { xml })
}

fn arb_raw_body() -> impl Strategy<Value = RawBody> {
    (arb_ident(), arb_extra_map()).prop_map(|(name, entries)| RawBody { name, entries })
}

fn arb_body() -> impl Strategy<Value = Body> {
    prop_oneof![
        arb_iso8583_body().prop_map(Body::Iso8583),
        arb_swift_mt_body().prop_map(Body::SwiftMt),
        arb_mx_body().prop_map(Body::Mx),
        arb_raw_body().prop_map(Body::Raw),
    ]
}
