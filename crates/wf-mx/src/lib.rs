//! Wireforge ISO 20022 (MX) facade.
//!
//! This crate is the MX counterpart to the SWIFT MT facade. It wraps a
//! vetted third-party ISO 20022 / CBPR+ message library and presents a
//! Wireforge-owned surface so callers never depend on the upstream
//! crate's error type directly.
//!
//! Unlike the MT facade — which has a structural fallback for unsupported
//! message types — there is no type-agnostic structural parser for MX
//! here: ISO 20022 messages are schema-typed XML, and the upstream parser
//! either recognises the `Document` body (one of ~25 supported message
//! types) or it does not. [`WfMx`] is therefore a single wrapper, not a
//! tagged enum.
//!
//! ## Inbound shape (important)
//!
//! [`WfMx::from_xml`] requires a **full ISO 20022 envelope**: an
//! `<AppHdr>` Business Application Header followed by a `<Document>`. The
//! message type is taken from the header's `MsgDefIdr` (e.g.
//! `pacs.008.001.08`). A bare `<Document>` with no header is rejected —
//! the upstream parser needs the header to classify the body. Callers
//! holding document-only XML must wrap it in an envelope first.
//!
//! ## Coupling note (deliberate)
//!
//! [`WfMx::document`] exposes the upstream [`Document`] enum directly.
//! That enum carries ~25 fully-typed message bodies (pacs.008, camt.053,
//! pain.001, …); re-wrapping every one behind a Wireforge-owned mirror
//! would duplicate the entire typed surface for no behavioural gain — the
//! *whole point* of the facade is to hand callers that rich typed body.
//! We therefore re-export it from this crate ([`Document`]) and accept the
//! coupling consciously. Everything else (the wrapper, the error type, the
//! accessors) is Wireforge-owned, so callers that only need "did it parse,
//! and what type is it" never touch the upstream types.

use std::fmt;

// Re-exported on purpose: the typed body is the value of the facade.
// See the module-level "Coupling note".
pub use mx_message::mx_envelope::Document;

/// A parsed ISO 20022 (MX) message.
///
/// Wraps the upstream parsed envelope (Business Application Header plus a
/// typed [`Document`] body). Construct one with [`WfMx::from_xml`]; read
/// its type with [`WfMx::message_type`]; reach the typed body with
/// [`WfMx::document`]; re-serialise with [`WfMx::to_xml`] /
/// [`WfMx::to_json`].
#[derive(Debug, Clone, PartialEq)]
pub struct WfMx {
    inner: mx_message::MxMessage,
}

impl WfMx {
    /// Parse a full ISO 20022 envelope (`<AppHdr>` + `<Document>`) into a
    /// typed message.
    ///
    /// The message type is detected from the header's `MsgDefIdr` and the
    /// document body from its first child element. Returns [`WfMxError`]
    /// if the input is not a well-formed, supported envelope (including
    /// the document-only case — see the module-level "Inbound shape"
    /// note).
    ///
    /// This never panics on caller input: every failure mode is a
    /// `Result`.
    pub fn from_xml(xml: &str) -> Result<WfMx, WfMxError> {
        mx_message::MxMessage::from_xml(xml)
            .map(|inner| WfMx { inner })
            .map_err(|e| WfMxError::inbound(&e))
    }

    /// The ISO 20022 message-type identifier from the header, e.g.
    /// `"pacs.008.001.08"`.
    ///
    /// Returns [`WfMxError`] only in the (upstream-internal) case where
    /// the identifier cannot be produced; for a value built from a
    /// successful [`WfMx::from_xml`] this is effectively infallible.
    pub fn message_type(&self) -> Result<&str, WfMxError> {
        self.inner
            .message_type()
            .map_err(|e| WfMxError::outbound(&e))
    }

    /// The typed document body (the upstream [`Document`] enum — see the
    /// module-level "Coupling note").
    pub fn document(&self) -> &Document {
        &self.inner.document
    }

    /// Re-serialise the message back to an ISO 20022 XML envelope.
    ///
    /// `from_xml` then `to_xml` is functional self-consistency, not a
    /// standards-conformance measurement.
    pub fn to_xml(&self) -> Result<String, WfMxError> {
        self.inner.to_xml().map_err(|e| WfMxError::outbound(&e))
    }

    /// Serialise the message to JSON (pretty-printed).
    pub fn to_json(&self) -> Result<String, WfMxError> {
        self.inner.to_json().map_err(|e| WfMxError::outbound(&e))
    }
}

/// Whether a [`WfMxError`] arose while parsing input or while producing
/// output.
///
/// This is the Wireforge-owned, stable classification callers may match
/// on without coupling to the upstream error enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WfMxStage {
    /// Failure while parsing an inbound XML envelope.
    Inbound,
    /// Failure while serialising an outbound XML/JSON representation.
    Outbound,
}

/// Error returned when the upstream ISO 20022 parser or serialiser
/// rejects an operation.
///
/// Carries a human-readable summary of the underlying failure rather
/// than the upstream error type itself, so callers stay decoupled from
/// it. The [`fmt::Display`] impl states the three things a caller needs:
/// what failed, what was expected, and what the caller can do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfMxError {
    stage: WfMxStage,
    /// Summary of the upstream failure.
    source: String,
}

impl WfMxError {
    fn inbound(source: &mx_message::error::MxError) -> Self {
        WfMxError {
            stage: WfMxStage::Inbound,
            source: source.to_string(),
        }
    }

    fn outbound(source: &mx_message::error::MxError) -> Self {
        WfMxError {
            stage: WfMxStage::Outbound,
            source: source.to_string(),
        }
    }

    /// Whether the failure was on the inbound (parse) or outbound
    /// (serialise) path.
    pub fn stage(&self) -> WfMxStage {
        self.stage
    }

    /// Summary of the underlying upstream failure.
    pub fn source_summary(&self) -> &str {
        &self.source
    }
}

impl fmt::Display for WfMxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Three-element error: (1) what failed, (2) what was expected,
        // (3) what the caller can do.
        match self.stage {
            WfMxStage::Inbound => write!(
                f,
                "could not parse ISO 20022 (MX) message ({}); \
                 expected a full envelope with an <AppHdr> business header \
                 (carrying MsgDefIdr) followed by a <Document> of a \
                 supported type (pacs/pain/camt/admi); \
                 check the input is a complete envelope (not a bare \
                 <Document>, a SWIFT MT message, or an unsupported \
                 message type) before retrying",
                self.source
            ),
            WfMxStage::Outbound => write!(
                f,
                "could not serialise ISO 20022 (MX) message ({}); \
                 expected an in-memory message produced by from_xml or \
                 built from the typed model; \
                 the parsed value may carry data the serialiser cannot \
                 represent — inspect the document body before retrying",
                self.source
            ),
        }
    }
}

impl std::error::Error for WfMxError {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A minimal, well-formed pacs.008.001.08 ISO 20022 envelope.
    ///
    /// PROVENANCE (honesty): this XML is authored here, but its element
    /// structure is taken field-for-field from the `mx-message` crate's
    /// OWN bundled CBPR+ scenario `test_scenarios/pacs008/minimal.json`
    /// (Apache-2.0, GoPlasmatic) — the same crate this facade wraps. The
    /// crate ships that scenario as a `datafake-rs` *template* (with
    /// `{"fake":…}` / `{"var":…}` generator nodes), not a ready-to-parse
    /// message, and ships no static `.xml` sample; so we instantiate that
    /// documented shape with fixed values rather than pulling the
    /// `datafake-rs` generator into the test. The envelope is the external
    /// anchor; the values (BIC `BANKUS33XXX`, USD 100.00, IBAN, dates) are
    /// concrete fills of the upstream-defined required fields. The
    /// upstream parser accepting this (and the typed model exposing the
    /// values below) is the evidence the structure is real — a malformed
    /// or invented shape would be rejected by `from_xml`.
    ///
    /// Required fields per the real typed model
    /// (`document::pacs_008_001_08`): the BAH needs `Fr`/`To`/`BizMsgIdr`/
    /// `MsgDefIdr`/`BizSvc` (pattern `[a-z0-9]+\.…\.\d\d`)/`CreDt` (with a
    /// timezone offset); the transaction needs `PmtId.EndToEndId`,
    /// `IntrBkSttlmAmt`, `IntrBkSttlmDt`, `ChrgBr`, `InstgAgt`/`InstdAgt`,
    /// `Dbtr`, `DbtrAgt`, `CdtrAgt`, `Cdtr`.
    const PACS008_ENVELOPE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope>
  <AppHdr>
    <Fr><FIId><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></FIId></Fr>
    <To><FIId><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></FIId></To>
    <BizMsgIdr>MSG-MIN-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>MIN-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-MIN-001</InstrId>
          <EndToEndId>E2E-MIN-001</EndToEndId>
          <UETR>00000000-0000-4000-8000-000000000001</UETR>
        </PmtId>
        <IntrBkSttlmAmt Ccy="USD">100.00</IntrBkSttlmAmt>
        <IntrBkSttlmDt>2024-01-15</IntrBkSttlmDt>
        <ChrgBr>SHAR</ChrgBr>
        <InstgAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></InstgAgt>
        <InstdAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></InstdAgt>
        <Dbtr><Nm>John Doe</Nm></Dbtr>
        <DbtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></DbtrAcct>
        <DbtrAgt><FinInstnId><BICFI>BANKUS33XXX</BICFI></FinInstnId></DbtrAgt>
        <CdtrAgt><FinInstnId><BICFI>BANKGB22XXX</BICFI></FinInstnId></CdtrAgt>
        <Cdtr><Nm>Jane Smith</Nm></Cdtr>
        <CdtrAcct><Id><IBAN>GB29NWBK60161331926819</IBAN></Id></CdtrAcct>
      </CdtTrfTxInf>
    </FIToFICstmrCdtTrf>
  </Document>
</Envelope>"#;

    #[test]
    fn pacs008_envelope_parses_and_reports_type() {
        let mx = WfMx::from_xml(PACS008_ENVELOPE).expect("pacs.008 envelope must parse");

        // (b) message_type reports the expected pacs.008.001.08, taken
        // from the AppHdr's MsgDefIdr.
        assert_eq!(
            mx.message_type().expect("message type present"),
            "pacs.008.001.08"
        );

        // (c) the typed body is the pacs.008 variant — reaching it through
        // the typed Document enum (not string slicing) proves the parser
        // classified the body.
        assert!(
            matches!(mx.document(), Document::Pacs008(_)),
            "document body must be the typed Pacs008 variant"
        );
    }

    #[test]
    fn pacs008_semantic_fields_are_reachable_via_typed_model() {
        // Semantic anchor: reach into the typed model and read fields off
        // the parsed structure. This proves the parser decoded them into
        // the typed model rather than merely tagging the message type.
        // Field paths verified against the real upstream source
        // (document::pacs_008_001_08): cdt_trf_tx_inf is a single
        // CreditTransferTransaction391; intr_bk_sttlm_amt is a CBPRAmount1
        // { ccy: String, value: f64 }; instg_agt.fin_instn_id is a
        // FinancialInstitutionIdentification182 { bicfi: String }.
        let mx = WfMx::from_xml(PACS008_ENVELOPE).expect("parse");

        let Document::Pacs008(body) = mx.document() else {
            panic!("expected Pacs008 document body");
        };
        let tx = &body.cdt_trf_tx_inf;
        assert_eq!(tx.intr_bk_sttlm_amt.ccy, "USD", "settlement currency");
        assert_eq!(
            tx.intr_bk_sttlm_amt.value, 100.00,
            "settlement amount value"
        );
        assert_eq!(
            tx.instg_agt.fin_instn_id.bicfi, "BANKUS33XXX",
            "instructing-agent BIC"
        );
    }

    #[test]
    fn malformed_xml_returns_error_without_panicking() {
        // Not a well-formed envelope: no <AppHdr>, no <Document>.
        let garbage = "this is definitely not an ISO 20022 envelope";
        let err = WfMx::from_xml(garbage).expect_err("garbage must not parse");
        assert_eq!(err.stage(), WfMxStage::Inbound);

        let msg = err.to_string();
        // Three-element message must name what was expected and what to do.
        assert!(
            msg.contains("could not parse") && msg.contains("AppHdr"),
            "error must explain the expected envelope shape; got: {msg}"
        );
        assert!(
            !err.source_summary().is_empty(),
            "underlying summary must be populated"
        );
    }

    #[test]
    fn document_only_xml_is_rejected_as_inbound_error() {
        // A bare <Document> with no <AppHdr> is the documented inbound
        // limitation: it must be reported as an inbound WfMxError, not a
        // panic and not a silent success.
        let doc_only = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document>
  <FIToFICstmrCdtTrf>
    <GrpHdr><MsgId>X</MsgId></GrpHdr>
  </FIToFICstmrCdtTrf>
</Document>"#;
        let err = WfMx::from_xml(doc_only).expect_err("document-only XML must be rejected");
        assert_eq!(err.stage(), WfMxStage::Inbound);
    }

    #[test]
    fn from_xml_to_xml_round_trip_is_self_consistent() {
        // Functional self-consistency (NOT a standards measurement): a
        // parsed envelope, re-serialised and re-parsed, yields an equal
        // typed message.
        let first = WfMx::from_xml(PACS008_ENVELOPE).expect("first parse");
        let wire = first.to_xml().expect("serialise");
        let second = WfMx::from_xml(&wire).expect("re-parse rebuilt envelope");
        assert_eq!(
            first, second,
            "from_xml -> to_xml -> from_xml must be self-consistent"
        );
    }
}
