//! Regression PIN: the detector's truncation verdict is grounded in the
//! CITED upstream maxLength caps, and this test fails loudly if an upstream
//! bump silently changes the boundary.
//!
//! Anti-tautology: the boundary (140) is taken from the cited sources, NOT
//! from the detector's classifier:
//!
//! - MT 50K debtor name capacity = 4 lines * 35 chars = 140 (SWIFT MT103
//!   field 50K format `4*35x`; mirrored by wf-codec field_50k.rs).
//! - MX Dbtr/Nm maxLength = 140 (mx-message 3.1.4
//!   `document::pacs_008_001_08` PartyIdentification1352::validate ->
//!   `validate_length(val, "Nm", Some(1), Some(140), ...)`).
//!
//! Therefore a 141-char MX debtor name CANNOT fit the 140-char MT field and
//! the detector must report Truncated with exactly the 141st char lost; a
//! 140-char name fits and must NOT be Truncated. If a future mx-message
//! release changes the 140 facet (e.g. to 70 or 350), the wf-xform
//! `Role::mx_max_len`/`mt_max_len` cap and this PIN will disagree and this
//! test breaks — surfacing the drift instead of hiding it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_xform::{diff_mt_mx, FieldDiff, Role};

/// The cited 140-char cap this PIN is grounded on. Defined as a literal
/// here (NOT read from the detector) so the test is an independent anchor.
const CITED_NAME_CAP: usize = 140;

/// Build a full pacs.008.001.08 envelope with a given debtor name. Same
/// envelope shape wf-xform's own in-crate tests use (which in turn mirrors
/// the wf-mx facade's documented pacs.008 sample), so this PIN exercises a
/// genuinely parseable message rather than an invented shape.
fn pacs008_envelope(dbtr_nm: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-PIN-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>PIN-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-PIN-001</InstrId>
          <EndToEndId>E2E-PIN-001</EndToEndId>
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
        <Cdtr><Nm>JANE SMITH</Nm></Cdtr>
        <CdtrAcct><Id><IBAN>GB29NWBK60161331926819</IBAN></Id></CdtrAcct>
      </CdtTrfTxInf>
    </FIToFICstmrCdtTrf>
  </Document>
</Envelope>"#
    )
}

/// Build an MT103 with the given 50K debtor name.
fn mt103(name_50k: &str) -> String {
    format!(
        "{{1:F01BANKUS33AXXX0000000000}}{{2:I103BANKGB22XXXXN}}{{4:\r\n\
         :20:REF-PIN-001\r\n\
         :23B:CRED\r\n\
         :32A:240115USD1234,56\r\n\
         :50K:{name_50k}\r\n\
         :59:JANE SMITH\r\n\
         :71A:OUR\r\n\
         -}}"
    )
}

fn debtor_diff(mt_name: &str, mx_name: &str) -> FieldDiff {
    let mt = wf_swift::parse(&mt103(mt_name)).expect("MT103 must parse");
    let mx = wf_mx::WfMx::from_xml(&pacs008_envelope(mx_name)).expect("pacs.008 must parse");
    let report = diff_mt_mx(&mt, &mx).expect("diff");
    report
        .rows
        .iter()
        .find(|r| r.role == Role::DebtorName)
        .map(|r| r.diff.clone())
        .expect("debtor role present in report")
}

#[test]
fn debtor_name_one_over_cited_cap_is_truncated_losing_exactly_one_char() {
    // MT 50K filled to its cited 140 cap (so it does not itself overflow);
    // MX one char longer (the same 140 plus a trailing 'Z'). The cited cap
    // predicts exactly the 141st char is lost.
    let mt_name = "A".repeat(CITED_NAME_CAP);
    let mx_name = format!("{mt_name}Z");
    assert_eq!(mt_name.chars().count(), CITED_NAME_CAP);
    assert_eq!(mx_name.chars().count(), CITED_NAME_CAP + 1);

    match debtor_diff(&mt_name, &mx_name) {
        FieldDiff::Truncated { lost_suffix } => {
            assert_eq!(
                lost_suffix.chars().count(),
                1,
                "the cited {CITED_NAME_CAP}-char cap predicts exactly one lost char"
            );
            assert_eq!(
                lost_suffix, "Z",
                "the lost suffix must be the trailing char beyond the cited cap"
            );
        }
        other => panic!(
            "expected Truncated against the cited {CITED_NAME_CAP}-char cap, got {other:?} \
             — an upstream maxLength change may have moved the boundary"
        ),
    }
}

#[test]
fn debtor_name_exactly_at_cited_cap_is_not_truncated() {
    // A 140-char name equals the cited cap, so it fits. With identical
    // values on both sides the verdict is Equal — and crucially NOT
    // Truncated. This pins the inclusive boundary the Some(140) facet
    // requires.
    let name = "B".repeat(CITED_NAME_CAP);
    let diff = debtor_diff(&name, &name);
    assert!(
        !matches!(diff, FieldDiff::Truncated { .. }),
        "a value exactly at the cited {CITED_NAME_CAP}-char cap must NOT be Truncated; got {diff:?}"
    );
    assert_eq!(
        diff,
        FieldDiff::Equal,
        "identical 140-char names at the cap fit and compare Equal"
    );
}

#[test]
fn detector_cited_caps_match_the_pin_assumption() {
    // Guard: the detector's own cited caps must equal the value this PIN is
    // grounded on. If wf-xform changes its constant without updating the
    // citation, this fails — keeping the two in lockstep.
    assert_eq!(
        Role::DebtorName.mt_max_len().capacity(),
        Some(CITED_NAME_CAP),
        "MT 50K cited capacity must be the pinned {CITED_NAME_CAP}"
    );
    assert_eq!(
        Role::DebtorName.mx_max_len().capacity(),
        Some(CITED_NAME_CAP),
        "MX Dbtr/Nm cited maxLength must be the pinned {CITED_NAME_CAP}"
    );
}
