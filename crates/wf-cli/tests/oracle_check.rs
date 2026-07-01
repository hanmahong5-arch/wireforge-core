//! Integration tests for the `wf oracle check` entry points.
//!
//! These call the pure lib.rs entry points directly (the binary is a thin
//! file/stdin dispatcher over them). ISO 8583 fixtures are produced through
//! the crate's own public `build_from_json` + `hex_to_bytes`. Expectations are
//! computed by hand from the mask spec and bytes — never read back from the
//! engine. All fixtures are SYNTHETIC.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_cli::{
    build_from_json, hex_to_bytes, oracle_check, oracle_report, oracle_report_from_wf,
    render_oracle_scan, OracleEntry,
};
use wf_oracle::{FieldKey, FieldVerdict, OracleReport};

/// Build ISO 8583 wire bytes from a JSON message description via the crate's
/// own `build`/hex entry points.
fn iso(json: &str) -> Vec<u8> {
    hex_to_bytes(&build_from_json(json).expect("build json")).expect("decode hex")
}

fn req() -> Vec<u8> {
    iso(r#"{"mti":"0200","fields":{"4":"000000010000"}}"#)
}

/// The gate code implied by a single report-or-error result, via the same
/// one-entry batch fold the binary uses.
fn gate_code(result: Result<OracleReport, String>) -> u8 {
    let (_body, gate) = render_oracle_scan(&[OracleEntry {
        label: "t".to_string(),
        result,
    }]);
    gate.code()
}

#[test]
fn four_flag_conformant_is_exit_zero() {
    let legacy = iso(r#"{"mti":"0210","fields":{"4":"000000010000","39":"00"}}"#);
    let migrated = legacy.clone();
    let result = oracle_report(&req(), &legacy, &migrated, "interface = \"iso8583\"\n");
    assert_eq!(gate_code(result), 0);
}

#[test]
fn four_flag_stable_byte_flip_is_exit_one() {
    // Flip DE39 on the migrated side; default-stable → UNEXPLAINED drift.
    let legacy = iso(r#"{"mti":"0210","fields":{"4":"000000010000","39":"00"}}"#);
    let migrated = iso(r#"{"mti":"0210","fields":{"4":"000000010000","39":"01"}}"#);
    let result = oracle_report(&req(), &legacy, &migrated, "interface = \"iso8583\"\n");
    let (body, gate) = render_oracle_scan(&[OracleEntry {
        label: "t".to_string(),
        result,
    }]);
    assert_eq!(gate.code(), 1);
    assert!(
        body.contains("UNEXPLAINED"),
        "a flipped stable byte must render an UNEXPLAINED row, got:\n{body}"
    );
}

#[test]
fn uncheckable_migrated_is_exit_two_not_one() {
    // Garbage migrated bytes cannot parse → Err → gate 2 (NOT folded to 1).
    let legacy = iso(r#"{"mti":"0210","fields":{"4":"000000010000"}}"#);
    let result = oracle_report(&req(), &legacy, &[0x00], "interface = \"iso8583\"\n");
    assert!(result.is_err());
    assert_eq!(
        gate_code(result),
        2,
        "uncheckable input must be exit 2, not 1"
    );
}

#[test]
fn toml_spec_with_masks_parses_and_applies() {
    // Volatile DE11 differs (not drift); intended-delta DE39 00→05 is met.
    let legacy = iso(r#"{"mti":"0210","fields":{"11":"100001","39":"00"}}"#);
    let migrated = iso(r#"{"mti":"0210","fields":{"11":"999999","39":"05"}}"#);
    let spec = r#"
        interface = "iso8583"
        default_mask = "stable"

        [[mask]]
        field = 11
        mask = "volatile"

        [[mask]]
        field = 39
        mask = "intended-delta"
        expect = "05"
    "#;
    let result = oracle_report(&req(), &legacy, &migrated, spec);
    assert_eq!(gate_code(result), 0);
}

#[test]
fn intended_delta_unmet_via_toml_is_exit_one() {
    // Same spec, but migrated DE39 is "07" not the approved "05" → drift.
    let legacy = iso(r#"{"mti":"0210","fields":{"39":"00"}}"#);
    let migrated = iso(r#"{"mti":"0210","fields":{"39":"07"}}"#);
    let spec =
        "interface = \"iso8583\"\n[[mask]]\nfield = 39\nmask = \"intended-delta\"\nexpect = \"05\"\n";
    let result = oracle_report(&req(), &legacy, &migrated, spec);
    assert_eq!(gate_code(result), 1);
}

#[test]
fn malformed_spec_is_exit_two() {
    // intended-delta without `expect` is malformed → Err → gate 2.
    let legacy = iso(r#"{"mti":"0210","fields":{"39":"00"}}"#);
    let spec = "[[mask]]\nfield = 39\nmask = \"intended-delta\"\n";
    let result = oracle_report(&req(), &legacy, &legacy, spec);
    assert!(result.is_err());
    assert_eq!(gate_code(result), 2);
}

#[test]
fn output_frames_as_evidence_not_proof() {
    let legacy = iso(r#"{"mti":"0210","fields":{"4":"000000010000"}}"#);
    let out = oracle_check(&req(), &legacy, &legacy, "interface = \"iso8583\"\n").unwrap();
    assert!(out.contains("Wireforge Conformance EVIDENCE"));
    assert!(out.contains("NOT a proof"));
    assert!(!out.contains("proves"));
    assert!(!out.contains("certifies"));
}

#[test]
fn wf_triple_path_is_conformant_with_intended_delta() {
    // Hand-built `.wf` triple: stable equal MTI/DE4/DE39, volatile DE11,
    // crypto DE52, intended-delta DE63 (V1→V2, met).
    let wf_src = "\
meta {
  name: triple
  type: oracle
}
iso8583 {
  role: req
  mti: 0200
  field 4: 000000010000
}
iso8583 {
  role: legacy
  mti: 0210
  field 4: 000000010000
  field 11: 100001
  field 39: 00
  field 52: MAC1RESP
  field 63: V1
}
iso8583 {
  role: migrated
  mti: 0210
  field 4: 000000010000
  field 11: 999999
  field 39: 00
  field 52: MAC2RESP
  field 63: V2
}
oracle-spec {
  interface: iso8583
  default: stable
  field 11: volatile
  field 52: crypto
  field 63: intended-delta V2
}
";
    let report = oracle_report_from_wf(wf_src).expect("checkable .wf triple");
    // Baseline = MTI + DE4 + DE39 + DE63 (stable/intended-delta, present on
    // legacy); DE11 (volatile) + DE52 (crypto) excluded.
    assert_eq!(report.coverage.total, 4);
    assert_eq!(report.coverage.checked, 4);
    assert_eq!(report.coverage.pct(), 100);
    let de63 = report
        .rows
        .iter()
        .find(|r| r.key == FieldKey::Iso8583(63))
        .expect("DE63 row");
    assert_eq!(de63.verdict, FieldVerdict::IntendedDelta);
}

#[test]
fn example_oracle_file_is_conformant() {
    // The committed example demonstrates a conformant migration end-to-end.
    let wf_src = include_str!("../../wf-format/examples/iso8583-oracle.wf");
    let report = oracle_report_from_wf(wf_src).expect("example .wf must check");
    assert_eq!(report.gate.code(), 0, "example must be conformant (exit 0)");
    assert_eq!(report.coverage.pct(), 100);
}
