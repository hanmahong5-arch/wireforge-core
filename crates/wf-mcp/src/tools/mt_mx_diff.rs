//! `wf_mt_mx_truncation_diff` tool: detect field truncation / loss between
//! a SWIFT MT103 and an ISO 20022 pacs.008.001.08.
//!
//! This is a **detector**, not a converter. It parses the two messages the
//! caller already holds, compares a fixed set of business roles, and
//! reports per role whether the MX value would fit the MT field. It does
//! NOT convert MT to MX or MX to MT and makes NO certification,
//! conformance, or equivalence claim.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use wf_xform::{diff_mt_mx, DiffReport, FieldDiff};

/// The honest scope statement reused in the tool description, the JSON
/// `note` field, and (mirrored) the CLI help — a single source of truth so
/// the three surfaces never drift.
pub const SCOPE_NOTE: &str = "DETECTS field truncation and loss between a SWIFT MT103 (ISO 15022) \
and an ISO 20022 pacs.008.001.08. This is a DETECTOR, not a converter: it does not convert MT to \
MX or MX to MT, and makes no certification, conformance, or equivalence claim. Coverage is limited \
to pacs.008.001.08 vs MT103 across five roles only: debtor name, creditor name, remittance info, \
settlement amount, settlement currency.";

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Pair mode — raw SWIFT MT103 wire text (the `{1:…}{2:…}{4:…-}` FIN
    /// format). Supply `mt` AND `mx` together, OR supply `wf` alone.
    pub mt: Option<String>,
    /// Pair mode — raw ISO 20022 MX XML, a full envelope (`<AppHdr>` +
    /// `<Document>`), not a bare `<Document>`. Supply `mt` AND `mx`
    /// together, OR supply `wf` alone.
    pub mx: Option<String>,
    /// Single-file mode — a `.wf` source string holding a matched
    /// `swift-mt` + `mx` pair. When set, `mt` / `mx` must be omitted; the
    /// MT wire and MX envelope are reconstructed from the `.wf` file.
    pub wf: Option<String>,
}

pub fn handle(req: Request) -> Result<Value, String> {
    // Resolve the two raw inputs from whichever mode the caller used:
    // either a `.wf` pair source, or an explicit (mt, mx) pair.
    let (mt_src, mx_src) = match req.wf {
        Some(wf_src) => {
            if req.mt.is_some() || req.mx.is_some() {
                return Err(
                    "`wf` cannot be combined with `mt` / `mx`; supply either a single `.wf` pair \
                     source via `wf`, or both `mt` and `mx`, not both"
                        .to_string(),
                );
            }
            let file = wf_format::parse(&wf_src).map_err(|e| e.to_string())?;
            wf_format::extract_mt_mx_pair(&file).map_err(|e| e.to_string())?
        }
        None => match (req.mt, req.mx) {
            (Some(mt), Some(mx)) => (mt, mx),
            _ => {
                return Err(
                    "expected either both `mt` and `mx`, or a single `wf` pair source; got an \
                     incomplete request; supply both `mt` and `mx`, or set `wf`"
                        .to_string(),
                );
            }
        },
    };

    // Parse the MT side via the wf-swift facade; its error already carries
    // the three-element (what / expected / recourse) message, so surface it
    // as-is rather than re-wrapping the raw upstream type.
    let mt = wf_swift::parse(&mt_src).map_err(|e| e.to_string())?;
    // Parse the MX side via the wf-mx facade (full-envelope requirement is
    // explained in that facade's error message).
    let mx = wf_mx::WfMx::from_xml(&mx_src).map_err(|e| e.to_string())?;
    // Compare; the detector's own error is likewise three-element.
    let report = diff_mt_mx(&mt, &mx).map_err(|e| e.to_string())?;

    Ok(json!({
        "note": SCOPE_NOTE,
        "roles": roles_json(&report),
    }))
}

/// Serialise each role row to `{ role, verdict, … }` with only the payload
/// relevant to its verdict.
pub fn roles_json(report: &DiffReport) -> Vec<Value> {
    report
        .rows
        .iter()
        .map(|r| {
            let mut obj = json!({
                "role": r.role.as_str(),
                "verdict": verdict_str(&r.diff),
            });
            // `obj` is constructed as a JSON object literal above, so
            // `as_object_mut` is always `Some`; if it somehow were not, we
            // simply emit the verdict without extra payload rather than
            // panicking.
            if let Some(map) = obj.as_object_mut() {
                match &r.diff {
                    FieldDiff::Truncated { lost_suffix } => {
                        map.insert("lost_suffix".into(), json!(lost_suffix));
                        map.insert("lost_chars".into(), json!(lost_suffix.chars().count()));
                    }
                    FieldDiff::Dropped => {
                        if let Some(v) = &r.mx_value {
                            map.insert("mx".into(), json!(v));
                        }
                    }
                    FieldDiff::Added => {
                        if let Some(v) = &r.mt_value {
                            map.insert("mt".into(), json!(v));
                        }
                    }
                    FieldDiff::Mismatch | FieldDiff::Reformatted => {
                        if let Some(v) = &r.mt_value {
                            map.insert("mt".into(), json!(v));
                        }
                        if let Some(v) = &r.mx_value {
                            map.insert("mx".into(), json!(v));
                        }
                    }
                    FieldDiff::Equal => {}
                    // Both sides absent: nothing to compare, so emit no
                    // extra payload (like `Equal`).
                    FieldDiff::BothAbsent => {}
                }
            }
            obj
        })
        .collect()
}

/// Stable lowercase verdict label for a [`FieldDiff`].
pub fn verdict_str(diff: &FieldDiff) -> &'static str {
    match diff {
        FieldDiff::Equal => "equal",
        FieldDiff::Reformatted => "reformatted",
        FieldDiff::Truncated { .. } => "truncated",
        FieldDiff::Dropped => "dropped",
        FieldDiff::Added => "added",
        FieldDiff::Mismatch => "mismatch",
        FieldDiff::BothAbsent => "absent_both",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Build a pacs.008 envelope with a given debtor name. Same envelope
    /// shape the wf-mx / wf-xform crates use in their own tests.
    fn pacs008(dbtr_nm: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-MCP-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>MCP-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-MCP-001</InstrId>
          <EndToEndId>E2E-MCP-001</EndToEndId>
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
             :20:REF-MCP-001\r\n\
             :23B:CRED\r\n\
             :32A:240115USD1234,56\r\n\
             :50K:{name_50k}\r\n\
             :59:JANE SMITH\r\n\
             :71A:OUR\r\n\
             -}}"
        )
    }

    #[test]
    fn equal_debtor_name_reports_equal_verdict() {
        let req = Request {
            mt: Some(mt103("JOHN DOE")),
            mx: Some(pacs008("JOHN DOE")),
            wf: None,
        };
        let v = handle(req).expect("diff");
        let roles = v["roles"].as_array().expect("roles array");
        let debtor = roles
            .iter()
            .find(|r| r["role"] == "debtor_name")
            .expect("debtor role present");
        assert_eq!(debtor["verdict"], "equal");
    }

    #[test]
    fn over_cap_debtor_name_reports_truncated_with_lost_suffix() {
        // MX Nm 141 chars vs MT 50K 140 cap -> exactly one char lost.
        let mt_name = "A".repeat(140);
        let mx_name = format!("{mt_name}Z");
        let req = Request {
            mt: Some(mt103(&mt_name)),
            mx: Some(pacs008(&mx_name)),
            wf: None,
        };
        let v = handle(req).expect("diff");
        let roles = v["roles"].as_array().expect("roles array");
        let debtor = roles
            .iter()
            .find(|r| r["role"] == "debtor_name")
            .expect("debtor role present");
        assert_eq!(debtor["verdict"], "truncated");
        assert_eq!(debtor["lost_suffix"], "Z");
        assert_eq!(debtor["lost_chars"], 1);
    }

    #[test]
    fn both_absent_role_serialises_with_absent_both_verdict() {
        // The helper MT/MX have no field 70 and no RmtInf, so the
        // remittance role is absent on both sides -> verdict "absent_both"
        // (not "mismatch") and no extra payload.
        let req = Request {
            mt: Some(mt103("JOHN DOE")),
            mx: Some(pacs008("JOHN DOE")),
            wf: None,
        };
        let v = handle(req).expect("diff");
        let roles = v["roles"].as_array().expect("roles array");
        let remittance = roles
            .iter()
            .find(|r| r["role"] == "remittance_info")
            .expect("remittance role present");
        assert_eq!(remittance["verdict"], "absent_both");
        assert!(
            remittance.get("mt").is_none() && remittance.get("mx").is_none(),
            "a both-absent role must carry no value payload: {remittance}"
        );
    }

    #[test]
    fn note_states_detector_not_converter_scope() {
        let req = Request {
            mt: Some(mt103("JOHN DOE")),
            mx: Some(pacs008("JOHN DOE")),
            wf: None,
        };
        let v = handle(req).expect("diff");
        let note = v["note"].as_str().expect("note string");
        assert!(note.contains("DETECTOR"), "note must say DETECTOR: {note}");
        assert!(
            note.contains("not a converter"),
            "note must disclaim conversion: {note}"
        );
        assert!(
            note.contains("no certification"),
            "note must disclaim certification: {note}"
        );
        assert!(
            note.contains("pacs.008.001.08") && note.contains("MT103"),
            "note must state the exact pair: {note}"
        );
    }

    #[test]
    fn unparseable_mt_returns_error_not_panic() {
        let req = Request {
            mt: Some("not a swift message".to_string()),
            mx: Some(pacs008("JOHN DOE")),
            wf: None,
        };
        assert!(handle(req).is_err());
    }

    #[test]
    fn document_only_mx_returns_error_not_panic() {
        let req = Request {
            mt: Some(mt103("JOHN DOE")),
            mx: Some("<Document></Document>".to_string()),
            wf: None,
        };
        assert!(handle(req).is_err());
    }

    /// Build a `.wf` pair source: a `swift-mt` block with 50K = `mt_name`
    /// and an `mx` block carrying the single-line pacs.008 envelope with
    /// Dbtr/Nm = `mx_name`.
    fn wf_pair(mt_name: &str, mx_name: &str) -> String {
        let mx_one_line = pacs008(mx_name).replace('\n', "");
        format!(
            "meta {{\n  name: Pair\n  type: pair\n}}\n\
             swift-mt {{\n\
               block 1: F01BANKUS33AXXX0000000000\n\
               block 2: I103BANKGB22XXXXN\n\
               block 4 {{\n\
                 field 20: REF-MCP-WF-001\n\
                 field 23B: CRED\n\
                 field 32A: 240115USD1234,56\n\
                 field 50K: {mt_name}\n\
                 field 59: JANE SMITH\n\
                 field 71A: OUR\n\
               }}\n\
             }}\n\
             mx {{\n\
               xml: {mx_one_line}\n\
             }}\n"
        )
    }

    #[test]
    fn wf_pair_source_over_cap_reports_truncated_debtor() {
        // 141-char MX name vs the MT 140 cap -> exactly one char lost.
        let mt_name = "A".repeat(140);
        let mx_name = format!("{mt_name}Z");
        let req = Request {
            mt: None,
            mx: None,
            wf: Some(wf_pair(&mt_name, &mx_name)),
        };
        let v = handle(req).expect("diff");
        let roles = v["roles"].as_array().expect("roles array");
        let debtor = roles
            .iter()
            .find(|r| r["role"] == "debtor_name")
            .expect("debtor role present");
        assert_eq!(debtor["verdict"], "truncated");
        assert_eq!(debtor["lost_suffix"], "Z");
        assert_eq!(debtor["lost_chars"], 1);
    }

    #[test]
    fn wf_and_explicit_pair_combined_is_an_error() {
        let req = Request {
            mt: Some(mt103("JOHN DOE")),
            mx: Some(pacs008("JOHN DOE")),
            wf: Some(wf_pair("JOHN DOE", "JOHN DOE")),
        };
        assert!(
            handle(req).is_err(),
            "mixing `wf` with `mt`/`mx` must be rejected"
        );
    }

    #[test]
    fn empty_request_is_an_error() {
        let req = Request {
            mt: None,
            mx: None,
            wf: None,
        };
        assert!(handle(req).is_err(), "a request with no inputs must error");
    }
}
