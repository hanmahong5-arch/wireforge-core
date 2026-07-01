//! End-to-end conformance-engine tests over **SYNTHETIC** ISO 8583 fixtures.
//!
//! Fixtures are built with `wf_codec::iso8583::build` from hand-specified
//! messages, and every expectation is **computed by hand from (mask rule,
//! bytes, expect)** — never read back from the engine. This mirrors the
//! anti-tautology stance of the inline `mask_to_verdict_table_is_pinned`
//! guard: the guard pins the table, these tests prove the table is what the
//! end-to-end engine actually applies.
//!
//! No real captures exist yet — these messages are invented.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use wf_codec::iso8583::{build, Iso8583Message};
use wf_oracle::{
    check_conformance, check_conformance_views, ConformanceGate, FieldKey, FieldMask, FieldVerdict,
    MaskType, OracleReport, OracleRow, OracleSpec, UnexplainedReason, WireMessage,
};

/// Build synthetic ISO 8583 wire bytes from an MTI and `(field, value)` pairs.
/// Field lengths are chosen to satisfy the built-in table so `build` succeeds.
fn wire(mti: &[u8; 4], fields: &[(u8, &[u8])]) -> Vec<u8> {
    let mut map: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for (n, v) in fields {
        map.insert(*n, v.to_vec());
    }
    build(&Iso8583Message {
        mti: *mti,
        fields: map,
    })
    .expect("synthetic message must build")
}

/// A minimal valid request (only ever parsed for validity, never diffed).
fn req() -> Vec<u8> {
    wire(b"0200", &[(4, b"000000010000")])
}

fn report_of(legacy: &[u8], migrated: &[u8], spec: &OracleSpec) -> OracleReport {
    check_conformance(&req(), legacy, migrated, spec).expect("checkable capture")
}

fn row(report: &OracleReport, field: u8) -> &OracleRow {
    report
        .rows
        .iter()
        .find(|r| r.key == FieldKey::Iso8583(field))
        .unwrap_or_else(|| panic!("expected a row for field {field}"))
}

// ---------------------------------------------------------------------------
// One test per verdict class (expectations hand-computed)
// ---------------------------------------------------------------------------

#[test]
fn stable_equal_is_equal_and_conformant() {
    // Identical responses, all-stable default → every field Equal, no drift.
    let legacy = wire(b"0210", &[(4, b"000000010000"), (39, b"00")]);
    let migrated = legacy.clone();
    let report = report_of(&legacy, &migrated, &OracleSpec::new("iso8583"));
    assert_eq!(row(&report, 4).verdict, FieldVerdict::Equal);
    assert_eq!(row(&report, 39).verdict, FieldVerdict::Equal);
    assert_eq!(report.gate, ConformanceGate::Conformant);
}

#[test]
fn stable_value_diff_is_unexplained_value_diff_and_drift() {
    // DE39 "00" vs "05": same length (2), different bytes → ValueDiff.
    let legacy = wire(b"0210", &[(39, b"00")]);
    let migrated = wire(b"0210", &[(39, b"05")]);
    let report = report_of(&legacy, &migrated, &OracleSpec::new("iso8583"));
    assert_eq!(
        row(&report, 39).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::ValueDiff
        }
    );
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

#[test]
fn stable_length_mismatch_is_length_diff_not_equal() {
    // THE min_len GUARD. DE48 (LLLVAR) carries 10 bytes on legacy and 12 on
    // migrated; the first 10 bytes are identical. A min-length comparison
    // would slice both to 10 and wrongly call this Equal. The engine compares
    // FULL slices and reports a dedicated LengthDiff(10, 12) — never Equal.
    let legacy = wire(b"0210", &[(48, b"ABCDEFGHIJ")]); // 10 bytes
    let migrated = wire(b"0210", &[(48, b"ABCDEFGHIJKL")]); // 12 bytes, shares the 10-byte prefix
    let report = report_of(&legacy, &migrated, &OracleSpec::new("iso8583"));
    assert_eq!(
        row(&report, 48).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::LengthDiff {
                legacy: 10,
                migrated: 12
            }
        },
        "shared-prefix but different length must be LengthDiff, not Equal"
    );
    assert_ne!(row(&report, 48).verdict, FieldVerdict::Equal);
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

#[test]
fn volatile_diff_is_normalized_not_drift() {
    // DE11 (STAN) differs run-to-run; masked Volatile → normalised, not drift.
    let legacy = wire(b"0210", &[(11, b"111111")]);
    let migrated = wire(b"0210", &[(11, b"222222")]);
    let spec = OracleSpec::new("iso8583").with_mask(FieldMask::volatile(FieldKey::Iso8583(11)));
    let report = report_of(&legacy, &migrated, &spec);
    assert_eq!(row(&report, 11).verdict, FieldVerdict::VolatileNormalized);
    assert_eq!(
        report.gate,
        ConformanceGate::Conformant,
        "a differing volatile field is not drift"
    );
}

#[test]
fn crypto_diff_is_excluded_not_drift() {
    // DE52 (PIN data) is re-derived; masked Crypto → excluded from value diff.
    let legacy = wire(b"0210", &[(52, b"PINBLOK1")]);
    let migrated = wire(b"0210", &[(52, b"PINBLOK2")]);
    let spec = OracleSpec::new("iso8583").with_mask(FieldMask::crypto(FieldKey::Iso8583(52)));
    let report = report_of(&legacy, &migrated, &spec);
    assert_eq!(row(&report, 52).verdict, FieldVerdict::CryptoExcluded);
    assert_eq!(report.gate, ConformanceGate::Conformant);
}

#[test]
fn intended_delta_match_is_conformant() {
    // DE39 intentionally changes 00 → 05; spec expect = "05" → IntendedDelta.
    let legacy = wire(b"0210", &[(39, b"00")]);
    let migrated = wire(b"0210", &[(39, b"05")]);
    let spec = OracleSpec::new("iso8583").with_mask(FieldMask::intended_delta(
        FieldKey::Iso8583(39),
        b"05".to_vec(),
    ));
    let report = report_of(&legacy, &migrated, &spec);
    assert_eq!(row(&report, 39).verdict, FieldVerdict::IntendedDelta);
    assert_eq!(report.gate, ConformanceGate::Conformant);
}

#[test]
fn intended_delta_unmet_is_drift() {
    // Migrated produced "07" but the operator approved "05" → unmet drift,
    // checked against the spec `expect`, not the legacy bytes.
    let legacy = wire(b"0210", &[(39, b"00")]);
    let migrated = wire(b"0210", &[(39, b"07")]);
    let spec = OracleSpec::new("iso8583").with_mask(FieldMask::intended_delta(
        FieldKey::Iso8583(39),
        b"05".to_vec(),
    ));
    let report = report_of(&legacy, &migrated, &spec);
    assert_eq!(
        row(&report, 39).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::IntendedDeltaUnmet {
                expected: b"05".to_vec(),
                got: Some(b"07".to_vec())
            }
        }
    );
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

#[test]
fn both_absent_is_non_counting_pass() {
    // DE60 named in the spec but present on neither side. Stable both-absent
    // is a pass (Equal) and, because it is absent on legacy, NOT counted in
    // the coverage denominator.
    let legacy = wire(b"0210", &[(4, b"000000010000")]);
    let migrated = legacy.clone();
    let spec = OracleSpec::new("iso8583").with_mask(FieldMask::stable(FieldKey::Iso8583(60)));
    let report = report_of(&legacy, &migrated, &spec);
    assert_eq!(row(&report, 60).verdict, FieldVerdict::Equal);
    assert_eq!(report.gate, ConformanceGate::Conformant);
    // Coverage = MTI + DE4 (both stable + present on legacy). DE60 absent on
    // legacy → excluded.
    assert_eq!(report.coverage.total, 2, "DE60 must not be counted");
}

#[test]
fn present_one_side_stable_is_presence_mismatch_drift() {
    // DE41 present on legacy, absent on migrated, stable → presence mismatch.
    let legacy = wire(b"0210", &[(41, b"TERM0001")]);
    let migrated = wire(b"0210", &[]);
    let report = report_of(&legacy, &migrated, &OracleSpec::new("iso8583"));
    assert_eq!(
        row(&report, 41).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::PresenceMismatch {
                legacy_present: true,
                migrated_present: false
            }
        }
    );
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

#[test]
fn uncheckable_input_is_err() {
    // Garbage migrated bytes cannot parse → Err (the CLI maps this to exit 2).
    let legacy = wire(b"0210", &[(4, b"000000010000")]);
    let result = check_conformance(&req(), &legacy, &[0x00], &OracleSpec::new("iso8583"));
    assert!(result.is_err(), "unparseable migrated response must Err");
}

#[test]
fn malformed_spec_is_err() {
    // IntendedDelta without `expect` is malformed → Err (→ exit 2), never a
    // silent mis-application.
    let legacy = wire(b"0210", &[(39, b"00")]);
    let spec = OracleSpec::new("iso8583").with_mask(FieldMask {
        key: FieldKey::Iso8583(39),
        mask: MaskType::IntendedDelta,
        expect: None,
    });
    let result = check_conformance(&req(), &legacy, &legacy, &spec);
    assert!(result.is_err(), "intended-delta without expect must Err");
}

#[test]
fn coverage_denominator_excludes_volatile_and_crypto() {
    // Legacy carries DE4 (stable), DE39 (stable), DE11 (volatile), DE52
    // (crypto). The value-bearing baseline is MTI + DE4 + DE39 = 3; DE11/DE52
    // are excluded so coverage cannot be inflated by masking value away.
    let legacy = wire(
        b"0210",
        &[
            (4, b"000000010000"),
            (11, b"111111"),
            (39, b"00"),
            (52, b"PINBLOK1"),
        ],
    );
    // Migrated differs ONLY on the volatile/crypto fields — all baseline
    // fields match.
    let migrated = wire(
        b"0210",
        &[
            (4, b"000000010000"),
            (11, b"999999"),
            (39, b"00"),
            (52, b"PINBLOK9"),
        ],
    );
    let spec = OracleSpec::new("iso8583")
        .with_mask(FieldMask::volatile(FieldKey::Iso8583(11)))
        .with_mask(FieldMask::crypto(FieldKey::Iso8583(52)));
    let report = report_of(&legacy, &migrated, &spec);
    assert_eq!(report.coverage.total, 3, "MTI + DE4 + DE39 only");
    assert_eq!(report.coverage.checked, 3);
    assert_eq!(report.coverage.pct(), 100);
    assert_eq!(report.gate, ConformanceGate::Conformant);
}

#[test]
fn default_mask_stable_fails_closed() {
    // DE60 differs and is NOT named in the spec → resolves to the Stable
    // default and surfaces as drift rather than silently passing.
    let legacy = wire(b"0210", &[(60, b"AAA")]);
    let migrated = wire(b"0210", &[(60, b"BBB")]);
    let report = report_of(&legacy, &migrated, &OracleSpec::new("iso8583"));
    assert_eq!(
        row(&report, 60).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::ValueDiff
        }
    );
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

// ---------------------------------------------------------------------------
// Multi-occurrence (exercised via a synthetic WireMessage stub — ISO is 0/1)
// ---------------------------------------------------------------------------

/// A minimal [`WireMessage`] whose fields can carry arbitrary occurrence
/// counts, so the multi-occurrence engine branches are exercised even though
/// real ISO 8583 fields are 0-or-1.
#[derive(Debug)]
struct StubMsg {
    occ: BTreeMap<u8, Vec<Vec<u8>>>,
}

impl StubMsg {
    fn new(entries: &[(u8, &[&[u8]])]) -> Self {
        let mut occ: BTreeMap<u8, Vec<Vec<u8>>> = BTreeMap::new();
        for (n, occs) in entries {
            occ.insert(*n, occs.iter().map(|o| o.to_vec()).collect());
        }
        StubMsg { occ }
    }
}

impl WireMessage for StubMsg {
    fn field_keys(&self) -> Vec<FieldKey> {
        self.occ.keys().map(|&n| FieldKey::Iso8583(n)).collect()
    }
    fn field_occurrences(&self, key: FieldKey) -> &[Vec<u8>] {
        match key {
            FieldKey::Iso8583(n) => match self.occ.get(&n) {
                Some(v) => v.as_slice(),
                None => &[],
            },
            _ => &[],
        }
    }
    fn field_label(&self, key: FieldKey) -> String {
        format!("F{}", key.number())
    }
}

#[test]
fn multi_occurrence_count_diff_is_drift() {
    // DE60: two occurrences on legacy, one on migrated → OccurrenceCountDiff.
    let legacy = StubMsg::new(&[(60, &[b"A", b"B"])]);
    let migrated = StubMsg::new(&[(60, &[b"A"])]);
    let report = check_conformance_views(&legacy, &migrated, &OracleSpec::new("iso8583"))
        .expect("checkable");
    assert_eq!(
        row(&report, 60).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::OccurrenceCountDiff {
                legacy: 2,
                migrated: 1
            }
        }
    );
    assert_eq!(report.gate, ConformanceGate::FoundDrift);
}

#[test]
fn multi_occurrence_equal_counts_worst_verdict_wins() {
    // DE60: two occurrences each. Occ 0 equal, occ 1 differs → the worst
    // (ValueDiff) wins for the row.
    let legacy = StubMsg::new(&[(60, &[b"A", b"B"])]);
    let migrated = StubMsg::new(&[(60, &[b"A", b"C"])]);
    let report = check_conformance_views(&legacy, &migrated, &OracleSpec::new("iso8583"))
        .expect("checkable");
    assert_eq!(
        row(&report, 60).verdict,
        FieldVerdict::Unexplained {
            reason: UnexplainedReason::ValueDiff
        }
    );
}

// ---------------------------------------------------------------------------
// Rendering: honest framing, determinism, golden snapshot
// ---------------------------------------------------------------------------

#[test]
fn render_never_claims_proof() {
    let legacy = wire(b"0210", &[(4, b"000000010000")]);
    let out = report_of(&legacy, &legacy, &OracleSpec::new("iso8583")).render();
    assert!(out.contains("Wireforge Conformance EVIDENCE"));
    // The honesty words appear ONLY inside the negative disclaimer.
    assert!(out.contains("NOT a proof"));
    assert!(out.contains("certification"));
    assert!(out.contains("equivalence claim"));
    assert!(
        !out.contains("proves"),
        "must not affirmatively claim proof"
    );
    assert!(!out.contains("certifies"), "must not claim certification");
    assert!(!out.contains("is equivalent"), "must not claim equivalence");
    // Every "proof" occurrence is part of "NOT a proof".
    assert_eq!(
        out.matches("proof").count(),
        out.matches("NOT a proof").count()
    );
}

#[test]
fn render_is_deterministic() {
    // Two independent runs over identical inputs render byte-identically.
    let legacy = wire(b"0210", &[(4, b"000000010000"), (39, b"00")]);
    let migrated = wire(b"0210", &[(39, b"05")]);
    let spec = OracleSpec::new("iso8583");
    let a = report_of(&legacy, &migrated, &spec).render();
    let b = report_of(&legacy, &migrated, &spec).render();
    assert_eq!(a, b);
}

#[test]
fn render_golden_snapshot_mti_only() {
    // The smallest complete artifact: an MTI-only, all-stable, equal capture.
    // Key universe = {MTI}; MTI is stable + present → coverage 1/1 = 100%.
    let msg = wire(b"0210", &[]);
    let out = report_of(&msg, &msg, &OracleSpec::new("iso8583")).render();
    let golden = "\
Wireforge Conformance EVIDENCE
  interface: iso8583
  mode: Mode-A replay (captured legacy response vs migrated response)
  fixtures: SYNTHETIC
  note: regression-conformance EVIDENCE only — NOT a proof, NOT a
        certification, NOT an equivalence claim. A conformant gate means
        every value-bearing baseline field matched its operator-approved
        mask rule under this capture — nothing more.
  coverage: 100% (1/1 value-bearing baseline fields accounted for)
  gate: conformant (exit 0)
Fields:
  └── [  0] MTI [stable]: equal
";
    assert_eq!(out, golden);
}
