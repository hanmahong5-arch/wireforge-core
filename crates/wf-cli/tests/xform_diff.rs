//! Integration tests for the `wf xform diff` entry point.
//!
//! These call the pure lib.rs entry point directly (the binary is a thin
//! file-reading dispatcher over it). The truncation expectation is anchored
//! to the CITED caps, not to the detector's classifier: MT 50K holds
//! 4*35 = 140 chars and the MX Dbtr/Nm maxLength is 140 (mx-message 3.1.4),
//! so a 141-char MX name loses exactly its 141st char against the MT field.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_cli::{mt_mx_truncation_diff, mt_mx_truncation_diff_from_wf};

/// Build a pacs.008 envelope with the given debtor name. Same envelope
/// shape the wf-mx / wf-xform crates use in their own tests.
fn pacs008(dbtr_nm: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-CLI-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>CLI-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-CLI-001</InstrId>
          <EndToEndId>E2E-CLI-001</EndToEndId>
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
         :20:REF-CLI-001\r\n\
         :23B:CRED\r\n\
         :32A:240115USD1234,56\r\n\
         :50K:{name_50k}\r\n\
         :59:JANE SMITH\r\n\
         :71A:OUR\r\n\
         -}}"
    )
}

#[test]
fn output_header_states_detector_scope() {
    let out = mt_mx_truncation_diff(&mt103("JOHN DOE"), &pacs008("JOHN DOE")).unwrap();
    assert!(
        out.contains("DETECTOR (not a converter)"),
        "header must frame this as a detector, got: {out}"
    );
    assert!(
        out.contains("MT103 vs pacs.008.001.08"),
        "header must name the exact pair, got: {out}"
    );
    assert!(
        out.contains("No certification"),
        "header must disclaim certification, got: {out}"
    );
}

#[test]
fn equal_debtor_name_renders_equal_verdict() {
    let out = mt_mx_truncation_diff(&mt103("JOHN DOE"), &pacs008("JOHN DOE")).unwrap();
    assert!(
        out.contains("debtor_name: equal"),
        "matching names must render as equal, got: {out}"
    );
}

#[test]
fn over_cap_debtor_name_renders_truncated_with_lost_char() {
    // 141-char MX name vs the MT 140 cap -> exactly the trailing 'Z' lost.
    let mt_name = "A".repeat(140);
    let mx_name = format!("{mt_name}Z");
    let out = mt_mx_truncation_diff(&mt103(&mt_name), &pacs008(&mx_name)).unwrap();
    assert!(
        out.contains("debtor_name: truncated"),
        "141-char name over the 140 cap must render as truncated, got: {out}"
    );
    assert!(
        out.contains("lost 1 char"),
        "exactly one lost char must be reported, got: {out}"
    );
    assert!(
        out.contains("\"Z\""),
        "the lost suffix must be the trailing 'Z', got: {out}"
    );
}

#[test]
fn unparseable_mt_returns_err_not_panic() {
    let result = mt_mx_truncation_diff("not a swift message", &pacs008("JOHN DOE"));
    assert!(result.is_err(), "garbage MT must error");
    assert!(
        result.unwrap_err().contains("SWIFT MT"),
        "error should explain the MT parse failure"
    );
}

#[test]
fn document_only_mx_returns_err_not_panic() {
    let result = mt_mx_truncation_diff(&mt103("JOHN DOE"), "<Document></Document>");
    assert!(result.is_err(), "bare <Document> MX must error");
}

/// Build a `.wf` pair source holding a `swift-mt` block (50K = `mt_name`)
/// and an `mx` block (single-line pacs.008 with Dbtr/Nm = `mx_name`). The
/// MX envelope is the same one-line shape `pacs008` produces; the XML uses
/// `<`/`>` only, so the `.wf` value reader carries it verbatim.
fn wf_pair(mt_name: &str, mx_name: &str) -> String {
    let mx_one_line = pacs008(mx_name).replace('\n', "");
    format!(
        "meta {{\n  name: Pair\n  type: pair\n}}\n\
         swift-mt {{\n\
           block 1: F01BANKUS33AXXX0000000000\n\
           block 2: I103BANKGB22XXXXN\n\
           block 4 {{\n\
             field 20: REF-WF-001\n\
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
fn wf_pair_source_over_cap_renders_truncated_debtor_row() {
    // 141-char MX name vs the MT 140 cap -> the trailing 'Z' is lost, so
    // the single-file `.wf` path must surface a truncated debtor row.
    let mt_name = "A".repeat(140);
    let mx_name = format!("{mt_name}Z");
    let out = mt_mx_truncation_diff_from_wf(&wf_pair(&mt_name, &mx_name)).unwrap();
    assert!(
        out.contains("debtor_name: truncated"),
        "wf-source 141-char name over the 140 cap must render as truncated, got: {out}"
    );
    assert!(
        out.contains("lost 1 char"),
        "exactly one lost char must be reported, got: {out}"
    );
}

#[test]
fn wf_pair_source_equal_debtor_renders_equal_row() {
    let out = mt_mx_truncation_diff_from_wf(&wf_pair("JOHN DOE", "JOHN DOE")).unwrap();
    assert!(
        out.contains("debtor_name: equal"),
        "matching names from a `.wf` pair must render as equal, got: {out}"
    );
}

#[test]
fn example_pair_file_renders_truncated_debtor_row() {
    // The committed example is engineered (140-char MT name + 'Z' in MX)
    // to demonstrate a truncated debtor role end-to-end.
    let wf_src = include_str!("../../wf-format/examples/mt-mx-pair.wf");
    let out = mt_mx_truncation_diff_from_wf(wf_src).unwrap();
    assert!(
        out.contains("debtor_name: truncated"),
        "the example pair file must render a truncated debtor row, got: {out}"
    );
}
