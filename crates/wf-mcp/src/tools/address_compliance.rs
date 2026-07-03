//! `wf_mx_address_compliance` tool: structural CBPR+ SR2026 postal-address
//! compliance check for pacs.008.001.08, pacs.004.001.09, pacs.003.001.08
//! and pain.001.001.09.
//!
//! This is a **presence check**, not a converter and not a full validator.
//! It auto-detects the message type and reports whether the debtor and
//! creditor postal addresses carry `TwnNm` and `Ctry` in dedicated
//! structured fields, as required by CBPR+ SR2026 (mandatory 2026-11-14).

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use wf_xform::check_mx_address;

/// Honest scope statement: single source of truth reused in the JSON `note`
/// field and the `#[tool(description=...)]` on the server method.
pub const SCOPE_NOTE: &str =
    "Structural CBPR+ SR2026 address-compliance check: verifies a pacs.008.001.08, \
     pacs.004.001.09, pacs.003.001.08 or pain.001.001.09 debtor/creditor postal address \
     carries Town Name (TwnNm) and Country (Ctry) in dedicated structured fields \
     (mandatory 2026-11-14). This is a presence check against that one rule, NOT a full \
     CBPR+ validation and NOT a certification.";

/// Request: the raw ISO 20022 MX XML envelope to check.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Raw ISO 20022 MX XML — a full `<Envelope>` wrapping `<AppHdr>` +
    /// `<Document>`. Must be a pacs.008.001.08, pacs.004.001.09,
    /// pacs.003.001.08 or pain.001.001.09 message.
    pub mx: String,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let mx = wf_mx::WfMx::from_xml(&req.mx).map_err(|e| e.to_string())?;
    let report = check_mx_address(&mx).map_err(|e| e.to_string())?;
    let report_json = report.to_json();
    Ok(json!({
        "note": SCOPE_NOTE,
        "message_type": report_json["message_type"],
        "compliant": report_json["compliant"],
        "rows": report_json["rows"],
    }))
}

// ---------------------------------------------------------------------------
// In-file tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Build a minimal pacs.008 envelope with an optional debtor PstlAdr block.
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
    <BizMsgIdr>MSG-MCP-ADR-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>MCP-ADR-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-MCP-ADR-001</InstrId>
          <EndToEndId>E2E-MCP-ADR-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000002</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="EUR">500.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr>
          <Nm>ACME CORP</Nm>
{pstl}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtrAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></CdtrAgt>
        <Cdtr><Nm>BETA LTD</Nm></Cdtr>
        <CdtrAcct><Id><IBAN>GB29NWBK60161331926819</IBAN></Id></CdtrAcct>
      </CdtTrfTxInf>
    </FIToFICstmrCdtTrf>
  </Document>
</Envelope>"#
        )
    }

    fn debtor_row(v: &Value) -> &Value {
        v["rows"]
            .as_array()
            .expect("rows array")
            .iter()
            .find(|r| r["party"] == "debtor")
            .expect("debtor row")
    }

    /// TwnNm + Ctry → compliant verdict and note present.
    #[test]
    fn compliant_debtor_reports_compliant_verdict() {
        let req = Request {
            mx: pacs008(Some(
                "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
            )),
        };
        let v = handle(req).expect("handle must succeed");
        let row = debtor_row(&v);
        assert_eq!(row["verdict"], "compliant");
        assert!(
            row["remediation"].is_null(),
            "compliant row must carry null remediation: {row:?}"
        );
        let note = v["note"].as_str().expect("note string");
        assert!(note.contains("SR2026"), "note must cite SR2026");
        assert!(
            note.contains("NOT a certification"),
            "note must disclaim certification"
        );
    }

    /// AdrLine only → missing_structured, unstructured_lines matches, and a
    /// non-null remediation string is present.
    #[test]
    fn adr_line_only_reports_missing_structured() {
        let req = Request {
            mx: pacs008(Some("<PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr>")),
        };
        let v = handle(req).expect("handle must succeed");
        let row = debtor_row(&v);
        assert_eq!(row["verdict"], "missing_structured");
        assert_eq!(row["unstructured_lines"], 1);
        assert!(
            row["remediation"].is_string(),
            "missing_structured row must carry remediation: {row:?}"
        );
    }

    /// No PstlAdr → no_address verdict.
    #[test]
    fn no_postal_address_reports_no_address() {
        let req = Request { mx: pacs008(None) };
        let v = handle(req).expect("handle must succeed");
        let row = debtor_row(&v);
        assert_eq!(row["verdict"], "no_address");
        assert_eq!(row["unstructured_lines"], 0);
    }

    /// Unparseable input → Err not panic.
    #[test]
    fn unparseable_mx_returns_error_not_panic() {
        let req = Request {
            mx: "not xml".to_string(),
        };
        assert!(handle(req).is_err(), "bad XML must produce an Err");
    }

    /// pacs.008 output carries the detected message_type.
    #[test]
    fn pacs008_output_reports_message_type() {
        let req = Request {
            mx: pacs008(Some(
                "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
            )),
        };
        let v = handle(req).expect("handle must succeed");
        assert_eq!(v["message_type"], "pacs.008.001.08");
    }

    // -----------------------------------------------------------------------
    // pacs.004 (Payment Return) — SYNTHETIC fixtures
    // -----------------------------------------------------------------------

    /// Build a minimal pacs.004.001.09 (PmtRtr) envelope. `dbtr_inner` is
    /// injected verbatim inside `<Dbtr>` in the return chain; the creditor is
    /// a fixed agent-only party (no postal address).
    fn pacs004(dbtr_inner: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-MCP-RTR-001</BizMsgIdr>
    <MsgDefIdr>pacs.004.001.09</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <PmtRtr>
      <GrpHdr>
        <MsgId>MCP-RTR-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <TxInf>
        <OrgnlEndToEndId>E2E-RTR-001</OrgnlEndToEndId>
        <OrgnlUETR>00000000-0000-4000-8000-000000000002</OrgnlUETR>
        <RtrdIntrBkSttlmAmt Ccy="USD">500.00</RtrdIntrBkSttlmAmt>
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

    /// pacs.004 with TwnNm + Ctry under Dbtr/Pty/PstlAdr → compliant, and the
    /// output reports the pacs.004 message_type.
    #[test]
    fn pacs004_compliant_debtor_reports_compliant_and_message_type() {
        let req = Request {
            mx: pacs004(
                "<Pty><Nm>ACME CORP</Nm>\
                 <PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr></Pty>",
            ),
        };
        let v = handle(req).expect("pacs.004 handle must succeed");
        assert_eq!(v["message_type"], "pacs.004.001.09");
        assert_eq!(debtor_row(&v)["verdict"], "compliant");
    }

    /// pacs.004 with AdrLine-only under Dbtr/Pty/PstlAdr → missing_structured.
    #[test]
    fn pacs004_adr_line_only_reports_missing_structured() {
        let req = Request {
            mx: pacs004(
                "<Pty><Nm>ACME CORP</Nm>\
                 <PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr></Pty>",
            ),
        };
        let v = handle(req).expect("pacs.004 handle must succeed");
        let row = debtor_row(&v);
        assert_eq!(row["verdict"], "missing_structured");
        assert_eq!(row["unstructured_lines"], 1);
    }

    /// pacs.004 agent-only debtor (no `Pty`) → no_address.
    #[test]
    fn pacs004_agent_only_debtor_reports_no_address() {
        let req = Request {
            mx: pacs004("<Agt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></Agt>"),
        };
        let v = handle(req).expect("pacs.004 handle must succeed");
        assert_eq!(debtor_row(&v)["verdict"], "no_address");
    }

    // -----------------------------------------------------------------------
    // pacs.003 (FIToFICstmrDrctDbt) — SYNTHETIC fixtures
    // -----------------------------------------------------------------------

    /// Build a minimal pacs.003.001.08 (customer direct debit) envelope with an
    /// optional debtor `<PstlAdr>` block (injected after `<Nm>`). Required-field
    /// set mirrors the upstream `DirectDebitTransactionInformation241`.
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
    <BizMsgIdr>MSG-MCP-DD-001</BizMsgIdr>
    <MsgDefIdr>pacs.003.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrDrctDbt>
      <GrpHdr>
        <MsgId>MCP-DD-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <DrctDbtTxInf>
        <PmtId>
          <InstrId>INSTR-MCP-DD-001</InstrId>
          <EndToEndId>E2E-MCP-DD-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000003</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="EUR">500.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <ReqdColltnDt>2024-01-15</ReqdColltnDt>
        <Cdtr><Nm>BETA LTD</Nm></Cdtr>
        <CdtrAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></CdtrAgt>
        <InstgAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr>
          <Nm>ACME CORP</Nm>
{pstl}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
      </DrctDbtTxInf>
    </FIToFICstmrDrctDbt>
  </Document>
</Envelope>"#
        )
    }

    /// pacs.003 with TwnNm + Ctry → compliant, and the output reports the
    /// pacs.003 message_type.
    #[test]
    fn pacs003_compliant_debtor_reports_compliant_and_message_type() {
        let req = Request {
            mx: pacs003(Some(
                "<PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>",
            )),
        };
        let v = handle(req).expect("pacs.003 handle must succeed");
        assert_eq!(v["message_type"], "pacs.003.001.08");
        assert_eq!(debtor_row(&v)["verdict"], "compliant");
    }

    /// pacs.003 with AdrLine only → missing_structured.
    #[test]
    fn pacs003_adr_line_only_reports_missing_structured() {
        let req = Request {
            mx: pacs003(Some("<PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr>")),
        };
        let v = handle(req).expect("pacs.003 handle must succeed");
        let row = debtor_row(&v);
        assert_eq!(row["verdict"], "missing_structured");
        assert_eq!(row["unstructured_lines"], 1);
    }

    /// pacs.003 with no debtor PstlAdr → no_address.
    #[test]
    fn pacs003_no_postal_address_reports_no_address() {
        let req = Request { mx: pacs003(None) };
        let v = handle(req).expect("pacs.003 handle must succeed");
        assert_eq!(debtor_row(&v)["verdict"], "no_address");
    }

    // -----------------------------------------------------------------------
    // pain.001 (CstmrCdtTrfInitn) — SYNTHETIC fixtures
    // -----------------------------------------------------------------------

    /// Build a minimal pain.001.001.09 (customer credit-transfer initiation)
    /// envelope with an optional debtor `<PstlAdr>` block (injected inside
    /// `PmtInf/Dbtr` after `<Nm>`). Required-field set mirrors the upstream
    /// `PaymentInstruction301` + `CreditTransferTransaction341`.
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
    <BizMsgIdr>MSG-MCP-INI-001</BizMsgIdr>
    <MsgDefIdr>pain.001.001.09</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <CstmrCdtTrfInitn>
      <GrpHdr>
        <MsgId>MCP-INI-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <InitgPty><Nm>INIT PARTY</Nm></InitgPty>
      </GrpHdr>
      <PmtInf>
        <PmtInfId>PMT-MCP-INI-001</PmtInfId>
        <PmtMtd>TRF</PmtMtd>
        <ReqdExctnDt><Dt>2024-01-15</Dt></ReqdExctnDt>
        <Dbtr>
          <Nm>ACME CORP</Nm>
{pstl}        </Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtTrfTxInf>
          <PmtId>
            <EndToEndId>E2E-MCP-INI-001</EndToEndId>
            <UETR>00000000-0000-4000-8000-000000000004</UETR>
          </PmtId>
          <Amt><InstdAmt Ccy="EUR">500.00</InstdAmt></Amt>
          <Cdtr><Nm>BETA LTD</Nm></Cdtr>
        </CdtTrfTxInf>
      </PmtInf>
    </CstmrCdtTrfInitn>
  </Document>
</Envelope>"#
        )
    }

    /// pain.001 with TwnNm + Ctry → compliant, and the output reports the
    /// pain.001 message_type.
    #[test]
    fn pain001_compliant_debtor_reports_compliant_and_message_type() {
        let req = Request {
            mx: pain001(Some(
                "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>",
            )),
        };
        let v = handle(req).expect("pain.001 handle must succeed");
        assert_eq!(v["message_type"], "pain.001.001.09");
        assert_eq!(debtor_row(&v)["verdict"], "compliant");
    }

    /// pain.001 with AdrLine only → missing_structured.
    #[test]
    fn pain001_adr_line_only_reports_missing_structured() {
        let req = Request {
            mx: pain001(Some("<PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr>")),
        };
        let v = handle(req).expect("pain.001 handle must succeed");
        let row = debtor_row(&v);
        assert_eq!(row["verdict"], "missing_structured");
        assert_eq!(row["unstructured_lines"], 1);
    }

    /// pain.001 with no debtor PstlAdr → no_address.
    #[test]
    fn pain001_no_postal_address_reports_no_address() {
        let req = Request { mx: pain001(None) };
        let v = handle(req).expect("pain.001 handle must succeed");
        assert_eq!(debtor_row(&v)["verdict"], "no_address");
    }
}
