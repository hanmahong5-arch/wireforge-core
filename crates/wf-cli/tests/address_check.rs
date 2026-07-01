//! Integration tests for the `wf xform address-check` entry point.
//!
//! These call the pure lib.rs entry point (`mx_address_compliance`) directly;
//! the binary is a thin file-reading dispatcher over it. Expectations are
//! **derived from the CBPR+ SR2026 rule** (TwnNm + Ctry mandatory in
//! structured fields, effective 2026-11-14), not from the checker's internals.
//! All envelopes are **SYNTHETIC**.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_cli::{
    mx_address_compliance, mx_address_report, render_address_scan, select_xml, AddressGate,
    ScanEntry,
};

/// pacs.008 envelope with an optional debtor `<PstlAdr>` block injected after
/// `<Nm>`. Same shape the wf-mx / wf-xform crates use in their own tests.
fn pacs008(dbtr_pstl_adr: Option<&str>) -> String {
    let pstl = match dbtr_pstl_adr {
        Some(b) => format!("        {b}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-CLI-ADR-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>CLI-ADR-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-CLI-ADR-001</InstrId>
          <EndToEndId>E2E-CLI-ADR-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000001</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="USD">1000.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr>
          <Nm>JOHN DOE</Nm>
{pstl}        </Dbtr>
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

/// pacs.004 (PmtRtr) envelope; `dbtr_inner` is injected inside the return
/// chain's `<Dbtr>`. The creditor is a fixed agent-only party.
fn pacs004(dbtr_inner: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-CLI-RTR-001</BizMsgIdr>
    <MsgDefIdr>pacs.004.001.09</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <PmtRtr>
      <GrpHdr>
        <MsgId>CLI-RTR-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <TxInf>
        <OrgnlEndToEndId>E2E-CLI-RTR-001</OrgnlEndToEndId>
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

#[test]
fn header_states_scope_and_disclaims_certification() {
    let out = mx_address_compliance(&pacs008(Some(
        "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
    )))
    .unwrap();
    assert!(
        out.contains("SR2026"),
        "header must cite SR2026, got: {out}"
    );
    assert!(
        out.contains("NOT a certification"),
        "header must disclaim certification, got: {out}"
    );
}

#[test]
fn pacs008_compliant_debtor_renders_compliant_and_message_type() {
    let out = mx_address_compliance(&pacs008(Some(
        "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
    )))
    .unwrap();
    assert!(
        out.contains("message_type: pacs.008.001.08"),
        "header must name the detected pacs.008 type, got: {out}"
    );
    assert!(
        out.contains("debtor: compliant"),
        "TwnNm + Ctry present must render compliant, got: {out}"
    );
}

#[test]
fn pacs004_compliant_debtor_renders_compliant_and_message_type() {
    let out = mx_address_compliance(&pacs004(
        "<Pty><Nm>ACME CORP</Nm><PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr></Pty>",
    ))
    .unwrap();
    assert!(
        out.contains("message_type: pacs.004.001.09"),
        "header must name the detected pacs.004 type, got: {out}"
    );
    assert!(
        out.contains("debtor: compliant"),
        "TwnNm + Ctry present must render compliant, got: {out}"
    );
}

#[test]
fn pacs004_agent_only_debtor_renders_no_address() {
    let out = mx_address_compliance(&pacs004(
        "<Agt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></Agt>",
    ))
    .unwrap();
    assert!(
        out.contains("debtor: no_address"),
        "agent-only debtor (no Pty) must render no_address, got: {out}"
    );
}

/// pacs.003 (FIToFICstmrDrctDbt) envelope; `dbtr_pstl_adr` is injected inside
/// `DrctDbtTxInf/Dbtr` after `<Nm>`. The creditor has no postal address.
fn pacs003(dbtr_pstl_adr: Option<&str>) -> String {
    let pstl = match dbtr_pstl_adr {
        Some(b) => format!("          {b}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-CLI-DD-001</BizMsgIdr>
    <MsgDefIdr>pacs.003.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrDrctDbt>
      <GrpHdr>
        <MsgId>CLI-DD-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <DrctDbtTxInf>
        <PmtId>
          <InstrId>INSTR-CLI-DD-001</InstrId>
          <EndToEndId>E2E-CLI-DD-001</EndToEndId>
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
{pstl}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
      </DrctDbtTxInf>
    </FIToFICstmrDrctDbt>
  </Document>
</Envelope>"#
    )
}

/// pain.001 (CstmrCdtTrfInitn) envelope; `dbtr_pstl_adr` is injected inside
/// `PmtInf/Dbtr` after `<Nm>`. The creditor (one level deeper, under
/// `CdtTrfTxInf`) has no postal address.
fn pain001(dbtr_pstl_adr: Option<&str>) -> String {
    let pstl = match dbtr_pstl_adr {
        Some(b) => format!("          {b}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-CLI-INI-001</BizMsgIdr>
    <MsgDefIdr>pain.001.001.09</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <CstmrCdtTrfInitn>
      <GrpHdr>
        <MsgId>CLI-INI-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <InitgPty><Nm>INIT PARTY</Nm></InitgPty>
      </GrpHdr>
      <PmtInf>
        <PmtInfId>PMT-CLI-INI-001</PmtInfId>
        <PmtMtd>TRF</PmtMtd>
        <ReqdExctnDt><Dt>2024-01-15</Dt></ReqdExctnDt>
        <Dbtr>
          <Nm>JOHN DOE</Nm>
{pstl}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtTrfTxInf>
          <PmtId>
            <EndToEndId>E2E-CLI-INI-001</EndToEndId>
            <UETR>00000000-0000-4000-8000-000000000001</UETR>
          </PmtId>
          <Amt><InstdAmt Ccy="USD">1000.00</InstdAmt></Amt>
          <Cdtr><Nm>JANE SMITH</Nm></Cdtr>
        </CdtTrfTxInf>
      </PmtInf>
    </CstmrCdtTrfInitn>
  </Document>
</Envelope>"#
    )
}

#[test]
fn pacs003_compliant_debtor_renders_compliant_and_message_type() {
    let out = mx_address_compliance(&pacs003(Some(
        "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
    )))
    .unwrap();
    assert!(
        out.contains("message_type: pacs.003.001.08"),
        "header must name the detected pacs.003 type, got: {out}"
    );
    assert!(
        out.contains("debtor: compliant"),
        "TwnNm + Ctry present must render compliant, got: {out}"
    );
}

#[test]
fn pacs003_adr_line_only_renders_missing_structured() {
    let out = mx_address_compliance(&pacs003(Some(
        "<PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr>",
    )))
    .unwrap();
    assert!(
        out.contains("debtor: missing_structured"),
        "AdrLine-only debtor must render missing_structured, got: {out}"
    );
}

#[test]
fn pain001_compliant_debtor_renders_compliant_and_message_type() {
    let out = mx_address_compliance(&pain001(Some(
        "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
    )))
    .unwrap();
    assert!(
        out.contains("message_type: pain.001.001.09"),
        "header must name the detected pain.001 type, got: {out}"
    );
    assert!(
        out.contains("debtor: compliant"),
        "TwnNm + Ctry present must render compliant, got: {out}"
    );
}

#[test]
fn pain001_no_address_debtor_renders_no_address() {
    let out = mx_address_compliance(&pain001(None)).unwrap();
    assert!(
        out.contains("debtor: no_address"),
        "debtor without PstlAdr must render no_address, got: {out}"
    );
}

#[test]
fn unparseable_mx_returns_err_not_panic() {
    let result = mx_address_compliance("not an iso 20022 envelope");
    assert!(result.is_err(), "garbage MX must error, not panic");
}

// ---------------------------------------------------------------------------
// Batch gate (`render_address_scan` + `select_xml`) — SYNTHETIC, anti-tautology
// ---------------------------------------------------------------------------
//
// The gate logic is exercised through the PURE entry points (no filesystem),
// and every verdict is computed by the real `mx_address_report` checker on
// synthetic XML, so the PASS/FAIL/ERROR partition derives from the SR2026
// TwnNm+Ctry rule rather than from a hand-built report.

/// pacs.008 with injectable debtor AND creditor `<PstlAdr>` blocks. The
/// existing `pacs008` builder only addresses the debtor (its creditor is fixed
/// and address-less, so its report can never be fully compliant); this variant
/// lets BOTH parties be made compliant. Same envelope shape; SYNTHETIC values.
fn pacs008_pair(dbtr_pstl_adr: Option<&str>, cdtr_pstl_adr: Option<&str>) -> String {
    let dbtr = match dbtr_pstl_adr {
        Some(b) => format!("          {b}\n"),
        None => String::new(),
    };
    let cdtr = match cdtr_pstl_adr {
        Some(b) => format!("          {b}\n"),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-CLI-ADR-PAIR-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>CLI-ADR-PAIR-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-CLI-ADR-PAIR-001</InstrId>
          <EndToEndId>E2E-CLI-ADR-PAIR-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000001</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="USD">1000.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr>
          <Nm>JOHN DOE</Nm>
{dbtr}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtrAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></CdtrAgt>
        <Cdtr>
          <Nm>JANE SMITH</Nm>
{cdtr}        </Cdtr>
        <CdtrAcct><Id><IBAN>GB29NWBK60161331926819</IBAN></Id></CdtrAcct>
      </CdtTrfTxInf>
    </FIToFICstmrCdtTrf>
  </Document>
</Envelope>"#
    )
}

const TWN_CTRY: &str = "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>";
const ADR_LINE_ONLY: &str = "<PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr>";

/// A `ScanEntry` whose verdict is computed by the real checker on synthetic
/// XML (not hand-built), so the gate test derives from the SR2026 rule.
fn ok_entry(label: &str, xml: &str) -> ScanEntry {
    ScanEntry {
        label: label.to_string(),
        result: mx_address_report(xml),
    }
}

/// A fully-compliant pacs.008 entry (both parties carry TwnNm + Ctry).
fn compliant_entry(label: &str) -> ScanEntry {
    let entry = ok_entry(label, &pacs008_pair(Some(TWN_CTRY), Some(TWN_CTRY)));
    // Sanity: the real checker must agree this is fully compliant, else the
    // gate tests below would be vacuous.
    let report = entry.result.as_ref().expect("compliant fixture must parse");
    assert!(
        report.all_compliant(),
        "both-party TwnNm+Ctry must be all_compliant per SR2026"
    );
    entry
}

/// A non-compliant pacs.008 entry (debtor AdrLine-only → missing_structured;
/// creditor has no PstlAdr → no_address).
fn non_compliant_entry(label: &str) -> ScanEntry {
    ok_entry(label, &pacs008(Some(ADR_LINE_ONLY)))
}

/// An entry that could not be checked at all (garbage XML → parse Err).
fn error_entry(label: &str) -> ScanEntry {
    ScanEntry {
        label: label.to_string(),
        result: mx_address_report("not an iso 20022 envelope"),
    }
}

#[test]
fn single_compliant_renders_tree_and_gate_zero() {
    let (body, gate) = render_address_scan(&[compliant_entry("a.xml")]);
    assert_eq!(gate, AddressGate::AllCompliant);
    assert_eq!(gate.code(), 0);
    // N==1 keeps the full single-file tree.
    assert!(
        body.contains("MX Address Compliance (CBPR+ SR2026)"),
        "single input must render the full tree, got: {body}"
    );
    assert!(
        body.contains("debtor: compliant") && body.contains("creditor: compliant"),
        "both parties must read compliant, got: {body}"
    );
}

#[test]
fn single_non_compliant_gate_one() {
    let (body, gate) = render_address_scan(&[non_compliant_entry("a.xml")]);
    assert_eq!(gate, AddressGate::FoundNonCompliant);
    assert_eq!(gate.code(), 1);
    assert!(
        body.contains("missing_structured"),
        "AdrLine-only debtor must render missing_structured, got: {body}"
    );
}

#[test]
fn single_error_gate_two_shows_error() {
    let (body, gate) = render_address_scan(&[error_entry("bad.xml")]);
    assert_eq!(gate, AddressGate::HadErrors);
    assert_eq!(gate.code(), 2);
    assert!(
        body.starts_with('✗') && body.contains("bad.xml"),
        "single error must render the one-line marker naming the label, got: {body}"
    );
}

#[test]
fn multi_all_compliant_gate_zero() {
    let (_body, gate) = render_address_scan(&[compliant_entry("a.xml"), compliant_entry("b.xml")]);
    assert_eq!(gate, AddressGate::AllCompliant);
    assert_eq!(gate.code(), 0);
}

#[test]
fn multi_compliant_plus_non_compliant_gate_one() {
    let (_body, gate) =
        render_address_scan(&[compliant_entry("a.xml"), non_compliant_entry("b.xml")]);
    assert_eq!(
        gate,
        AddressGate::FoundNonCompliant,
        "a clean run with one non-compliant file is exit 1"
    );
    assert_eq!(gate.code(), 1);
}

#[test]
fn multi_any_error_dominates_gate_two() {
    // Errors dominate non-compliance: a compliant + a non-compliant + an
    // errored entry must still be exit 2.
    let (_body, gate) = render_address_scan(&[
        compliant_entry("a.xml"),
        non_compliant_entry("b.xml"),
        error_entry("c.xml"),
    ]);
    assert_eq!(gate, AddressGate::HadErrors);
    assert_eq!(gate.code(), 2);
}

#[test]
fn multi_body_has_compact_lines_and_summary_footer() {
    let (body, _gate) = render_address_scan(&[
        compliant_entry("a.xml"),
        non_compliant_entry("b.xml"),
        error_entry("c.xml"),
    ]);
    // Compact per-file lines (N>1), one status token each.
    assert!(body.contains("PASS"), "compliant file → PASS, got: {body}");
    assert!(
        body.contains("FAIL"),
        "non-compliant file → FAIL, got: {body}"
    );
    assert!(body.contains("ERROR"), "errored file → ERROR, got: {body}");
    assert!(
        body.contains("a.xml") && body.contains("b.xml") && body.contains("c.xml"),
        "every label must appear, got: {body}"
    );
    assert!(
        body.contains("pacs.008.001.08"),
        "checkable files must name their message type, got: {body}"
    );
    // Summary footer: counts derived from the partition (1/1/1 of 3).
    assert!(
        body.contains("scanned 3 · compliant 1 · non-compliant 1 · errors 1"),
        "summary footer counts must be correct, got: {body}"
    );
    // Scope footer carries the SR2026 deadline (static, deterministic).
    assert!(
        body.contains("2026-11-14") && body.contains("NOT a certification"),
        "scope footer must cite the deadline and disclaim certification, got: {body}"
    );
}

#[test]
fn select_xml_filters_non_xml_and_sorts() {
    // Mixed case extensions are kept; non-.xml dropped; result sorted so a
    // directory scan is order-deterministic regardless of read_dir order.
    let got = select_xml(vec![
        "dir/b.XML".to_string(),
        "dir/a.xml".to_string(),
        "dir/notes.txt".to_string(),
        "dir/c.Xml".to_string(),
        "dir/README".to_string(),
    ]);
    assert_eq!(
        got,
        vec![
            "dir/a.xml".to_string(),
            "dir/b.XML".to_string(),
            "dir/c.Xml".to_string(),
        ],
        "only *.xml (case-insensitive) kept, and sorted"
    );
}

#[test]
fn select_xml_empty_when_no_xml() {
    let got = select_xml(vec!["a.txt".to_string(), "b.json".to_string()]);
    assert!(
        got.is_empty(),
        "no .xml inputs must yield an empty selection (the binary fails loud on this), got: {got:?}"
    );
}
