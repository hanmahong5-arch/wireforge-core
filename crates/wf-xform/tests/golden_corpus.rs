//! SYNTHETIC corpus — NOT production data.
//!
//! A golden corpus of hand-built MT103 <-> pacs.008.001.08 pairs that, across
//! the five roles the detector understands, exercises EACH of the seven
//! [`FieldDiff`] variants at least once. Every message below is SYNTHETIC: it
//! is a concrete fill of a *documented* message shape, not a captured real
//! sample. No claim of real-sample validation is made.
//!
//! ## Anti-tautology grounding
//!
//! The truncation expectations are NOT read back from the detector's own
//! classifier. They are derived from the CITED upstream maximum lengths,
//! declared here as a local literal independent of `Role::*::mt_max_len()`:
//!
//! - MT 50K (debtor) / 59 (creditor) / 70 (remittance) capacity =
//!   4 lines * 35 chars = 140 chars (SWIFT MT103 format spec; mirrored by
//!   `wf-codec` `field_50k.rs`).
//! - MX `Dbtr/Nm`, `Cdtr/Nm`, `RmtInf/Ustrd` maxLength = 140 (mx-message
//!   3.1.4 pacs.008.001.08 validators: `PartyIdentification1352` /
//!   `PartyIdentification1353` `validate_length("Nm", Some(140))`;
//!   `RemittanceInformation161` `validate_length("Ustrd", Some(140))`).
//!
//! So a 141-char MX name CANNOT fit the 140-char MT field -> the detector
//! must report `Truncated` with exactly the 141st char lost; a 140-char name
//! fits. A guard test pins `Role::*::{mt,mx}_max_len()` against the same
//! literal so the detector and this corpus stay in lockstep — if an upstream
//! facet moves, the guard fails loudly instead of hiding the drift.
//!
//! The MESSAGE SHAPES are copied from the two documented builders the crate
//! already uses (`tests/maxlen_pin.rs` and the in-crate `tests` module), so
//! every case parses through the real `wf_swift::parse` / `wf_mx::from_xml`
//! paths rather than an invented format.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_xform::{diff_mt_mx, DiffReport, FieldDiff, Role};

/// The cited 140-char name/remittance cap this corpus is grounded on.
///
/// Declared as a literal here (NOT read from the detector) so the truncation
/// case is an INDEPENDENT anchor: the expectation comes from the cited SWIFT
/// MT103 50K format and the mx-message `Nm` maxLength facet, not from the
/// classifier output we are checking.
const CITED_NAME_CAP: usize = 140;

/// SYNTHETIC: build a full pacs.008.001.08 envelope with the given debtor
/// name, creditor name, and optional unstructured remittance (`RmtInf/Ustrd`).
///
/// Structure is the documented `mx-message` pacs.008 shape, identical to the
/// envelope the in-crate `wf-xform` tests use, so this exercises a genuinely
/// parseable message rather than an invented one. Only the role values vary.
fn pacs008_envelope(dbtr_nm: &str, cdtr_nm: &str, ustrd: Option<&str>) -> String {
    let rmt = match ustrd {
        Some(u) => format!("        <RmtInf><Ustrd>{u}</Ustrd></RmtInf>\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-GOLD-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>GOLD-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-GOLD-001</InstrId>
          <EndToEndId>E2E-GOLD-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000001</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="USD">1234.56</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr><Nm>{dbtr_nm}</Nm></Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtrAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></CdtrAgt>
        <Cdtr><Nm>{cdtr_nm}</Nm></Cdtr>
        <CdtrAcct><Id><IBAN>GB29NWBK60161331926819</IBAN></Id></CdtrAcct>
{rmt}      </CdtTrfTxInf>
    </FIToFICstmrCdtTrf>
  </Document>
</Envelope>"#
    )
}

/// SYNTHETIC: build an MT103 with the given raw 50K name lines, 59 name, and
/// optional field 70. Lines are the raw on-the-wire multi-line layout, so a
/// caller can supply a `/account\r\nNAME` 50K shape to exercise reformatting.
///
/// This mirrors the documented `mt103` builder the in-crate tests use; only
/// the role values vary.
fn mt103(name_50k: &str, name_59: &str, field70: Option<&str>) -> String {
    let f70 = match field70 {
        Some(t) => format!(":70:{t}\r\n"),
        None => String::new(),
    };
    format!(
        "{{1:F01BANKUS33AXXX0000000000}}{{2:I103BANKGB22XXXXN}}{{4:\r\n\
         :20:REF-GOLD-001\r\n\
         :23B:CRED\r\n\
         :32A:240115USD1234,56\r\n\
         :50K:{name_50k}\r\n\
         :59:{name_59}\r\n\
         {f70}\
         :71A:OUR\r\n\
         -}}"
    )
}

/// SYNTHETIC: parse a hand-built MT103 + pacs.008 pair into a `DiffReport`.
///
/// `diff_mt_mx` handles both the upstream Typed path (a normal-length MT103)
/// and the Structural fallback (e.g. an over-long 50K name the typed parser
/// rejects), so this helper does not assert which path was taken.
fn report_for(mt: &str, mx: &str) -> DiffReport {
    let mt = wf_swift::parse(mt).expect("SYNTHETIC MT103 must parse");
    let mx = wf_mx::WfMx::from_xml(mx).expect("SYNTHETIC pacs.008 must parse");
    diff_mt_mx(&mt, &mx).expect("diff_mt_mx must succeed on a SYNTHETIC pacs.008 pair")
}

/// The classification the report assigns to a single role.
fn diff_for(report: &DiffReport, role: Role) -> FieldDiff {
    report
        .rows
        .iter()
        .find(|r| r.role == role)
        .map(|r| r.diff.clone())
        .expect("every role must appear in the report")
}

/// GUARD: keep the detector's cited caps in lockstep with this corpus's
/// independent `CITED_NAME_CAP` literal. If an upstream maxLength facet
/// moves and `Role::*` is bumped without re-checking the citation, this
/// fails loudly — surfacing the drift instead of letting the truncation
/// case silently re-baseline against a moved boundary.
#[test]
fn detector_caps_match_corpus_literal() {
    assert_eq!(
        Role::DebtorName.mt_max_len().capacity(),
        Some(CITED_NAME_CAP),
        "MT 50K cited capacity must equal the corpus literal {CITED_NAME_CAP}"
    );
    assert_eq!(
        Role::DebtorName.mx_max_len().capacity(),
        Some(CITED_NAME_CAP),
        "MX Dbtr/Nm cited maxLength must equal the corpus literal {CITED_NAME_CAP}"
    );
}

/// `Equal`: identical debtor name on both sides, well within the cap.
#[test]
fn equal_identical_debtor_name() {
    // SYNTHETIC: byte-identical debtor names -> Equal.
    let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
    let mt = mt103("JOHN DOE", "JANE SMITH", None);
    let report = report_for(&mt, &mx);

    assert_eq!(
        diff_for(&report, Role::DebtorName),
        FieldDiff::Equal,
        "identical debtor names must be Equal"
    );
}

/// `Reformatted`: MT 50K is a multi-line `/account` + double-spaced name that
/// normalises equal to the MX single-line name, but is NOT byte-equal.
#[test]
fn reformatted_multiline_50k_vs_single_line_mx() {
    // SYNTHETIC: MT 50K carries a leading /account line and an extra space
    // inside the name ("JOHN  DOE"); after the detector strips the account
    // line and collapses whitespace this normalises to the MX "JOHN DOE",
    // but the raw extracted value differs byte-wise -> Reformatted, not Equal.
    let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
    let mt = mt103("/12345678\r\nJOHN  DOE", "JANE SMITH", None);
    let report = report_for(&mt, &mx);

    assert_eq!(
        diff_for(&report, Role::DebtorName),
        FieldDiff::Reformatted,
        "a /account multi-line, double-spaced 50K that normalises equal to the \
         single-line MX name must be Reformatted (not byte-Equal)"
    );
}

/// `Truncated`: MX debtor name is one char over the cited 140 cap; the MT 50K
/// is filled exactly to the cap so it does not itself overflow. The cited cap
/// predicts EXACTLY the 141st char is lost.
#[test]
fn truncated_mx_debtor_name_one_over_cited_cap() {
    // ANTI-TAUTOLOGY: the boundary (CITED_NAME_CAP) and the expected lost char
    // are derived from the cited SWIFT/mx-message caps, NOT from the
    // classifier. MT 50K = 140 'A's (fills the cap, no overflow); MX = the
    // same 140 'A's plus a trailing 'Z' = 141 chars. The cited 140 cap means
    // exactly that 141st 'Z' cannot be carried on the MT side.
    let mt_name = "A".repeat(CITED_NAME_CAP);
    let mx_name = format!("{mt_name}Z");
    assert_eq!(
        mt_name.chars().count(),
        CITED_NAME_CAP,
        "MT 50K is filled to the cited cap"
    );
    assert_eq!(
        mx_name.chars().count(),
        CITED_NAME_CAP + 1,
        "MX name is exactly one char over the cited cap"
    );

    let mx = pacs008_envelope(&mx_name, "JANE SMITH", None);
    let mt = mt103(&mt_name, "JANE SMITH", None);
    let report = report_for(&mt, &mx);

    match diff_for(&report, Role::DebtorName) {
        FieldDiff::Truncated { lost_suffix } => {
            assert_eq!(
                lost_suffix.chars().count(),
                1,
                "the cited {CITED_NAME_CAP}-char cap predicts exactly one lost char"
            );
            assert_eq!(
                lost_suffix, "Z",
                "the lost suffix must be the single char beyond the cited cap"
            );
        }
        other => panic!(
            "expected Truncated against the cited {CITED_NAME_CAP}-char cap, got {other:?} \
             — an upstream maxLength change may have moved the boundary"
        ),
    }
}

/// `Dropped`: MX carries `RmtInf/Ustrd` (remittance) but the MT has no field
/// 70 -> remittance is MX-only and would be lost going to MT.
#[test]
fn dropped_remittance_present_in_mx_absent_in_mt() {
    // SYNTHETIC: MX-only remittance -> Dropped.
    let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", Some("INVOICE 12345"));
    let mt = mt103("JOHN DOE", "JANE SMITH", None);
    let report = report_for(&mt, &mx);

    assert_eq!(
        diff_for(&report, Role::RemittanceInfo),
        FieldDiff::Dropped,
        "remittance present in MX but absent in MT must be Dropped"
    );
}

/// `Added`: the MT carries field 70 (remittance) but the MX omits `RmtInf` ->
/// remittance is MT-only and would be lost going to MX.
#[test]
fn added_remittance_present_in_mt_absent_in_mx() {
    // SYNTHETIC: MT-only remittance (field 70 present, MX RmtInf omitted)
    // -> Added.
    let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
    let mt = mt103("JOHN DOE", "JANE SMITH", Some("PAYMENT FOR SERVICES"));
    let report = report_for(&mt, &mx);

    assert_eq!(
        diff_for(&report, Role::RemittanceInfo),
        FieldDiff::Added,
        "remittance present in MT but absent in MX must be Added"
    );
}

/// `BothAbsent`: remittance is absent from BOTH sides (no MT field 70 and no
/// MX `RmtInf`). This is neither a loss nor a disagreement, so it must
/// classify as `BothAbsent` and must NOT appear in `lossy_rows()`.
#[test]
fn both_absent_remittance_missing_on_both_sides() {
    // SYNTHETIC: no field 70 on the MT side, no RmtInf on the MX side ->
    // nothing to compare for remittance -> BothAbsent (not Mismatch).
    let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
    let mt = mt103("JOHN DOE", "JANE SMITH", None);
    let report = report_for(&mt, &mx);

    assert_eq!(
        diff_for(&report, Role::RemittanceInfo),
        FieldDiff::BothAbsent,
        "remittance absent on both sides must be BothAbsent, not a disagreement"
    );

    let lossy: Vec<Role> = report.lossy_rows().map(|r| r.role).collect();
    assert!(
        !lossy.contains(&Role::RemittanceInfo),
        "a BothAbsent role is not lossy and must not appear in lossy_rows(); got {lossy:?}"
    );
}

/// `Mismatch`: creditor name genuinely differs — not reformatting, not an
/// overflow — so it must be a plain Mismatch.
#[test]
fn mismatch_genuinely_different_creditor_name() {
    // SYNTHETIC: "JANE SMITH" (MT 59) vs "WRONG PERSON" (MX Cdtr/Nm). Both
    // fit the cap and neither is a whitespace/line reformat of the other ->
    // Mismatch.
    let mx = pacs008_envelope("JOHN DOE", "WRONG PERSON", None);
    let mt = mt103("JOHN DOE", "JANE SMITH", None);
    let report = report_for(&mt, &mx);

    assert_eq!(
        diff_for(&report, Role::CreditorName),
        FieldDiff::Mismatch,
        "two genuinely different creditor names (no reformat, no overflow) must be Mismatch"
    );
}
