//! `wf_mx_address_scan` tool: batch structural CBPR+ SR2026 postal-address
//! compliance check over one-or-more ISO 20022 MX envelopes, with the same
//! diff-style gate/exit-code convention as `wf xform address-check --scan`
//! on the CLI surface.
//!
//! This is a **presence check**, not a converter and not a full validator —
//! see [`super::address_compliance::SCOPE_NOTE`] for the per-message scope.
//! This module only adds batching and the aggregate gate on top.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

/// Honest scope statement for the batch variant: single source of truth
/// reused in the JSON `note` field and the `#[tool(description=...)]` on the
/// server method.
pub const SCOPE_NOTE: &str =
    "Batch structural CBPR+ SR2026 address-compliance check: runs the same presence check as \
     wf_mx_address_compliance (Town Name / Ctry in dedicated structured fields, mandatory \
     2026-11-14) over one or more pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 or \
     pain.001.001.09 envelopes, auto-detecting each message's type. Returns a diff-style gate \
     and exit_code summarizing the whole batch. This is a presence check against that one \
     SR2026 rule, NOT a full CBPR+ validation and NOT a certification.";

/// One message to scan: an optional display label and the raw MX XML.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScanInput {
    /// Display label for this message in the results (e.g. a file name or
    /// sequence id). Defaults to the message's 1-based position in
    /// `messages` (as a string) when omitted.
    pub label: Option<String>,
    /// Raw ISO 20022 MX XML — a full `<Envelope>` wrapping `<AppHdr>` +
    /// `<Document>`. Must be a pacs.008.001.08, pacs.004.001.09,
    /// pacs.003.001.08 or pain.001.001.09 message.
    pub mx: String,
}

/// Request: the batch of MX envelopes to scan.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// One or more messages to scan. Must be non-empty.
    pub messages: Vec<ScanInput>,
}

pub fn handle(req: Request) -> Result<Value, String> {
    if req.messages.is_empty() {
        return Err("wf_mx_address_scan requires a non-empty `messages` array".to_string());
    }

    let mut results = Vec::with_capacity(req.messages.len());
    let mut compliant = 0usize;
    let mut non_compliant = 0usize;
    let mut errors = 0usize;

    for (idx, input) in req.messages.iter().enumerate() {
        let label = input
            .label
            .clone()
            .unwrap_or_else(|| (idx + 1).to_string());
        match scan_one(&input.mx) {
            Ok(report_json) => {
                if report_json["compliant"] == json!(true) {
                    compliant += 1;
                } else {
                    non_compliant += 1;
                }
                let mut entry = report_json;
                if let Some(map) = entry.as_object_mut() {
                    map.insert("label".to_string(), json!(label));
                    map.insert("status".to_string(), json!("ok"));
                }
                results.push(entry);
            }
            Err(err) => {
                errors += 1;
                results.push(json!({
                    "label": label,
                    "status": "error",
                    "error": err,
                }));
            }
        }
    }

    let (gate, exit_code) = if errors > 0 {
        ("had_errors", 2)
    } else if non_compliant > 0 {
        ("found_non_compliant", 1)
    } else {
        ("all_compliant", 0)
    };

    Ok(json!({
        "schema_version": "1.0",
        "note": SCOPE_NOTE,
        "gate": gate,
        "exit_code": exit_code,
        "summary": {
            "scanned": req.messages.len(),
            "compliant": compliant,
            "non_compliant": non_compliant,
            "errors": errors,
        },
        "results": results,
    }))
}

/// Parse + check one MX envelope, returning the shared wire JSON shape from
/// `AddressComplianceReport::to_json()` (`message_type`, `compliant`, `rows`).
fn scan_one(mx: &str) -> Result<Value, String> {
    let parsed = wf_mx::WfMx::from_xml(mx).map_err(|e| e.to_string())?;
    let report = wf_xform::check_mx_address(&parsed).map_err(|e| e.to_string())?;
    Ok(report.to_json())
}

// ---------------------------------------------------------------------------
// In-file tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // pacs.008 batch-scan fixtures — SYNTHETIC

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
    <BizMsgIdr>MSG-MCP-SCAN-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>MCP-SCAN-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-MCP-SCAN-001</InstrId>
          <EndToEndId>E2E-MCP-SCAN-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000005</UETR>
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
        <Cdtr>
          <Nm>BETA LTD</Nm>
          <PstlAdr><TwnNm>LONDON</TwnNm><Ctry>GB</Ctry></PstlAdr>
        </Cdtr>
        <CdtrAcct><Id><IBAN>GB29NWBK60161331926819</IBAN></Id></CdtrAcct>
      </CdtTrfTxInf>
    </FIToFICstmrCdtTrf>
  </Document>
</Envelope>"#
        )
    }

    const COMPLIANT_PSTL: &str = "<PstlAdr><TwnNm>BERLIN</TwnNm><Ctry>DE</Ctry></PstlAdr>";
    const MISSING_STRUCTURED_PSTL: &str = "<PstlAdr><AdrLine>1 HIGH ST</AdrLine></PstlAdr>";

    /// A batch of {compliant, missing_structured, garbage} yields the right
    /// per-entry statuses, summary counts, and gate/exit_code 2 (errors
    /// dominate).
    #[test]
    fn mixed_batch_yields_had_errors_gate_and_per_entry_statuses() {
        let req = Request {
            messages: vec![
                ScanInput {
                    label: Some("compliant-one".to_string()),
                    mx: pacs008(Some(COMPLIANT_PSTL)),
                },
                ScanInput {
                    label: Some("missing-structured-one".to_string()),
                    mx: pacs008(Some(MISSING_STRUCTURED_PSTL)),
                },
                ScanInput {
                    label: None,
                    mx: "not xml".to_string(),
                },
            ],
        };
        let v = handle(req).expect("handle must succeed");
        assert_eq!(v["gate"], "had_errors");
        assert_eq!(v["exit_code"], 2);
        assert_eq!(v["summary"]["scanned"], 3);
        assert_eq!(v["summary"]["compliant"], 1);
        assert_eq!(v["summary"]["non_compliant"], 1);
        assert_eq!(v["summary"]["errors"], 1);

        let results = v["results"].as_array().expect("results array");
        assert_eq!(results.len(), 3);

        assert_eq!(results[0]["label"], "compliant-one");
        assert_eq!(results[0]["status"], "ok");
        assert_eq!(results[0]["compliant"], true);
        assert_eq!(results[0]["message_type"], "pacs.008.001.08");
        assert!(results[0]["rows"].is_array());

        assert_eq!(results[1]["label"], "missing-structured-one");
        assert_eq!(results[1]["status"], "ok");
        assert_eq!(results[1]["compliant"], false);

        // Unlabeled third entry defaults its label to its 1-based index.
        assert_eq!(results[2]["label"], "3");
        assert_eq!(results[2]["status"], "error");
        assert!(results[2]["error"].is_string());
    }

    /// A batch of all-compliant messages yields all_compliant/0.
    #[test]
    fn all_compliant_batch_yields_all_compliant_gate() {
        let req = Request {
            messages: vec![
                ScanInput {
                    label: Some("a".to_string()),
                    mx: pacs008(Some(COMPLIANT_PSTL)),
                },
                ScanInput {
                    label: Some("b".to_string()),
                    mx: pacs008(Some(COMPLIANT_PSTL)),
                },
            ],
        };
        let v = handle(req).expect("handle must succeed");
        assert_eq!(v["gate"], "all_compliant");
        assert_eq!(v["exit_code"], 0);
        assert_eq!(v["summary"]["scanned"], 2);
        assert_eq!(v["summary"]["compliant"], 2);
        assert_eq!(v["summary"]["non_compliant"], 0);
        assert_eq!(v["summary"]["errors"], 0);
    }

    /// A batch with non-compliant but no errors yields found_non_compliant/1.
    #[test]
    fn non_compliant_without_errors_yields_found_non_compliant_gate() {
        let req = Request {
            messages: vec![
                ScanInput {
                    label: Some("a".to_string()),
                    mx: pacs008(Some(COMPLIANT_PSTL)),
                },
                ScanInput {
                    label: Some("b".to_string()),
                    mx: pacs008(Some(MISSING_STRUCTURED_PSTL)),
                },
            ],
        };
        let v = handle(req).expect("handle must succeed");
        assert_eq!(v["gate"], "found_non_compliant");
        assert_eq!(v["exit_code"], 1);
    }

    /// Empty batch → Err, not a panic.
    #[test]
    fn empty_batch_returns_error_not_panic() {
        let req = Request { messages: vec![] };
        assert!(handle(req).is_err(), "empty messages must produce an Err");
    }
}
