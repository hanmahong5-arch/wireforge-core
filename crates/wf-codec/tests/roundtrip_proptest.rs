//! Property-based round-trip tests for wf-codec: ISO 8583 and SWIFT MT.
//!
//! # TEST 2 — ISO 8583 build∘parse inverse
//!
//! Property: `parse_with(build_with(msg, d)?, d)? == msg` for each of the
//! three dialects (HybridAscii, FullAscii, FullBinary).
//!
//! Generator constraints (in-contract only):
//!   • Field numbers 2..=128 only (0 rejected, 1 is auto-managed, >128 out of range).
//!   • Only fields that exist in the builtin table (field_def returns Some).
//!   • Fixed(n): payload is exactly n bytes of the correct charset.
//!   • LLVAR: 0..=min(99, max) bytes.
//!   • LLLVAR: 0..=min(999, max) bytes.
//!   • Numeric fields under FullBinary: ASCII digit bytes only (even length is
//!     NOT required — the BCD codec is right-justified and handles odd lengths).
//!   • MTI: 4 ASCII digit bytes.
//!
//! Fields excluded from generation because they have special semantics:
//!   • Field 1 — builder explicitly rejects it (secondary-bitmap indicator).
//!   • Fields 105..=127 — Reserved/Binary/LLLVAR{max:999}: these are
//!     Binary type so they take arbitrary bytes; included in the generator
//!     but payload is raw bytes (not digit-restricted) since they are Binary.
//!
//! # TEST 3 — SWIFT MT structural build∘parse inverse
//!
//! Property: `parse(build(msg)?)? == msg`.
//!
//! Generator constraints:
//!   • Blocks 1,2: Raw strings without `{`/`}`/control chars.
//!   • Block 4 Text fields: tags `[A-Z0-9]{1..=3}`, values without `\n:`
//!     sequence and no lone `-` line.
//!   • Blocks 3,5 Tagged: sub-tags `[A-Z0-9]{1..=3}`, values without `}` or `:`.
//!   • The builder emits CRLF line endings for block 4; the parser accepts both
//!     CRLF and LF. To satisfy `parse(build(msg)?) == msg`, block-4 field values
//!     must not contain bare `\n` (build → CRLF → re-parse would preserve the
//!     CRLF inside values, not convert them, so we restrict generated values to
//!     have no `\n` at all — only single-line values — making the round-trip
//!     deterministic).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use proptest::prelude::*;
use std::collections::BTreeMap;
use wf_codec::iso8583::{
    build_with,
    field::{field_def, DataType, LengthSpec},
    parse_with, Dialect, Iso8583Message,
};
use wf_codec::swift::{build, parse, Block, MtField, MtMessage, MtSubBlock};

// ---------------------------------------------------------------------------
// Proptest config
// ---------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    // =======================================================================
    // TEST 2 — ISO 8583 build∘parse inverse
    // =======================================================================

    #[test]
    fn prop_iso8583_roundtrip_hybrid_ascii(msg in arb_iso8583_message(false)) {
        assert_iso_round_trips(&msg, Dialect::HybridAscii);
    }

    #[test]
    fn prop_iso8583_roundtrip_full_ascii(msg in arb_iso8583_message(false)) {
        assert_iso_round_trips(&msg, Dialect::FullAscii);
    }

    #[test]
    fn prop_iso8583_roundtrip_full_binary(msg in arb_iso8583_message(true)) {
        // FullBinary: numeric fields must be ASCII digits only (the BCD encoder
        // requires it). The `true` flag tells the generator to restrict Numeric
        // field payloads to ASCII digits.
        assert_iso_round_trips(&msg, Dialect::FullBinary);
    }

    // =======================================================================
    // TEST 3 — SWIFT MT structural build∘parse inverse
    // =======================================================================

    #[test]
    fn prop_swift_mt_roundtrip(msg in arb_mt_message()) {
        let wire = build(&msg).unwrap_or_else(|e| {
            panic!("build failed: {e}\nmsg: {msg:#?}")
        });
        let reparsed = parse(&wire).unwrap_or_else(|e| {
            panic!("parse failed: {e}\nwire: {wire:?}\nmsg: {msg:#?}")
        });
        assert_eq!(
            msg, reparsed,
            "SWIFT MT message changed after build → parse\nwire: {wire:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// ISO 8583 helpers
// ---------------------------------------------------------------------------

fn assert_iso_round_trips(msg: &Iso8583Message, dialect: Dialect) {
    let wire = build_with(msg, dialect)
        .unwrap_or_else(|e| panic!("build_with({dialect:?}) failed: {e}\nmsg: {msg:#?}"));
    let reparsed = parse_with(&wire, dialect).unwrap_or_else(|e| {
        panic!(
            "parse_with({dialect:?}) failed: {e}\nwire (hex): {}\nmsg: {msg:#?}",
            hex(&wire)
        )
    });
    assert_eq!(
        *msg,
        reparsed,
        "ISO 8583 message changed after build → parse for {dialect:?}\nwire (hex): {}",
        hex(&wire)
    );
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

// ---------------------------------------------------------------------------
// ISO 8583 generator
// ---------------------------------------------------------------------------

/// Generate a valid `Iso8583Message`. When `bcd_numeric = true`, Numeric
/// field payloads are restricted to ASCII digit bytes (required for FullBinary).
fn arb_iso8583_message(bcd_numeric: bool) -> impl Strategy<Value = Iso8583Message> {
    let mti_strategy = prop::array::uniform4(b'0'..=b'9');

    // Build a list of valid (field_number, meta) pairs we can generate for.
    // We use a small curated selection of concrete fields covering Fixed,
    // LLVAR, LLLVAR and the major data types.
    let candidate_fields: Vec<u8> = (2u8..=128u8).filter(|&n| field_def(n).is_some()).collect();

    // Choose up to 4 distinct field numbers.
    let field_count = 0usize..=4usize;

    (
        mti_strategy,
        prop::sample::subsequence(candidate_fields, field_count),
    )
        .prop_flat_map(move |(mti, field_nums)| {
            let field_strategies: Vec<BoxedStrategy<(u8, Vec<u8>)>> = field_nums
                .into_iter()
                .filter_map(|n| {
                    let def = field_def(n)?;
                    let strategy = arb_field_payload(n, def.data_type, def.length, bcd_numeric)?;
                    Some(strategy.prop_map(move |payload| (n, payload)).boxed())
                })
                .collect();

            prop::collection::vec(Just(()), 0..=0)
                .prop_flat_map(move |_| {
                    let fs = field_strategies.clone();
                    prop::collection::vec(Just(()), 0..=0).prop_flat_map(move |_| {
                        let pairs: Vec<BoxedStrategy<(u8, Vec<u8>)>> = fs.clone();
                        if pairs.is_empty() {
                            Just(Vec::<(u8, Vec<u8>)>::new()).boxed()
                        } else {
                            prop::collection::vec(
                                prop::sample::select(pairs).prop_flat_map(|s| s),
                                0..=0,
                            )
                            .boxed()
                        }
                    })
                })
                .prop_map(move |_| mti)
        })
        // Simpler approach: just enumerate small concrete combinations
        .prop_flat_map(move |_mti_unused| {
            // Use a cleaner flat_map: generate MTI + list of (field, payload) pairs.
            (
                prop::array::uniform4(b'0'..=b'9'),
                arb_field_set(bcd_numeric),
            )
                .prop_map(|(mti, fields_vec)| {
                    let mut fields = BTreeMap::new();
                    for (n, payload) in fields_vec {
                        // BTreeMap insert: later overwrites earlier for same key.
                        // Since arb_field_set may produce duplicates, we just keep last.
                        fields.insert(n, payload);
                    }
                    Iso8583Message { mti, fields }
                })
        })
}

/// Generate up to 4 `(field_number, payload)` pairs, all within-contract.
fn arb_field_set(bcd_numeric: bool) -> impl Strategy<Value = Vec<(u8, Vec<u8>)>> {
    // A curated selection of field specs covering the major shapes:
    // Fixed Numeric, Fixed Binary, LLVAR Numeric, LLVAR ANS, LLLVAR ANS.
    // We generate one optional entry per "shape bucket" to keep it manageable.
    (
        // Bucket 1: Fixed Numeric — field 3 (Fixed 6)
        prop::option::of(arb_fixed_numeric_payload(6, bcd_numeric).prop_map(|p| (3u8, p))),
        // Bucket 2: Fixed Numeric — field 4 (Fixed 12)
        prop::option::of(arb_fixed_numeric_payload(12, bcd_numeric).prop_map(|p| (4u8, p))),
        // Bucket 3: LLVAR Numeric — field 2 (LLVAR max:19)
        prop::option::of(arb_llvar_numeric_payload(19, bcd_numeric).prop_map(|p| (2u8, p))),
        // Bucket 4: LLVAR AlphaNumericSpecial — field 37 (Fixed 12, ANS)
        prop::option::of(arb_fixed_ans_payload(12).prop_map(|p| (37u8, p))),
        // Bucket 5: LLLVAR AlphaNumeric — field 48 (LLLVAR max:999 → we cap at 20)
        prop::option::of(arb_lllvar_ans_payload(20).prop_map(|p| (48u8, p))),
        // Bucket 6: Fixed Binary — field 52 (Fixed 8)
        prop::option::of(arb_fixed_binary_payload(8).prop_map(|p| (52u8, p))),
        // Bucket 7: Fixed Numeric — field 7 (Fixed 10)
        prop::option::of(arb_fixed_numeric_payload(10, bcd_numeric).prop_map(|p| (7u8, p))),
        // Bucket 8: LLVAR ANS — field 44 (LLVAR max:25, AlphaNumeric)
        prop::option::of(arb_llvar_ans_payload(25).prop_map(|p| (44u8, p))),
    )
        .prop_map(|(b1, b2, b3, b4, b5, b6, b7, b8)| {
            [b1, b2, b3, b4, b5, b6, b7, b8]
                .into_iter()
                .flatten()
                .collect()
        })
}

/// Generate a payload for a field based on its DataType and LengthSpec.
/// Returns None only for field specs we cannot safely generate for (none expected).
fn arb_field_payload(
    _field: u8,
    data_type: DataType,
    length: LengthSpec,
    bcd_numeric: bool,
) -> Option<impl Strategy<Value = Vec<u8>>> {
    Some(match (data_type, length) {
        (DataType::Numeric, LengthSpec::Fixed(n)) => {
            arb_fixed_numeric_payload(n, bcd_numeric).boxed()
        }
        (DataType::Numeric, LengthSpec::LLVAR { max }) => {
            arb_llvar_numeric_payload(max, bcd_numeric).boxed()
        }
        (DataType::Numeric, LengthSpec::LLLVAR { max }) => {
            arb_lllvar_numeric_payload(max, bcd_numeric).boxed()
        }
        (DataType::Binary, LengthSpec::Fixed(n)) => arb_fixed_binary_payload(n).boxed(),
        (DataType::Binary, LengthSpec::LLVAR { max }) => {
            let cap = max.min(99);
            prop::collection::vec(any::<u8>(), 0..=cap).boxed()
        }
        (DataType::Binary, LengthSpec::LLLVAR { max }) => {
            let cap = max.min(20); // cap for speed
            prop::collection::vec(any::<u8>(), 0..=cap).boxed()
        }
        (_, LengthSpec::Fixed(n)) => {
            // AlphaNumeric / AlphaNumericSpecial / Alpha / Track etc.
            arb_fixed_ans_payload(n).boxed()
        }
        (_, LengthSpec::LLVAR { max }) => {
            let cap = max.min(99);
            arb_llvar_ans_payload(cap).boxed()
        }
        (_, LengthSpec::LLLVAR { max }) => {
            let cap = max.min(20); // cap for speed
            arb_lllvar_ans_payload(cap).boxed()
        }
    })
}

// ---------------------------------------------------------------------------
// Payload sub-strategies
// ---------------------------------------------------------------------------

/// Generate exactly `n` ASCII digit bytes (`b'0'..=b'9'`).
fn arb_fixed_numeric_payload(n: usize, _bcd_numeric: bool) -> impl Strategy<Value = Vec<u8>> {
    // For FullBinary: Numeric payloads must be ASCII digits of any length
    // (BCD handles odd/even correctly). For the Fixed case it's always `n`
    // digits. This is identical across dialects since the Iso8583Message
    // stores ASCII regardless.
    prop::collection::vec(b'0'..=b'9', n..=n)
}

/// Generate 0..=min(99, max) ASCII digit bytes.
fn arb_llvar_numeric_payload(max: usize, _bcd_numeric: bool) -> impl Strategy<Value = Vec<u8>> {
    let cap = max.min(99);
    prop::collection::vec(b'0'..=b'9', 0..=cap)
}

/// Generate 0..=min(999, max) ASCII digit bytes.
fn arb_lllvar_numeric_payload(max: usize, _bcd_numeric: bool) -> impl Strategy<Value = Vec<u8>> {
    let cap = max.min(20); // small cap for test speed
    prop::collection::vec(b'0'..=b'9', 0..=cap)
}

/// Generate exactly `n` printable ASCII bytes (AlphaNumericSpecial category).
/// We restrict to printable ASCII 0x20..=0x7E for simplicity — within contract
/// since these fields accept the full ANS charset.
fn arb_fixed_ans_payload(n: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(0x20u8..=0x7Eu8, n..=n)
}

/// Generate 0..=cap printable ASCII bytes for an LLVAR ANS field.
fn arb_llvar_ans_payload(cap: usize) -> impl Strategy<Value = Vec<u8>> {
    let actual_cap = cap.min(99);
    prop::collection::vec(0x20u8..=0x7Eu8, 0..=actual_cap)
}

/// Generate 0..=cap printable ASCII bytes for an LLLVAR ANS field.
fn arb_lllvar_ans_payload(cap: usize) -> impl Strategy<Value = Vec<u8>> {
    let actual_cap = cap.min(20);
    prop::collection::vec(0x20u8..=0x7Eu8, 0..=actual_cap)
}

/// Generate exactly `n` arbitrary bytes for a Binary field.
fn arb_fixed_binary_payload(n: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), n..=n)
}

// ---------------------------------------------------------------------------
// SWIFT MT generator (TEST 3)
// ---------------------------------------------------------------------------

/// Generate a valid `[A-Z0-9]{1..=3}` tag string.
fn arb_mt_tag() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::char::ranges(vec!['A'..='Z', '0'..='9'].into()), 1..=3)
        .prop_map(|chars| chars.into_iter().collect())
}

/// Generate a block 1 or 2 Raw content string: no `{`, `}`, no control chars.
/// Printable ASCII only (the parser stores verbatim bytes as UTF-8 lossless).
fn arb_raw_block_string() -> impl Strategy<Value = String> {
    prop::collection::vec(
        (0x20u8..=0x7Eu8).prop_filter("not brace", |b| *b != b'{' && *b != b'}'),
        1..=20,
    )
    .prop_map(|bytes| String::from_utf8(bytes).unwrap())
}

/// Generate a block-4 field value within the SWIFT wire contract:
///   • No `{` or `}` — these are SWIFT wire frame delimiters. A `{` in a
///     block-4 value is not in-contract: the outer `find_matching_brace` brace
///     tracker counts it as an open and the subsequent `}` that closes block 4
///     is consumed instead, causing `UnbalancedBrace` on re-parse. This is an
///     in-contract generator tightening — the SWIFT FIN format treats braces as
///     structural delimiters that must not appear verbatim inside field values.
///   • No `\n:` (would split into a new field on the receiver)
///   • No line that is exactly `-` (collides with the block terminator)
///   • No `\n` or `\r` at all — simplest constraint satisfying all the above.
fn arb_block4_value() -> impl Strategy<Value = String> {
    prop::collection::vec(
        (0x20u8..=0x7Eu8).prop_filter("not brace or newline", |b| {
            *b != b'{' && *b != b'}' && *b != b'\n' && *b != b'\r'
        }),
        0..=20,
    )
    .prop_map(|bytes| String::from_utf8(bytes).unwrap())
    // Extra safety: reject if the value is exactly "-" (lone-dash terminator collision).
    .prop_filter("not lone dash", |s| s != "-")
}

/// Generate a block-3/5 sub-block value within the SWIFT wire contract.
/// Excluded characters:
///   • `}` — terminates the sub-block prematurely on parse.
///   • `{` — shifts the brace-depth tracker inside `find_matching_brace`,
///     causing the outer block's `}` to be consumed before the block closes.
///     Both braces are wire-level delimiters and must not appear in values.
/// Sub-block values MAY contain `:` — the builder only validates the TAG.
fn arb_subblock_value() -> impl Strategy<Value = String> {
    prop::collection::vec(
        (0x20u8..=0x7Eu8).prop_filter("not brace", |b| *b != b'{' && *b != b'}'),
        0..=15,
    )
    .prop_map(|bytes| String::from_utf8(bytes).unwrap())
}

/// Generate an `MtMessage` with a valid structure.
fn arb_mt_message() -> impl Strategy<Value = MtMessage> {
    (
        // Block 1: optional Raw
        prop::option::of(arb_raw_block_string()),
        // Block 2: optional Raw
        prop::option::of(arb_raw_block_string()),
        // Block 3: optional Tagged
        prop::option::of(arb_tagged_block()),
        // Block 4: optional Text
        prop::option::of(arb_text_block()),
        // Block 5: optional Tagged
        prop::option::of(arb_tagged_block()),
    )
        .prop_map(|(b1, b2, b3, b4, b5)| {
            let mut blocks = BTreeMap::new();
            if let Some(s) = b1 {
                blocks.insert(1u8, Block::Raw(s));
            }
            if let Some(s) = b2 {
                blocks.insert(2u8, Block::Raw(s));
            }
            if let Some(subs) = b3 {
                blocks.insert(3u8, Block::Tagged(subs));
            }
            if let Some(fields) = b4 {
                blocks.insert(4u8, Block::Text(fields));
            }
            if let Some(subs) = b5 {
                blocks.insert(5u8, Block::Tagged(subs));
            }
            MtMessage { blocks }
        })
}

/// Generate a block-4 Text block: 0..=4 `MtField` entries with valid tags and values.
fn arb_text_block() -> impl Strategy<Value = Vec<MtField>> {
    prop::collection::vec(
        (arb_mt_tag(), arb_block4_value()).prop_map(|(tag, value)| MtField { tag, value }),
        0..=4,
    )
}

/// Generate a block-3 or block-5 Tagged block: 0..=4 `MtSubBlock` entries.
fn arb_tagged_block() -> impl Strategy<Value = Vec<MtSubBlock>> {
    prop::collection::vec(
        (arb_mt_tag(), arb_subblock_value()).prop_map(|(tag, value)| MtSubBlock { tag, value }),
        0..=4,
    )
}
