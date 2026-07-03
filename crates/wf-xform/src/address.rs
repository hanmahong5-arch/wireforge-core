//! Structural CBPR+ SR2026 postal-address compliance checker for
//! pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 and pain.001.001.09.
//!
//! ## What this module is — and is NOT
//!
//! CBPR+ SR2026 (mandatory 2026-11-14) requires that a debtor/creditor
//! postal address carry Town Name (`TwnNm`) and Country (`Ctry`) in
//! dedicated structured fields, not only in unstructured `AdrLine`
//! elements. This module **detects** whether those structured fields are
//! present, across the customer-facing payment family: the
//! pacs.008.001.08 (FIToFICstmrCdtTrf) credit transfer, the
//! pacs.004.001.09 (PmtRtr) payment return, the pacs.003.001.08
//! (FIToFICstmrDrctDbt) customer direct debit, and the pain.001.001.09
//! (CstmrCdtTrfInitn) customer credit-transfer initiation.
//!
//! It is a **structural presence check against that ONE cited rule only**:
//!
//! - It does **not** perform a full CBPR+ validation.
//! - It makes **no** certification or conformance claim.
//! - A verdict of [`AddressVerdict::Compliant`] means only that `TwnNm` and
//!   `Ctry` are present — nothing about their contents, character sets, or
//!   any other CBPR+ requirement.

use wf_mx::{Document, WfMx};

use crate::XformError;

/// Which party is being checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub enum AddressParty {
    Debtor,
    Creditor,
}

impl AddressParty {
    /// Every party this checker covers, in stable iteration order.
    pub const ALL: [AddressParty; 2] = [AddressParty::Debtor, AddressParty::Creditor];

    /// A short, stable, human-readable name for the party.
    pub fn as_str(self) -> &'static str {
        match self {
            AddressParty::Debtor => "debtor",
            AddressParty::Creditor => "creditor",
        }
    }
}

/// Verdict for one party against the CBPR+ SR2026 rule: `PstlAdr` must carry
/// `TwnNm` + `Ctry` in dedicated structured fields (mandatory 2026-11-14).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub enum AddressVerdict {
    /// `PstlAdr` is present with BOTH `TwnNm` and `Ctry` populated.
    Compliant,
    /// `PstlAdr` is present but is missing the mandatory structured `TwnNm`
    /// and/or `Ctry` fields.
    MissingStructured {
        /// Whether a `TwnNm` element was present.
        town_name_present: bool,
        /// Whether a `Ctry` element was present.
        country_present: bool,
        /// Number of `AdrLine` elements found (unstructured address lines).
        unstructured_lines: usize,
    },
    /// No `PstlAdr` element is present for this party at all.
    NoAddress,
}

/// One party's address compliance result row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct AddressRow {
    /// The party this row describes.
    pub party: AddressParty,
    /// The compliance verdict for this party.
    pub verdict: AddressVerdict,
    /// The `TwnNm` value, if present.
    pub town_name: Option<String>,
    /// The `Ctry` value, if present.
    pub country: Option<String>,
    /// The number of unstructured `AdrLine` elements found (0 when none).
    pub unstructured_lines: usize,
}

/// The full address compliance report: one [`AddressRow`] per party in
/// [`AddressParty::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "json", derive(serde::Serialize))]
pub struct AddressComplianceReport {
    /// One row per party in [`AddressParty::ALL`] order.
    pub rows: Vec<AddressRow>,
    /// The ISO 20022 message-type identifier this report was produced for —
    /// one of `"pacs.008.001.08"`, `"pacs.004.001.09"`, `"pacs.003.001.08"`
    /// or `"pain.001.001.09"`. Lets consumers state which spec was checked.
    pub message_type: &'static str,
}

impl AddressComplianceReport {
    /// The rows whose verdict is not [`AddressVerdict::Compliant`].
    pub fn non_compliant_rows(&self) -> impl Iterator<Item = &AddressRow> {
        self.rows
            .iter()
            .filter(|r| !matches!(r.verdict, AddressVerdict::Compliant))
    }

    /// `true` iff every row is [`AddressVerdict::Compliant`].
    pub fn all_compliant(&self) -> bool {
        self.rows
            .iter()
            .all(|r| matches!(r.verdict, AddressVerdict::Compliant))
    }

    /// Serialize this report to the canonical wire JSON shape shared by every
    /// surface (CLI, MCP tool, future API): `{ "message_type", "compliant",
    /// "rows": [ { "party", "verdict", "town_name", "country",
    /// "unstructured_lines", "remediation" }, ... ] }`.
    ///
    /// `town_name` / `country` / `remediation` are JSON `null` (never
    /// omitted) when absent, so downstream CSV columns stay stable.
    #[cfg(feature = "json")]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "message_type": self.message_type,
            "compliant": self.all_compliant(),
            "rows": self.rows.iter().map(|row| {
                serde_json::json!({
                    "party": row.party.as_str(),
                    "verdict": verdict_str(&row.verdict),
                    "town_name": row.town_name,
                    "country": row.country,
                    "unstructured_lines": row.unstructured_lines,
                    "remediation": row.remediation(),
                })
            }).collect::<Vec<_>>(),
        })
    }
}

/// Map an [`AddressVerdict`] to its stable, machine-readable string form —
/// identical to the strings the MCP address-compliance tool has always
/// returned (`"compliant"` | `"missing_structured"` | `"no_address"`).
pub fn verdict_str(v: &AddressVerdict) -> &'static str {
    match v {
        AddressVerdict::Compliant => "compliant",
        AddressVerdict::MissingStructured { .. } => "missing_structured",
        AddressVerdict::NoAddress => "no_address",
    }
}

impl AddressRow {
    /// Actionable, per-party fix guidance for this row's verdict.
    ///
    /// This is a **DETECTOR**: it names which structured field(s) are
    /// absent and tells the operator what to do about it — it does NOT
    /// restructure, convert, or auto-fix the address itself. `None` when
    /// the row is already [`AddressVerdict::Compliant`].
    pub fn remediation(&self) -> Option<String> {
        match &self.verdict {
            AddressVerdict::Compliant => None,
            AddressVerdict::MissingStructured {
                town_name_present,
                country_present,
                unstructured_lines,
            } => {
                let mut missing = Vec::new();
                if !town_name_present {
                    missing.push("TwnNm");
                }
                if !country_present {
                    missing.push("Ctry");
                }
                let missing_list = missing.join(" and ");
                Some(format!(
                    "The {party} address is missing the structured {missing_list} field(s); \
                     {lines} unstructured AdrLine line(s) were found instead. Populate the \
                     missing structured field(s) at the originating system and migrate the \
                     free-text AdrLine content into structured elements — this checker is a \
                     DETECTOR and does NOT restructure the address for you. CBPR+ SR2026 makes \
                     this mandatory from 2026-11-14.",
                    party = self.party.as_str(),
                    missing_list = missing_list,
                    lines = unstructured_lines,
                ))
            }
            AddressVerdict::NoAddress => Some(format!(
                "The {party} carries no PstlAdr at all; if this flow requires an address, add \
                 one with structured TwnNm + Ctry populated. CBPR+ SR2026 mandates structure \
                 whenever an address is present (mandatory 2026-11-14).",
                party = self.party.as_str(),
            )),
        }
    }
}

/// Structured-address fields read from a single party's `PstlAdr`,
/// independent of which MX message type carried it.
///
/// Each supported schema defines its own postal-address type — pacs.008,
/// pacs.004 and pacs.003 a `PostalAddress241`, pain.001 a
/// `PostalAddress242` — distinct Rust types with identical fields, so every
/// checker extracts into this shared, format-agnostic shape before the
/// common classifier runs. A `None` at the call site means the party had no
/// `PstlAdr` element at all (→ [`AddressVerdict::NoAddress`]).
struct PartyAddress {
    /// The `TwnNm` value, if present.
    town_name: Option<String>,
    /// The `Ctry` value, if present.
    country: Option<String>,
    /// Number of unstructured `AdrLine` elements (0 when none).
    unstructured_lines: usize,
}

/// Classify one party's extracted address against the CBPR+ SR2026 rule.
///
/// `None` → [`AddressVerdict::NoAddress`]; otherwise both `TwnNm` and `Ctry`
/// present → [`AddressVerdict::Compliant`], else
/// [`AddressVerdict::MissingStructured`].
fn row_for(party: AddressParty, addr: Option<PartyAddress>) -> AddressRow {
    match addr {
        None => AddressRow {
            party,
            verdict: AddressVerdict::NoAddress,
            town_name: None,
            country: None,
            unstructured_lines: 0,
        },
        Some(a) => {
            let verdict = if a.town_name.is_some() && a.country.is_some() {
                AddressVerdict::Compliant
            } else {
                AddressVerdict::MissingStructured {
                    town_name_present: a.town_name.is_some(),
                    country_present: a.country.is_some(),
                    unstructured_lines: a.unstructured_lines,
                }
            };
            AddressRow {
                party,
                verdict,
                town_name: a.town_name,
                country: a.country,
                unstructured_lines: a.unstructured_lines,
            }
        }
    }
}

/// Build a full report from the debtor's and creditor's extracted addresses.
///
/// Rows are emitted in [`AddressParty::ALL`] order (debtor, then creditor) —
/// the shared core both checkers delegate to, so the per-party
/// `TwnNm` + `Ctry` logic lives in exactly one place.
fn report_from_pair(
    dbtr: Option<PartyAddress>,
    cdtr: Option<PartyAddress>,
    message_type: &'static str,
) -> AddressComplianceReport {
    AddressComplianceReport {
        rows: vec![
            row_for(AddressParty::Debtor, dbtr),
            row_for(AddressParty::Creditor, cdtr),
        ],
        message_type,
    }
}

/// Structural presence check of pacs.008.001.08 debtor & creditor postal
/// addresses against the CBPR+ SR2026 requirement that `TwnNm` + `Ctry`
/// appear in dedicated structured fields (mandatory 2026-11-14). NOT a full
/// CBPR+ validation and NOT a certification.
///
/// Delegates the per-party classification to the shared core
/// ([`report_from_pair`]). Returns [`XformError`] if `mx` is not a pacs.008
/// document.
pub fn check_pacs008_address(mx: &WfMx) -> Result<AddressComplianceReport, XformError> {
    let Document::Pacs008(body) = mx.document() else {
        return Err(XformError::mx_not_address_checkable(document_kind(
            mx.document(),
        )));
    };

    let tx = &body.cdt_trf_tx_inf;
    // pacs.008 carries each party directly (no `Pty` choice indirection).
    // The extraction is inlined per party because the pacs.008 and pacs.004
    // `PostalAddress241` are distinct upstream types with identical fields, so
    // a single shared reader would have to name (or be generic over) them.
    let dbtr = tx.dbtr.pstl_adr.as_ref().map(|adr| PartyAddress {
        town_name: adr.twn_nm.clone(),
        country: adr.ctry.clone(),
        unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
    });
    let cdtr = tx.cdtr.pstl_adr.as_ref().map(|adr| PartyAddress {
        town_name: adr.twn_nm.clone(),
        country: adr.ctry.clone(),
        unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
    });

    Ok(report_from_pair(dbtr, cdtr, "pacs.008.001.08"))
}

/// Structural presence check of pacs.004.001.09 (Payment Return) debtor &
/// creditor postal addresses against the same CBPR+ SR2026 rule
/// (`TwnNm` + `Ctry` in dedicated structured fields, mandatory 2026-11-14).
/// NOT a full CBPR+ validation and NOT a certification.
///
/// The pacs.004 parties live under the return chain
/// (`TxInf/RtrChain/{Dbtr,Cdtr}`) behind an `Option<Pty>` choice: a party
/// identified only by an agent (no `Pty`) carries no postal address and maps
/// to [`AddressVerdict::NoAddress`]. Returns [`XformError`] if `mx` is not a
/// pacs.004 document.
pub fn check_pacs004_address(mx: &WfMx) -> Result<AddressComplianceReport, XformError> {
    let Document::Pacs004(body) = mx.document() else {
        return Err(XformError::mx_not_address_checkable(document_kind(
            mx.document(),
        )));
    };

    let chain = &body.tx_inf.rtr_chain;
    let dbtr = chain
        .dbtr
        .pty
        .as_ref()
        .and_then(|p| p.pstl_adr.as_ref())
        .map(|adr| PartyAddress {
            town_name: adr.twn_nm.clone(),
            country: adr.ctry.clone(),
            unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
        });
    let cdtr = chain
        .cdtr
        .pty
        .as_ref()
        .and_then(|p| p.pstl_adr.as_ref())
        .map(|adr| PartyAddress {
            town_name: adr.twn_nm.clone(),
            country: adr.ctry.clone(),
            unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
        });

    Ok(report_from_pair(dbtr, cdtr, "pacs.004.001.09"))
}

/// Structural presence check of pacs.003.001.08 (FIToFICstmrDrctDbt, customer
/// direct debit) debtor & creditor postal addresses against the same CBPR+
/// SR2026 rule (`TwnNm` + `Ctry` in dedicated structured fields, mandatory
/// 2026-11-14). NOT a full CBPR+ validation and NOT a certification.
///
/// pacs.003 is a single-transaction message whose debtor and creditor sit
/// directly under `DrctDbtTxInf` (no `Pty` choice indirection — an exact
/// mirror of pacs.008). The extraction is inlined per party because the
/// pacs.003 `PostalAddress241` is a distinct upstream type from the
/// pacs.008 / pacs.004 ones with identical fields. Returns [`XformError`] if
/// `mx` is not a pacs.003 document.
pub fn check_pacs003_address(mx: &WfMx) -> Result<AddressComplianceReport, XformError> {
    let Document::Pacs003(body) = mx.document() else {
        return Err(XformError::mx_not_address_checkable(document_kind(
            mx.document(),
        )));
    };

    let tx = &body.drct_dbt_tx_inf;
    let dbtr = tx.dbtr.pstl_adr.as_ref().map(|adr| PartyAddress {
        town_name: adr.twn_nm.clone(),
        country: adr.ctry.clone(),
        unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
    });
    let cdtr = tx.cdtr.pstl_adr.as_ref().map(|adr| PartyAddress {
        town_name: adr.twn_nm.clone(),
        country: adr.ctry.clone(),
        unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
    });

    Ok(report_from_pair(dbtr, cdtr, "pacs.003.001.08"))
}

/// Structural presence check of pain.001.001.09 (CstmrCdtTrfInitn, customer
/// credit-transfer initiation) debtor & creditor postal addresses against the
/// same CBPR+ SR2026 rule (`TwnNm` + `Ctry` in dedicated structured fields,
/// mandatory 2026-11-14). NOT a full CBPR+ validation and NOT a certification.
///
/// Unlike the pacs messages, pain.001's two parties sit at **different**
/// nesting levels under the single `PmtInf`: the debtor is `PmtInf/Dbtr`,
/// while the creditor is one level deeper under the single transaction,
/// `PmtInf/CdtTrfTxInf/Cdtr`. Both carry a `PstlAdr: Option<PostalAddress242>`
/// — again a distinct upstream type with the same `Option<String>` fields, so
/// the per-party extraction is identical. Returns [`XformError`] if `mx` is
/// not a pain.001 document.
pub fn check_pain001_address(mx: &WfMx) -> Result<AddressComplianceReport, XformError> {
    let Document::Pain001(body) = mx.document() else {
        return Err(XformError::mx_not_address_checkable(document_kind(
            mx.document(),
        )));
    };

    let pmt = &body.pmt_inf;
    let dbtr = pmt.dbtr.pstl_adr.as_ref().map(|adr| PartyAddress {
        town_name: adr.twn_nm.clone(),
        country: adr.ctry.clone(),
        unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
    });
    let cdtr = pmt
        .cdt_trf_tx_inf
        .cdtr
        .pstl_adr
        .as_ref()
        .map(|adr| PartyAddress {
            town_name: adr.twn_nm.clone(),
            country: adr.ctry.clone(),
            unstructured_lines: adr.adr_line.as_ref().map_or(0, |v| v.len()),
        });

    Ok(report_from_pair(dbtr, cdtr, "pain.001.001.09"))
}

/// Structural CBPR+ SR2026 address-compliance check that auto-detects the
/// message type and dispatches to the matching checker.
///
/// Supports pacs.008.001.08 (FIToFICstmrCdtTrf), pacs.004.001.09 (PmtRtr),
/// pacs.003.001.08 (FIToFICstmrDrctDbt) and pain.001.001.09
/// (CstmrCdtTrfInitn); any other document type returns [`XformError`]. The
/// returned report's [`AddressComplianceReport::message_type`] states which
/// spec was checked. NOT a full CBPR+ validation and NOT a certification.
pub fn check_mx_address(mx: &WfMx) -> Result<AddressComplianceReport, XformError> {
    match mx.document() {
        Document::Pacs008(_) => check_pacs008_address(mx),
        Document::Pacs004(_) => check_pacs004_address(mx),
        Document::Pacs003(_) => check_pacs003_address(mx),
        Document::Pain001(_) => check_pain001_address(mx),
        other => Err(XformError::mx_not_address_checkable(document_kind(other))),
    }
}

/// A short label for a [`Document`] variant, used only in error messages.
fn document_kind(doc: &Document) -> &'static str {
    match doc {
        Document::Pacs008(_) => "pacs.008",
        Document::Pacs004(_) => "pacs.004",
        Document::Pacs003(_) => "pacs.003",
        Document::Pain001(_) => "pain.001",
        _ => "an unsupported MX document",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // Every fixture here is hand-authored SYNTHETIC data; expectations are
    // computed by hand from the SR2026 rule (TwnNm + Ctry both present ⇒
    // compliant), never read back from the checker under test.

    fn compliant_row(party: AddressParty) -> AddressRow {
        AddressRow {
            party,
            verdict: AddressVerdict::Compliant,
            town_name: Some("LONDON".to_string()),
            country: Some("GB".to_string()),
            unstructured_lines: 0,
        }
    }

    fn missing_town_row(party: AddressParty) -> AddressRow {
        // Ctry present, TwnNm absent.
        AddressRow {
            party,
            verdict: AddressVerdict::MissingStructured {
                town_name_present: false,
                country_present: true,
                unstructured_lines: 2,
            },
            town_name: None,
            country: Some("GB".to_string()),
            unstructured_lines: 2,
        }
    }

    fn missing_country_row(party: AddressParty) -> AddressRow {
        // TwnNm present, Ctry absent.
        AddressRow {
            party,
            verdict: AddressVerdict::MissingStructured {
                town_name_present: true,
                country_present: false,
                unstructured_lines: 1,
            },
            town_name: Some("LONDON".to_string()),
            country: None,
            unstructured_lines: 1,
        }
    }

    fn missing_both_row(party: AddressParty) -> AddressRow {
        // Neither TwnNm nor Ctry present.
        AddressRow {
            party,
            verdict: AddressVerdict::MissingStructured {
                town_name_present: false,
                country_present: false,
                unstructured_lines: 3,
            },
            town_name: None,
            country: None,
            unstructured_lines: 3,
        }
    }

    fn no_address_row(party: AddressParty) -> AddressRow {
        AddressRow {
            party,
            verdict: AddressVerdict::NoAddress,
            town_name: None,
            country: None,
            unstructured_lines: 0,
        }
    }

    fn assert_honest_wording(text: &str) {
        assert!(text.contains("2026-11-14"), "text: {text}");
        let lower = text.to_lowercase();
        assert!(!lower.contains("certif"), "text: {text}");
        assert!(!lower.contains("convert"), "text: {text}");
        assert!(!lower.contains("guarantee"), "text: {text}");
    }

    #[test]
    fn remediation_none_for_compliant() {
        let row = compliant_row(AddressParty::Debtor);
        assert_eq!(row.remediation(), None);
    }

    #[test]
    fn remediation_names_twn_nm_when_only_country_present() {
        let row = missing_town_row(AddressParty::Debtor);
        let text = row.remediation().expect("must have remediation text");
        assert!(text.contains("TwnNm"), "text: {text}");
        assert!(!text.contains("Ctry"), "text: {text}");
        assert!(text.contains('2'), "should mention 2 AdrLine lines: {text}");
        assert_honest_wording(&text);
    }

    #[test]
    fn remediation_names_ctry_when_only_town_present() {
        let row = missing_country_row(AddressParty::Creditor);
        let text = row.remediation().expect("must have remediation text");
        assert!(text.contains("Ctry"), "text: {text}");
        assert!(!text.contains("TwnNm"), "text: {text}");
        assert_honest_wording(&text);
    }

    #[test]
    fn remediation_names_both_when_neither_present() {
        let row = missing_both_row(AddressParty::Debtor);
        let text = row.remediation().expect("must have remediation text");
        assert!(text.contains("TwnNm"), "text: {text}");
        assert!(text.contains("Ctry"), "text: {text}");
        assert_honest_wording(&text);
    }

    #[test]
    fn remediation_some_for_no_address() {
        let row = no_address_row(AddressParty::Creditor);
        let text = row.remediation().expect("must have remediation text");
        assert!(text.contains("PstlAdr"), "text: {text}");
        assert_honest_wording(&text);
    }

    #[test]
    fn verdict_str_matches_mcp_tool_strings() {
        assert_eq!(verdict_str(&AddressVerdict::Compliant), "compliant");
        assert_eq!(
            verdict_str(&AddressVerdict::MissingStructured {
                town_name_present: false,
                country_present: true,
                unstructured_lines: 1,
            }),
            "missing_structured"
        );
        assert_eq!(verdict_str(&AddressVerdict::NoAddress), "no_address");
    }

    #[cfg(feature = "json")]
    mod json_tests {
        use super::*;

        #[test]
        fn to_json_null_town_name_on_missing_town_row() {
            let report = AddressComplianceReport {
                rows: vec![
                    missing_town_row(AddressParty::Debtor),
                    compliant_row(AddressParty::Creditor),
                ],
                message_type: "pacs.008.001.08",
            };
            let json = report.to_json();
            assert_eq!(json["message_type"], "pacs.008.001.08");
            assert_eq!(json["compliant"], false);

            let debtor = &json["rows"][0];
            assert_eq!(debtor["party"], "debtor");
            assert_eq!(debtor["verdict"], "missing_structured");
            assert!(debtor["town_name"].is_null());
            assert_eq!(debtor["country"], "GB");
            assert_eq!(debtor["unstructured_lines"], 2);
            assert!(
                debtor["remediation"].is_string(),
                "remediation must be non-null string: {debtor:?}"
            );
            let remediation_text = debtor["remediation"].as_str().unwrap();
            assert_honest_wording(remediation_text);
        }

        #[test]
        fn to_json_compliant_true_on_all_compliant_report() {
            let report = AddressComplianceReport {
                rows: vec![
                    compliant_row(AddressParty::Debtor),
                    compliant_row(AddressParty::Creditor),
                ],
                message_type: "pacs.003.001.08",
            };
            let json = report.to_json();
            assert_eq!(json["compliant"], true);
            for row in json["rows"].as_array().unwrap() {
                assert_eq!(row["verdict"], "compliant");
                assert!(row["remediation"].is_null());
            }
        }

        #[test]
        fn to_json_no_address_row_has_null_fields_and_remediation() {
            let report = AddressComplianceReport {
                rows: vec![
                    no_address_row(AddressParty::Debtor),
                    compliant_row(AddressParty::Creditor),
                ],
                message_type: "pain.001.001.09",
            };
            let json = report.to_json();
            let debtor = &json["rows"][0];
            assert_eq!(debtor["verdict"], "no_address");
            assert!(debtor["town_name"].is_null());
            assert!(debtor["country"].is_null());
            assert_eq!(debtor["unstructured_lines"], 0);
            assert!(debtor["remediation"].is_string());
        }
    }
}
