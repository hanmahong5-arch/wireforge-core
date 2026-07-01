//! End-to-end tests for the fixed-length record path: a recovered
//! [`FixedLayout`] parses two captured frames and the masked-diff engine
//! compares them — the same EVIDENCE machinery as ISO 8583, keyed by field
//! ordinal instead of data-element number.
//!
//! Frames are **SYNTHETIC**, shaped like the classic
//! `len(4) + code(2) + fixed body fields` host responses; every expectation
//! is hand-computed from (mask rule, bytes), never read back from the engine.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_oracle::{
    check_conformance_views, ConformanceGate, FieldKey, FieldMask, FieldVerdict, FixedField,
    FixedLayout, OracleSpec, UnexplainedReason,
};

/// A hand-specified response layout: 4+2+20+12 = 38 bytes.
fn resp_layout() -> FixedLayout {
    FixedLayout::new(
        "demo response",
        [
            FixedField::bytes("msg_len", 4),
            FixedField::bytes("ret_code", 2),
            FixedField::bytes("account", 20),
            FixedField::bytes("amount", 12),
        ],
    )
    .expect("valid layout")
}

/// 38 bytes exactly: "0038" + code + 20-byte account + 12-byte amount.
fn frame(code: &[u8; 2], amount12: &[u8; 12]) -> Vec<u8> {
    let mut f = Vec::with_capacity(38);
    f.extend_from_slice(b"0038");
    f.extend_from_slice(code);
    f.extend_from_slice(b"13681279407         ");
    f.extend_from_slice(amount12);
    assert_eq!(f.len(), 38, "frame must tile the 38-byte layout");
    f
}

#[test]
fn identical_fixed_frames_are_conformant() {
    let layout = resp_layout();
    let legacy = layout
        .parse(&frame(b"00", b"123.45      "))
        .expect("legacy");
    let migrated = layout
        .parse(&frame(b"00", b"123.45      "))
        .expect("migrated");
    let report = check_conformance_views(&legacy, &migrated, &OracleSpec::new("fixed-tcp"))
        .expect("checkable");
    assert_eq!(report.gate, ConformanceGate::Conformant);
    // All 4 fields are stable + present on legacy → coverage 4/4.
    assert_eq!(report.coverage.total, 4);
    assert_eq!(report.coverage.checked, 4);
}

#[test]
fn flipped_amount_field_is_ordinal_drift() {
    // Field 3 (amount) differs by value at equal length → ValueDiff on
    // FieldKey::Ordinal(3), and the row label carries the layout's name.
    let layout = resp_layout();
    let legacy = layout
        .parse(&frame(b"00", b"123.45      "))
        .expect("legacy");
    let migrated = layout
        .parse(&frame(b"00", b"999.99      "))
        .expect("migrated");
    let report = check_conformance_views(&legacy, &migrated, &OracleSpec::new("fixed-tcp"))
        .expect("checkable");
    let amount_row = report
        .rows
        .iter()
        .find(|r| r.key == FieldKey::Ordinal(3))
        .expect("amount row");
    assert_eq!(amount_row.label, "amount");
    assert_eq!(
        amount_row.verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::ValueDiff
        }
    );
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

#[test]
fn volatile_mask_applies_to_ordinal_key() {
    // Mask field 3 volatile → the same flipped amount is normalised away.
    let layout = resp_layout();
    let legacy = layout
        .parse(&frame(b"00", b"123.45      "))
        .expect("legacy");
    let migrated = layout
        .parse(&frame(b"00", b"999.99      "))
        .expect("migrated");
    let spec = OracleSpec::new("fixed-tcp").with_mask(FieldMask::volatile(FieldKey::Ordinal(3)));
    let report = check_conformance_views(&legacy, &migrated, &spec).expect("checkable");
    assert_eq!(report.gate, ConformanceGate::Conformant);
    // Volatile field leaves the coverage denominator: 3 value-bearing left.
    assert_eq!(report.coverage.total, 3);
}

#[test]
fn render_shows_layout_field_names() {
    let layout = resp_layout();
    let legacy = layout
        .parse(&frame(b"00", b"123.45      "))
        .expect("legacy");
    let migrated = layout
        .parse(&frame(b"05", b"123.45      "))
        .expect("migrated");
    let report = check_conformance_views(&legacy, &migrated, &OracleSpec::new("fixed-tcp"))
        .expect("checkable");
    let out = report.render();
    assert!(
        out.contains("ret_code"),
        "rows must carry field names: {out}"
    );
    assert!(
        out.contains("UNEXPLAINED"),
        "flipped ret_code is drift: {out}"
    );
    assert!(out.contains("NOT a proof"), "honesty note on every surface");
}
