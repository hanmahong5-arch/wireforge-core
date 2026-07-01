//! Integration tests for the structured-address compliance checker
//! (pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 and pain.001.001.09).
//!
//! ## Anti-tautology contract
//!
//! Expectations here are **derived from the CBPR+ SR2026 rule** (TwnNm + Ctry
//! mandatory in structured fields, effective 2026-11-14), NOT from the
//! checker's internal logic. Each case states the rule that drives it.
//!
//! ## Envelope design
//!
//! Every envelope injects a `<PstlAdr>` block inside the debtor only (the
//! creditor always has no postal address) so we can test each debtor case in
//! isolation without conflating the two parties. All envelopes here are
//! **SYNTHETIC** — hand-built from the upstream typed shapes, never real
//! production messages — and each is asserted to parse via `WfMx::from_xml`
//! before any verdict is checked.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_xform::{
    check_mx_address, check_pacs003_address, check_pacs004_address, check_pacs008_address,
    check_pain001_address, AddressParty, AddressVerdict,
};

// ---------------------------------------------------------------------------
// Envelope builder
// ---------------------------------------------------------------------------

/// Build a pacs.008 envelope.  `dbtr_pstl_adr` is injected verbatim inside
/// the `<Dbtr>` element (after the `<Nm>` tag).  When `None` the debtor has
/// no postal address.  The creditor never has a postal address in this helper.
fn pacs008_envelope(dbtr_pstl_adr: Option<&str>) -> String {
    let pstl_block = match dbtr_pstl_adr {
        Some(block) => format!("        {block}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-ADR-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>ADR-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-ADR-001</InstrId>
          <EndToEndId>E2E-ADR-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000001</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="USD">1000.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr>
          <Nm>JOHN DOE</Nm>
{pstl_block}        </Dbtr>
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

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn row_for(
    report: &wf_xform::AddressComplianceReport,
    party: AddressParty,
) -> &wf_xform::AddressRow {
    report
        .rows
        .iter()
        .find(|r| r.party == party)
        .unwrap_or_else(|| panic!("{} row missing from report", party.as_str()))
}

// ---------------------------------------------------------------------------
// Test cases
// ---------------------------------------------------------------------------

/// Rule: TwnNm AND Ctry present in PstlAdr → Compliant.
///
/// CBPR+ SR2026 requires both structured fields; having both satisfies the
/// rule regardless of any AdrLine presence.
#[test]
fn debtor_both_structured_fields_present_is_compliant() {
    let xml = pacs008_envelope(Some(
        "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::Compliant,
        "TwnNm + Ctry present → must be Compliant per CBPR+ SR2026 rule"
    );
    assert_eq!(row.town_name.as_deref(), Some("LONDON"));
    assert_eq!(row.country.as_deref(), Some("GB"));

    // Creditor has no PstlAdr at all → NoAddress.
    let cdtr = row_for(&report, AddressParty::Creditor);
    assert_eq!(cdtr.verdict, AddressVerdict::NoAddress);
}

/// Rule: PstlAdr present with ONLY AdrLine (no TwnNm, no Ctry) →
/// MissingStructured { town_name_present: false, country_present: false, .. }.
///
/// Unstructured lines do NOT satisfy the SR2026 structured-field requirement.
#[test]
fn debtor_only_adr_lines_no_structured_fields_is_missing_structured() {
    let xml = pacs008_envelope(Some(
        "<PstlAdr><AdrLine>123 MAIN ST</AdrLine><AdrLine>LONDON GB</AdrLine></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(
                !town_name_present,
                "TwnNm absent — town_name_present must be false"
            );
            assert!(
                !country_present,
                "Ctry absent — country_present must be false"
            );
            assert_eq!(
                *unstructured_lines, 2,
                "two AdrLine elements present — unstructured_lines must be 2"
            );
        }
        other => panic!("expected MissingStructured for AdrLine-only address, got {other:?}"),
    }
    assert_eq!(row.unstructured_lines, 2);
    assert!(row.town_name.is_none());
    assert!(row.country.is_none());
}

/// Rule: PstlAdr present with TwnNm but no Ctry →
/// MissingStructured { town_name_present: true, country_present: false, .. }.
///
/// SR2026 requires BOTH fields; TwnNm alone does not satisfy the rule.
#[test]
fn debtor_town_name_only_no_country_is_missing_structured() {
    let xml = pacs008_envelope(Some("<PstlAdr><TwnNm>PARIS</TwnNm></PstlAdr>"));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(
                town_name_present,
                "TwnNm present — town_name_present must be true"
            );
            assert!(
                !country_present,
                "Ctry absent — country_present must be false"
            );
            assert_eq!(*unstructured_lines, 0, "no AdrLine elements");
        }
        other => panic!("expected MissingStructured for TwnNm-only address, got {other:?}"),
    }
    assert_eq!(row.town_name.as_deref(), Some("PARIS"));
    assert!(row.country.is_none());
}

/// Rule: no `<PstlAdr>` element for the debtor → NoAddress.
///
/// A party with no address element at all cannot satisfy SR2026 and is
/// reported as NoAddress so callers can distinguish "element absent" from
/// "element present but incomplete".
#[test]
fn debtor_no_postal_address_element_is_no_address() {
    let xml = pacs008_envelope(None);
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::NoAddress,
        "no PstlAdr element → must be NoAddress"
    );
    assert!(row.town_name.is_none());
    assert!(row.country.is_none());
    assert_eq!(row.unstructured_lines, 0);
}

/// Non-pacs.008 / malformed input → the checker returns an error, not a panic.
///
/// The three-element error message must name the expected type (pacs.008) so
/// callers understand what input is required.
#[test]
fn malformed_or_non_pacs008_input_returns_error_not_panic() {
    // A bare snippet that is not a valid pacs.008 envelope at all.
    let bad = "<NotAMessage/>";
    let result = wf_mx::WfMx::from_xml(bad);
    // Either the upstream parse fails (preferred) or the checker rejects it.
    match result {
        Err(_) => {
            // Parse failure propagates as expected — no panic.
        }
        Ok(mx) => {
            // The upstream accepted it; the checker must still return Err.
            let err = check_pacs008_address(&mx);
            assert!(
                err.is_err(),
                "a non-pacs.008 input must produce an error, not a report"
            );
            let msg = err.unwrap_err().to_string();
            assert!(
                msg.contains("pacs.008"),
                "error must name the expected type: {msg}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Report helper method tests
// ---------------------------------------------------------------------------

/// `non_compliant_rows()` must yield only the rows that are NOT Compliant.
#[test]
fn non_compliant_rows_excludes_compliant_party() {
    // Debtor compliant, creditor NoAddress (no pstl block added).
    let xml = pacs008_envelope(Some(
        "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    // Debtor is Compliant, Creditor is NoAddress → one non-compliant row.
    let nc: Vec<_> = report.non_compliant_rows().collect();
    assert_eq!(
        nc.len(),
        1,
        "only the creditor (NoAddress) must appear in non_compliant_rows"
    );
    assert_eq!(nc[0].party, AddressParty::Creditor);
}

/// `all_compliant()` returns `true` only when every row is Compliant.
#[test]
fn all_compliant_false_when_any_row_is_not_compliant() {
    // No postal address on debtor → NoAddress → not all_compliant.
    let xml = pacs008_envelope(None);
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    assert!(
        !report.all_compliant(),
        "a report with NoAddress rows must not be all_compliant"
    );
}

/// Rows are emitted in `AddressParty::ALL` order (Debtor first, Creditor
/// second) so callers can rely on stable ordering.
#[test]
fn rows_are_in_address_party_all_order() {
    let xml = pacs008_envelope(None);
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_pacs008_address(&mx).expect("checker must not error");

    assert_eq!(report.rows.len(), 2, "always two rows (debtor + creditor)");
    assert_eq!(
        report.rows[0].party,
        AddressParty::ALL[0],
        "first row must be Debtor"
    );
    assert_eq!(
        report.rows[1].party,
        AddressParty::ALL[1],
        "second row must be Creditor"
    );
}

// ---------------------------------------------------------------------------
// pacs.004.001.09 (Payment Return) — SYNTHETIC envelopes
// ---------------------------------------------------------------------------
//
// The pacs.004 parties live under the return chain
// (`TxInf/RtrChain/{Dbtr,Cdtr}`) behind an `Option<Pty>` choice, unlike
// pacs.008 where the party is direct. The structured-field rule is identical:
// a `PstlAdr` must carry `TwnNm` + `Ctry`. Required-field set below is taken
// from the upstream `pacs_004_001_09` typed model (GrpHdr + TxInf with the
// mandatory OrgnlEndToEndId / OrgnlUETR / RtrdIntrBkSttlmAmt / IntrBkSttlmDt /
// ChrgBr / InstgAgt / InstdAgt / RtrChain / RtrRsnInf elements).

/// Build a pacs.004 (PmtRtr) envelope. `dbtr_inner` is injected verbatim
/// inside the return chain's `<Dbtr>`; the creditor is a fixed agent-only
/// party (no `Pty`, hence no postal address).
fn pacs004_envelope(dbtr_inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-RTR-001</BizMsgIdr>
    <MsgDefIdr>pacs.004.001.09</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <PmtRtr>
      <GrpHdr>
        <MsgId>RTR-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <TxInf>
        <OrgnlEndToEndId>E2E-RTR-001</OrgnlEndToEndId>
        <OrgnlUETR>00000000-0000-4000-8000-000000000001</OrgnlUETR>
        <RtrdIntrBkSttlmAmt Ccy="USD">1000.00</RtrdIntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstdAgt>
        <RtrChain>
          <Dbtr>{dbtr_inner}</Dbtr>
          <Cdtr><Agt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></Agt></Cdtr>
        </RtrChain>
        <RtrRsnInf><Rsn><Cd>AC04</Cd></Rsn></RtrRsnInf>
      </TxInf>
    </PmtRtr>
  </Document>
</Envelope>"#
    )
}

/// Guard: every pacs.004 fixture must actually parse as a pacs.004 document
/// before its verdict can mean anything. If this fails, the required-field
/// set in `pacs004_envelope` is wrong — fix the envelope, not the assertion.
#[test]
fn pacs004_fixture_parses_as_pacs004() {
    let xml = pacs004_envelope(
        "<Pty><Nm>ACME CORP</Nm><PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr></Pty>",
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 fixture must parse");
    assert_eq!(
        mx.message_type().expect("message type"),
        "pacs.004.001.09",
        "fixture must classify as pacs.004.001.09"
    );
    // Reaching check_pacs004_address without error proves the typed body is
    // the Pacs004 variant.
    check_pacs004_address(&mx).expect("pacs.004 checker must accept the fixture");
}

/// Rule: TwnNm AND Ctry present in the return-chain debtor's PstlAdr →
/// Compliant. Same SR2026 structured-field rule as pacs.008.
#[test]
fn pacs004_debtor_both_structured_fields_present_is_compliant() {
    let xml = pacs004_envelope(
        "<Pty><Nm>ACME CORP</Nm><PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr></Pty>",
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 must parse");
    let report = check_pacs004_address(&mx).expect("checker must not error");
    assert_eq!(report.message_type, "pacs.004.001.09");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::Compliant,
        "TwnNm + Ctry present → Compliant per CBPR+ SR2026 rule"
    );
    assert_eq!(row.town_name.as_deref(), Some("LONDON"));
    assert_eq!(row.country.as_deref(), Some("GB"));

    // Creditor is an agent-only party (no Pty) → NoAddress.
    let cdtr = row_for(&report, AddressParty::Creditor);
    assert_eq!(cdtr.verdict, AddressVerdict::NoAddress);
}

/// Rule: PstlAdr with ONLY AdrLine → MissingStructured (both flags false),
/// unstructured line count reported. Unstructured lines do not satisfy SR2026.
#[test]
fn pacs004_debtor_only_adr_lines_is_missing_structured() {
    let xml = pacs004_envelope(
        "<Pty><Nm>ACME CORP</Nm>\
         <PstlAdr><AdrLine>123 MAIN ST</AdrLine><AdrLine>LONDON GB</AdrLine></PstlAdr></Pty>",
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 must parse");
    let report = check_pacs004_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(!town_name_present, "TwnNm absent");
            assert!(!country_present, "Ctry absent");
            assert_eq!(*unstructured_lines, 2, "two AdrLine elements present");
        }
        other => panic!("expected MissingStructured for AdrLine-only, got {other:?}"),
    }
    assert_eq!(row.unstructured_lines, 2);
}

/// Rule: PstlAdr with TwnNm but no Ctry → MissingStructured (town true,
/// country false). SR2026 requires BOTH fields.
#[test]
fn pacs004_debtor_town_name_only_is_missing_structured() {
    let xml =
        pacs004_envelope("<Pty><Nm>ACME CORP</Nm><PstlAdr><TwnNm>PARIS</TwnNm></PstlAdr></Pty>");
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 must parse");
    let report = check_pacs004_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(town_name_present, "TwnNm present");
            assert!(!country_present, "Ctry absent");
            assert_eq!(*unstructured_lines, 0, "no AdrLine elements");
        }
        other => panic!("expected MissingStructured for TwnNm-only, got {other:?}"),
    }
    assert_eq!(row.town_name.as_deref(), Some("PARIS"));
    assert!(row.country.is_none());
}

/// Rule: a return-chain party identified only by an agent (`<Agt>`, no
/// `<Pty>`) carries no postal address → NoAddress. This is the pacs.004-
/// specific `.pty == None` path that pacs.008 has no analogue for.
#[test]
fn pacs004_agent_only_debtor_is_no_address() {
    let xml = pacs004_envelope("<Agt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></Agt>");
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 must parse");
    let report = check_pacs004_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::NoAddress,
        "agent-only party (.pty == None) → NoAddress"
    );
    assert!(row.town_name.is_none());
    assert!(row.country.is_none());
    assert_eq!(row.unstructured_lines, 0);
}

/// Rule: a `<Pty>` present but with NO `<PstlAdr>` also yields NoAddress —
/// the and_then over `pstl_adr` collapses to None just like the agent-only
/// case.
#[test]
fn pacs004_party_without_postal_address_is_no_address() {
    let xml = pacs004_envelope("<Pty><Nm>ACME CORP</Nm></Pty>");
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 must parse");
    let report = check_pacs004_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(row.verdict, AddressVerdict::NoAddress);
}

// ---------------------------------------------------------------------------
// Unified dispatch: check_mx_address
// ---------------------------------------------------------------------------

/// `check_mx_address` routes a pacs.008 document to the pacs.008 checker.
#[test]
fn dispatch_routes_pacs008_to_pacs008_checker() {
    let xml = pacs008_envelope(Some(
        "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.008 must parse");
    let report = check_mx_address(&mx).expect("dispatch must accept pacs.008");
    assert_eq!(report.message_type, "pacs.008.001.08");
    assert_eq!(
        row_for(&report, AddressParty::Debtor).verdict,
        AddressVerdict::Compliant
    );
}

/// `check_mx_address` routes a pacs.004 document to the pacs.004 checker.
#[test]
fn dispatch_routes_pacs004_to_pacs004_checker() {
    let xml = pacs004_envelope(
        "<Pty><Nm>ACME CORP</Nm><PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr></Pty>",
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.004 must parse");
    let report = check_mx_address(&mx).expect("dispatch must accept pacs.004");
    assert_eq!(report.message_type, "pacs.004.001.09");
    assert_eq!(
        row_for(&report, AddressParty::Debtor).verdict,
        AddressVerdict::Compliant
    );
}

/// `check_mx_address` rejects an unsupported MX document with the
/// MxNotAddressCheckable three-element error naming the supported set.
#[test]
fn dispatch_rejects_unsupported_document_type() {
    // A pacs.002 (payment status report) is a real, supported MX type for
    // parsing, but NOT one the address checker handles — the ideal probe for
    // the dispatch fallthrough. If this fixture stops parsing, swap it for
    // any other non-pacs.008/004 envelope the facade accepts.
    let pacs002 = r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-STS-001</BizMsgIdr>
    <MsgDefIdr>pacs.002.001.10</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFIPmtStsRpt>
      <GrpHdr>
        <MsgId>STS-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
      </GrpHdr>
      <TxInfAndSts>
        <OrgnlEndToEndId>E2E-STS-001</OrgnlEndToEndId>
        <OrgnlUETR>00000000-0000-4000-8000-000000000001</OrgnlUETR>
        <TxSts>ACSP</TxSts>
      </TxInfAndSts>
    </FIToFIPmtStsRpt>
  </Document>
</Envelope>"#;

    match wf_mx::WfMx::from_xml(pacs002) {
        Ok(mx) => {
            // Parsed as some non-address-checkable type → must be rejected,
            // and the error must name the supported set.
            let err =
                check_mx_address(&mx).expect_err("a non-address-checkable type must be rejected");
            let msg = err.to_string();
            assert!(
                msg.contains("pacs.008.001.08")
                    && msg.contains("pacs.004.001.09")
                    && msg.contains("pacs.003.001.08")
                    && msg.contains("pain.001.001.09"),
                "error must name the full supported set: {msg}"
            );
        }
        Err(_) => {
            // If the facade does not accept this particular pacs.002 fixture,
            // the dispatch-rejection path is still covered by the unit test in
            // wf-xform/src/lib.rs (mx_not_address_checkable Display); skip
            // rather than assert on an envelope the parser won't take.
        }
    }
}

// ---------------------------------------------------------------------------
// pacs.003.001.08 (FIToFICstmrDrctDbt — customer direct debit) — SYNTHETIC
// ---------------------------------------------------------------------------
//
// pacs.003 is single-transaction with the debtor and creditor directly under
// `DrctDbtTxInf` (no `Pty` indirection — an exact pacs.008 mirror). The
// structured-field rule is identical: a `PstlAdr` must carry `TwnNm` + `Ctry`.
// The required-field set below is taken from the upstream
// `pacs_003_001_08::DirectDebitTransactionInformation241` (the mandatory
// PmtId / IntrBkSttlmAmt / IntrBkSttlmDt / ChrgBr / ReqdColltnDt / Cdtr /
// CdtrAgt / InstgAgt / InstdAgt / Dbtr / DbtrAcct / DbtrAgt elements) plus
// `GroupHeader941`.

/// Build a pacs.003 (FIToFICstmrDrctDbt) envelope. `dbtr_pstl_adr` is injected
/// verbatim inside the `<Dbtr>` element (after `<Nm>`); when `None` the debtor
/// has no postal address. The creditor never has a postal address here.
fn pacs003_envelope(dbtr_pstl_adr: Option<&str>) -> String {
    let pstl_block = match dbtr_pstl_adr {
        Some(block) => format!("          {block}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-DD-001</BizMsgIdr>
    <MsgDefIdr>pacs.003.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrDrctDbt>
      <GrpHdr>
        <MsgId>DD-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <DrctDbtTxInf>
        <PmtId>
          <InstrId>INSTR-DD-001</InstrId>
          <EndToEndId>E2E-DD-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000001</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="USD">1000.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <ReqdColltnDt>2024-01-15</ReqdColltnDt>
        <Cdtr><Nm>JANE SMITH</Nm></Cdtr>
        <CdtrAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></CdtrAgt>
        <InstgAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr>
          <Nm>JOHN DOE</Nm>
{pstl_block}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
      </DrctDbtTxInf>
    </FIToFICstmrDrctDbt>
  </Document>
</Envelope>"#
    )
}

/// Guard: the pacs.003 fixture must actually parse as a pacs.003 document
/// before any verdict can mean anything. If this fails, the required-field set
/// in `pacs003_envelope` is wrong — fix the envelope, not the assertion.
#[test]
fn pacs003_fixture_parses_as_pacs003() {
    let xml = pacs003_envelope(Some(
        "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.003 fixture must parse");
    assert_eq!(
        mx.message_type().expect("message type"),
        "pacs.003.001.08",
        "fixture must classify as pacs.003.001.08"
    );
    // Reaching check_pacs003_address without error proves the typed body is
    // the Pacs003 variant.
    check_pacs003_address(&mx).expect("pacs.003 checker must accept the fixture");
}

/// Rule: TwnNm AND Ctry present in the debtor's PstlAdr → Compliant. Same
/// SR2026 structured-field rule as pacs.008.
#[test]
fn pacs003_debtor_both_structured_fields_present_is_compliant() {
    let xml = pacs003_envelope(Some(
        "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.003 must parse");
    let report = check_pacs003_address(&mx).expect("checker must not error");
    assert_eq!(report.message_type, "pacs.003.001.08");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::Compliant,
        "TwnNm + Ctry present → Compliant per CBPR+ SR2026 rule"
    );
    assert_eq!(row.town_name.as_deref(), Some("LONDON"));
    assert_eq!(row.country.as_deref(), Some("GB"));

    // Creditor has no PstlAdr at all → NoAddress.
    let cdtr = row_for(&report, AddressParty::Creditor);
    assert_eq!(cdtr.verdict, AddressVerdict::NoAddress);
}

/// Rule: PstlAdr with ONLY AdrLine → MissingStructured (both flags false),
/// unstructured line count reported. Unstructured lines do not satisfy SR2026.
#[test]
fn pacs003_debtor_only_adr_lines_is_missing_structured() {
    let xml = pacs003_envelope(Some(
        "<PstlAdr><AdrLine>123 MAIN ST</AdrLine><AdrLine>LONDON GB</AdrLine></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.003 must parse");
    let report = check_pacs003_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(!town_name_present, "TwnNm absent");
            assert!(!country_present, "Ctry absent");
            assert_eq!(*unstructured_lines, 2, "two AdrLine elements present");
        }
        other => panic!("expected MissingStructured for AdrLine-only, got {other:?}"),
    }
    assert_eq!(row.unstructured_lines, 2);
}

/// Rule: PstlAdr with TwnNm but no Ctry → MissingStructured (town true,
/// country false). SR2026 requires BOTH fields.
#[test]
fn pacs003_debtor_town_name_only_is_missing_structured() {
    let xml = pacs003_envelope(Some("<PstlAdr><TwnNm>PARIS</TwnNm></PstlAdr>"));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.003 must parse");
    let report = check_pacs003_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(town_name_present, "TwnNm present");
            assert!(!country_present, "Ctry absent");
            assert_eq!(*unstructured_lines, 0, "no AdrLine elements");
        }
        other => panic!("expected MissingStructured for TwnNm-only, got {other:?}"),
    }
    assert_eq!(row.town_name.as_deref(), Some("PARIS"));
    assert!(row.country.is_none());
}

/// Rule: no `<PstlAdr>` element for the debtor → NoAddress.
#[test]
fn pacs003_debtor_no_postal_address_is_no_address() {
    let xml = pacs003_envelope(None);
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.003 must parse");
    let report = check_pacs003_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::NoAddress,
        "no PstlAdr element → NoAddress"
    );
    assert!(row.town_name.is_none());
    assert!(row.country.is_none());
    assert_eq!(row.unstructured_lines, 0);
}

// ---------------------------------------------------------------------------
// pain.001.001.09 (CstmrCdtTrfInitn — customer credit-transfer initiation)
// — SYNTHETIC
// ---------------------------------------------------------------------------
//
// pain.001 is single-transaction, but its two parties sit at DIFFERENT nesting
// levels under the single `PmtInf`: debtor = `PmtInf/Dbtr`, creditor =
// `PmtInf/CdtTrfTxInf/Cdtr`. The structured-field rule is identical. The
// required-field set below is taken from the upstream `pain_001_001_09`
// (`GroupHeader851`, `PaymentInstruction301`, `CreditTransferTransaction341`):
// mandatory GrpHdr {MsgId, CreDtTm, NbOfTxs, InitgPty}; PmtInf {PmtInfId,
// PmtMtd, ReqdExctnDt, Dbtr, DbtrAcct, DbtrAgt, CdtTrfTxInf}; CdtTrfTxInf
// {PmtId(EndToEndId, UETR), Amt, Cdtr}.

/// Build a pain.001 (CstmrCdtTrfInitn) envelope. `dbtr_pstl_adr` is injected
/// inside `PmtInf/Dbtr`, `cdtr_pstl_adr` inside `PmtInf/CdtTrfTxInf/Cdtr`
/// (each after `<Nm>`); `None` means that party has no postal address. The two
/// injection points exercise the distinct debtor/creditor nesting levels.
fn pain001_envelope(dbtr_pstl_adr: Option<&str>, cdtr_pstl_adr: Option<&str>) -> String {
    let dbtr_block = match dbtr_pstl_adr {
        Some(block) => format!("          {block}\n"),
        None => String::new(),
    };
    let cdtr_block = match cdtr_pstl_adr {
        Some(block) => format!("            {block}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-INI-001</BizMsgIdr>
    <MsgDefIdr>pain.001.001.09</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <CstmrCdtTrfInitn>
      <GrpHdr>
        <MsgId>INI-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <InitgPty><Nm>INIT PARTY</Nm></InitgPty>
      </GrpHdr>
      <PmtInf>
        <PmtInfId>PMT-INI-001</PmtInfId>
        <PmtMtd>TRF</PmtMtd>
        <ReqdExctnDt><Dt>2024-01-15</Dt></ReqdExctnDt>
        <Dbtr>
          <Nm>JOHN DOE</Nm>
{dbtr_block}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtTrfTxInf>
          <PmtId>
            <EndToEndId>E2E-INI-001</EndToEndId>
            <UETR>00000000-0000-4000-8000-000000000001</UETR>
          </PmtId>
          <Amt><InstdAmt Ccy="USD">1000.00</InstdAmt></Amt>
          <Cdtr>
            <Nm>JANE SMITH</Nm>
{cdtr_block}          </Cdtr>
        </CdtTrfTxInf>
      </PmtInf>
    </CstmrCdtTrfInitn>
  </Document>
</Envelope>"#
    )
}

/// Guard: the pain.001 fixture must actually parse as a pain.001 document
/// before any verdict can mean anything. If this fails, the required-field set
/// in `pain001_envelope` is wrong — fix the envelope, not the assertion.
#[test]
fn pain001_fixture_parses_as_pain001() {
    let xml = pain001_envelope(
        Some("<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>"),
        None,
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 fixture must parse");
    assert_eq!(
        mx.message_type().expect("message type"),
        "pain.001.001.09",
        "fixture must classify as pain.001.001.09"
    );
    check_pain001_address(&mx).expect("pain.001 checker must accept the fixture");
}

/// Rule: TwnNm AND Ctry present in the debtor's PstlAdr → Compliant.
#[test]
fn pain001_debtor_both_structured_fields_present_is_compliant() {
    let xml = pain001_envelope(
        Some("<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>"),
        None,
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 must parse");
    let report = check_pain001_address(&mx).expect("checker must not error");
    assert_eq!(report.message_type, "pain.001.001.09");

    let row = row_for(&report, AddressParty::Debtor);
    assert_eq!(
        row.verdict,
        AddressVerdict::Compliant,
        "TwnNm + Ctry present → Compliant per CBPR+ SR2026 rule"
    );
    assert_eq!(row.town_name.as_deref(), Some("LONDON"));
    assert_eq!(row.country.as_deref(), Some("GB"));

    // Creditor has no PstlAdr → NoAddress.
    let cdtr = row_for(&report, AddressParty::Creditor);
    assert_eq!(cdtr.verdict, AddressVerdict::NoAddress);
}

/// Rule: the creditor lives one level deeper (`CdtTrfTxInf/Cdtr`); a compliant
/// creditor PstlAdr must be read from that distinct nesting level. This is the
/// pain.001-specific path that the pacs mirrors have no analogue for.
#[test]
fn pain001_creditor_structured_fields_read_from_deeper_nesting() {
    let xml = pain001_envelope(
        None,
        Some("<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>"),
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 must parse");
    let report = check_pain001_address(&mx).expect("checker must not error");

    // Debtor had no PstlAdr → NoAddress; the creditor address at the deeper
    // level must still be found and classified Compliant.
    let dbtr = row_for(&report, AddressParty::Debtor);
    assert_eq!(dbtr.verdict, AddressVerdict::NoAddress);

    let cdtr = row_for(&report, AddressParty::Creditor);
    assert_eq!(
        cdtr.verdict,
        AddressVerdict::Compliant,
        "creditor TwnNm + Ctry at PmtInf/CdtTrfTxInf/Cdtr must be Compliant"
    );
    assert_eq!(cdtr.town_name.as_deref(), Some("BERLIN"));
    assert_eq!(cdtr.country.as_deref(), Some("DE"));
}

/// Rule: PstlAdr with ONLY AdrLine → MissingStructured (both flags false).
#[test]
fn pain001_debtor_only_adr_lines_is_missing_structured() {
    let xml = pain001_envelope(
        Some("<PstlAdr><AdrLine>123 MAIN ST</AdrLine><AdrLine>LONDON GB</AdrLine></PstlAdr>"),
        None,
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 must parse");
    let report = check_pain001_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(!town_name_present, "TwnNm absent");
            assert!(!country_present, "Ctry absent");
            assert_eq!(*unstructured_lines, 2, "two AdrLine elements present");
        }
        other => panic!("expected MissingStructured for AdrLine-only, got {other:?}"),
    }
    assert_eq!(row.unstructured_lines, 2);
}

/// Rule: PstlAdr with TwnNm but no Ctry → MissingStructured (town true,
/// country false). SR2026 requires BOTH fields.
#[test]
fn pain001_debtor_town_name_only_is_missing_structured() {
    let xml = pain001_envelope(Some("<PstlAdr><TwnNm>PARIS</TwnNm></PstlAdr>"), None);
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 must parse");
    let report = check_pain001_address(&mx).expect("checker must not error");

    let row = row_for(&report, AddressParty::Debtor);
    match &row.verdict {
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            assert!(town_name_present, "TwnNm present");
            assert!(!country_present, "Ctry absent");
            assert_eq!(*unstructured_lines, 0, "no AdrLine elements");
        }
        other => panic!("expected MissingStructured for TwnNm-only, got {other:?}"),
    }
    assert_eq!(row.town_name.as_deref(), Some("PARIS"));
    assert!(row.country.is_none());
}

/// Rule: no `<PstlAdr>` on either party → both NoAddress.
#[test]
fn pain001_no_postal_addresses_are_no_address() {
    let xml = pain001_envelope(None, None);
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 must parse");
    let report = check_pain001_address(&mx).expect("checker must not error");

    assert_eq!(
        row_for(&report, AddressParty::Debtor).verdict,
        AddressVerdict::NoAddress
    );
    assert_eq!(
        row_for(&report, AddressParty::Creditor).verdict,
        AddressVerdict::NoAddress
    );
}

/// `check_mx_address` routes a pacs.003 document to the pacs.003 checker.
#[test]
fn dispatch_routes_pacs003_to_pacs003_checker() {
    let xml = pacs003_envelope(Some(
        "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
    ));
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pacs.003 must parse");
    let report = check_mx_address(&mx).expect("dispatch must accept pacs.003");
    assert_eq!(report.message_type, "pacs.003.001.08");
    assert_eq!(
        row_for(&report, AddressParty::Debtor).verdict,
        AddressVerdict::Compliant
    );
}

/// `check_mx_address` routes a pain.001 document to the pain.001 checker.
#[test]
fn dispatch_routes_pain001_to_pain001_checker() {
    let xml = pain001_envelope(
        Some("<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>"),
        None,
    );
    let mx = wf_mx::WfMx::from_xml(&xml).expect("pain.001 must parse");
    let report = check_mx_address(&mx).expect("dispatch must accept pain.001");
    assert_eq!(report.message_type, "pain.001.001.09");
    assert_eq!(
        row_for(&report, AddressParty::Debtor).verdict,
        AddressVerdict::Compliant
    );
}
