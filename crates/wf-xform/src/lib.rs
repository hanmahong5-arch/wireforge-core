//! Wireforge MT/MX field truncation & loss **detector**.
//!
//! ## What this crate is — and is NOT
//!
//! Given an already-parsed SWIFT MT message and an already-parsed ISO
//! 20022 (MX) message that a caller believes correspond to the same
//! payment, this crate extracts a small set of core *roles* (debtor name,
//! creditor name, remittance information, settlement amount, settlement
//! currency) from each side and reports, per role, whether the values are
//! equal, merely reformatted, or whether the MX value would not fit in
//! the corresponding MT field's standard capacity (**truncation**), plus
//! whether a role is present on only one side.
//!
//! It is a **detector**, not a converter:
//!
//! - It does **not** convert MT to MX or MX to MT. It only *compares* two
//!   messages the caller already holds.
//! - It makes **no** certification, conformance, or equivalence claim. A
//!   report of [`FieldDiff::Equal`] means the two extracted strings are
//!   byte-equal — nothing about scheme-level correctness.
//! - The pure-Rust ecosystem has no certified MT/MX converter; this crate
//!   deliberately does not attempt to be one.
//!
//! ## How capacities are grounded
//!
//! Every truncation verdict is driven by a **cited** maximum length: the
//! MT-side field capacity from the SWIFT MT103 format spec, and the
//! MX-side `maxLength` facet read from the `mx-message` crate's generated
//! pacs.008.001.08 validators. See [`Role::mt_max_len`] and
//! [`Role::mx_max_len`] — each carries an inline source citation. Where a
//! side has no cited maximum (the MX `IntrBkSttlmAmt` value is an
//! unconstrained `f64`), the capacity is [`MaxLen::Unknown`] and
//! truncation classification is **skipped** for that role rather than
//! guessed.
//!
//! ## Scope (honest)
//!
//! This covers pacs.008.001.08 vs MT103 for five core roles only. It does
//! not cover other message pairs, structured remittance, postal
//! addresses, identifiers, charges, or regulatory reporting.

pub mod address;
pub use address::{
    check_mx_address, check_pacs003_address, check_pacs004_address, check_pacs008_address,
    check_pain001_address, AddressComplianceReport, AddressParty, AddressRow, AddressVerdict,
};

/// The ISO 20022 payment messages the SR2026 address-compliance family
/// (`check_mx_address` and the per-type checkers) understands, as a single
/// human-readable list. One source of truth so the `XformError` Display and
/// the MCP/CLI scope notes never drift apart.
pub const ADDRESS_CHECKABLE_TYPES: &str =
    "pacs.008.001.08, pacs.004.001.09, pacs.003.001.08, pain.001.001.09";

use std::fmt;

use wf_mx::{Document, WfMx};
use wf_swift::{MtMessage, WfMt};

/// A field role that can be located on both the MT and MX sides of a
/// corresponding pacs.008 / MT103 pair.
///
/// The role — not a tag or an XML path — is the join key between the two
/// sides, so the same comparison logic works regardless of how each
/// format names the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Debtor (ordering customer) name: MT field 50K name lines vs MX
    /// `CdtTrfTxInf/Dbtr/Nm`.
    DebtorName,
    /// Creditor (beneficiary) name: MT field 59 name lines vs MX
    /// `CdtTrfTxInf/Cdtr/Nm`.
    CreditorName,
    /// Unstructured remittance information: MT field 70 vs MX
    /// `CdtTrfTxInf/RmtInf/Ustrd`.
    RemittanceInfo,
    /// Interbank settlement amount: MT field 32A amount component vs MX
    /// `CdtTrfTxInf/IntrBkSttlmAmt`.
    SettlementAmount,
    /// Interbank settlement currency: MT field 32A currency component vs
    /// MX `CdtTrfTxInf/IntrBkSttlmAmt/@Ccy`.
    SettlementCurrency,
}

impl Role {
    /// Every role this detector understands, in a stable order.
    pub const ALL: [Role; 5] = [
        Role::DebtorName,
        Role::CreditorName,
        Role::RemittanceInfo,
        Role::SettlementAmount,
        Role::SettlementCurrency,
    ];

    /// A short, stable, human-readable name for the role.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::DebtorName => "debtor_name",
            Role::CreditorName => "creditor_name",
            Role::RemittanceInfo => "remittance_info",
            Role::SettlementAmount => "settlement_amount",
            Role::SettlementCurrency => "settlement_currency",
        }
    }

    /// The MT-side maximum capacity for this role, with the standard it is
    /// cited from.
    ///
    /// Citations (do not change a length without re-checking its source):
    /// - `DebtorName` / `CreditorName`: SWIFT MT103 fields 50K / 59,
    ///   format `[/34x] 4*35x` — 4 name lines of 35 chars = 140 chars.
    ///   (Paiementor "SWIFT MT103 Format Specifications"; mirrored by
    ///   `wf-codec` `swift/fields/field_50k.rs`: `MAX_NAME_LINES = 4`,
    ///   `NAME_LINE_MAX_LEN = 35`.)
    /// - `RemittanceInfo`: SWIFT MT103 field 70, format `4*35x` — up to 4
    ///   lines of 35 chars = 140 chars. (Paiementor "SWIFT MT103 Message
    ///   Example with Optional Fields".)
    /// - `SettlementCurrency`: SWIFT MT103 field 32A currency component,
    ///   format `3!a` — exactly 3 ISO 4217 letters. (`wf-codec`
    ///   `swift/fields/field_32a.rs`: `CURRENCY_LEN = 3`.)
    /// - `SettlementAmount`: SWIFT MT103 field 32A amount component is
    ///   `15d` (<=15 chars incl. decimal comma; `wf-codec`
    ///   `field_32a.rs` `AMOUNT_MAX_LEN = 15`). The amount is compared as
    ///   a *normalised decimal string*, not a raw field slice, so its MT
    ///   capacity is reported as [`MaxLen::Unknown`] (a char cap on the
    ///   wire format does not translate to a cap on the canonical value);
    ///   truncation is therefore skipped for this role.
    pub fn mt_max_len(self) -> MaxLen {
        match self {
            // 50K / 59 / 70: 4 lines x 35 chars (cited above).
            Role::DebtorName | Role::CreditorName | Role::RemittanceInfo => {
                MaxLen::Lines { lines: 4, per: 35 }
            }
            // 32A currency component: exactly 3 ISO 4217 letters.
            Role::SettlementCurrency => MaxLen::Chars(3),
            // 32A amount: see doc comment — compared as canonical value.
            Role::SettlementAmount => MaxLen::Unknown,
        }
    }

    /// The MX-side maximum capacity for this role, with the standard it is
    /// cited from.
    ///
    /// Citations (verified in `mx-message` 3.1.4
    /// `src/document/pacs_008_001_08.rs`):
    /// - `DebtorName`: `PartyIdentification1352::validate` calls
    ///   `validate_length(val, "Nm", Some(1), Some(140), ...)` — `Nm`
    ///   maxLength **140**.
    /// - `CreditorName`: `PartyIdentification1353::validate` calls
    ///   `validate_length(val, "Nm", Some(1), Some(140), ...)` — `Nm`
    ///   maxLength **140**.
    /// - `RemittanceInfo`: `RemittanceInformation161::validate` calls
    ///   `validate_length(val, "Ustrd", Some(1), Some(140), ...)` —
    ///   `Ustrd` maxLength **140**.
    /// - `SettlementCurrency`: ISO 4217 codes are exactly 3 letters;
    ///   `CBPRAmount1::ccy` is a `String` with no explicit facet, but all
    ///   valid values are 3 chars.
    /// - `SettlementAmount`: `CBPRAmount1::value` is an `f64` with **no**
    ///   `maxLength` facet (`CBPRAmount1::validate` is a no-op) →
    ///   [`MaxLen::Unknown`]; truncation skipped.
    pub fn mx_max_len(self) -> MaxLen {
        match self {
            Role::DebtorName | Role::CreditorName | Role::RemittanceInfo => MaxLen::Chars(140),
            Role::SettlementCurrency => MaxLen::Chars(3),
            Role::SettlementAmount => MaxLen::Unknown,
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A cited maximum capacity for a field.
///
/// Carries the *shape* of the capacity so a multi-line MT field
/// (e.g. 4 lines x 35 chars) is distinguished from a flat character cap,
/// and so a role with no cited length is explicitly [`MaxLen::Unknown`]
/// rather than defaulting to some guessed number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxLen {
    /// A flat maximum of `n` characters.
    Chars(usize),
    /// A multi-line capacity: `lines` lines of `per` characters each. The
    /// total text capacity (ignoring line separators) is `lines * per`.
    Lines {
        /// Number of lines the field allows.
        lines: usize,
        /// Maximum characters per line.
        per: usize,
    },
    /// No cited maximum length is known for this side. Truncation is not
    /// classified against an `Unknown` capacity.
    Unknown,
}

impl MaxLen {
    /// The total character capacity, if known.
    ///
    /// For [`MaxLen::Lines`] this is `lines * per` (the separators between
    /// lines are format framing, not payload, so they are not counted).
    /// Returns `None` for [`MaxLen::Unknown`].
    pub fn capacity(self) -> Option<usize> {
        match self {
            MaxLen::Chars(n) => Some(n),
            MaxLen::Lines { lines, per } => Some(lines * per),
            MaxLen::Unknown => None,
        }
    }
}

/// A single role's value extracted from one side, paired with that side's
/// cited capacity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemField {
    /// Which role this value plays.
    pub role: Role,
    /// The extracted value (for multi-line MT fields, lines are joined as
    /// described on [`diff_mt_mx`]).
    pub value: String,
    /// The cited maximum capacity of the field this value came from.
    pub max_len: MaxLen,
}

/// How an MX value relates to its corresponding MT field.
///
/// Exactly one variant applies to a given role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldDiff {
    /// Both sides present and byte-equal.
    Equal,
    /// Both sides present and equal after normalising whitespace and line
    /// structure, but not byte-equal (e.g. MT's 4-line layout flattened
    /// to a single MX string).
    Reformatted,
    /// Both sides present; the MX value's character count exceeds the
    /// MT field's cited capacity, so carrying it on the MT side would lose
    /// the trailing characters.
    Truncated {
        /// The characters of the MX value beyond the MT capacity — what an
        /// MT carrier could not hold.
        lost_suffix: String,
    },
    /// Present in MX but absent in MT (would be lost going to MT).
    Dropped,
    /// Present in MT but absent in MX (would be lost going to MX).
    Added,
    /// Both sides present and differ in a way that is not pure
    /// reformatting and not a length overflow.
    Mismatch,
    /// Neither side carries this role — nothing to compare. Not a loss and
    /// not a disagreement; e.g. an optional unstructured-remittance field
    /// absent from both messages.
    BothAbsent,
}

/// One row of a [`DiffReport`]: a role and its per-side values plus the
/// classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    /// The role compared.
    pub role: Role,
    /// The MT-side value, if present.
    pub mt_value: Option<String>,
    /// The MX-side value, if present.
    pub mx_value: Option<String>,
    /// The classification of this role.
    pub diff: FieldDiff,
}

/// The full comparison of an MT/MX pair across all known roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    /// One row per role in [`Role::ALL`] order.
    pub rows: Vec<DiffRow>,
}

impl DiffReport {
    /// The rows whose classification indicates potential data loss
    /// ([`FieldDiff::Truncated`], [`FieldDiff::Dropped`]).
    pub fn lossy_rows(&self) -> impl Iterator<Item = &DiffRow> {
        // Only `Truncated` / `Dropped` indicate data loss. `BothAbsent` is
        // intentionally NOT lossy — neither side carries the role, so there
        // is nothing to lose.
        self.rows
            .iter()
            .filter(|r| matches!(r.diff, FieldDiff::Truncated { .. } | FieldDiff::Dropped))
    }
}

/// Error returned when the inputs are not the shape this detector can
/// compare.
///
/// The [`fmt::Display`] impl states the three things a caller needs: what
/// failed, what was expected, and what the caller can do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XformError {
    kind: XformErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum XformErrorKind {
    /// The MT side could not be reduced to readable block-4 fields. This
    /// covers a typed body of a type this detector does not map, or a
    /// typed body whose re-serialised wire text the structural tokeniser
    /// rejected — i.e. the MT message is unusable for field comparison.
    MtUnreadable { detail: String },
    /// The MX side was not the pacs.008 document this detector compares.
    MxNotPacs008 { found: String },
    /// The MX side was not one of the document types the SR2026 address-
    /// compliance checker supports (see [`ADDRESS_CHECKABLE_TYPES`]).
    MxNotAddressCheckable { found: String },
}

impl XformError {
    fn mt_unreadable(detail: String) -> Self {
        XformError {
            kind: XformErrorKind::MtUnreadable { detail },
        }
    }

    pub(crate) fn mx_not_pacs008(found: &str) -> Self {
        XformError {
            kind: XformErrorKind::MxNotPacs008 {
                found: found.to_string(),
            },
        }
    }

    pub(crate) fn mx_not_address_checkable(found: &str) -> Self {
        XformError {
            kind: XformErrorKind::MxNotAddressCheckable {
                found: found.to_string(),
            },
        }
    }
}

impl fmt::Display for XformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Three-element error: (1) what failed, (2) what was expected,
        // (3) what the caller can do.
        match &self.kind {
            XformErrorKind::MtUnreadable { detail } => write!(
                f,
                "cannot extract MT block-4 fields for comparison ({detail}); \
                 expected an MT message whose block-4 fields are readable \
                 (either parsed structurally, or a typed body that \
                 re-serialises to well-framed MT text); \
                 re-parse the MT input from a complete, well-framed MT \
                 message before retrying"
            ),
            XformErrorKind::MxNotPacs008 { found } => write!(
                f,
                "cannot compare this MX message ({found}); \
                 expected an ISO 20022 pacs.008.001.08 (FIToFICstmrCdtTrf) \
                 document — the only MX type this detector maps; \
                 pass a pacs.008 message, or extend the role-map before \
                 comparing other message types"
            ),
            XformErrorKind::MxNotAddressCheckable { found } => write!(
                f,
                "cannot run SR2026 address compliance on this MX message ({found}); \
                 the address-compliance checker supports ISO 20022 {ADDRESS_CHECKABLE_TYPES} \
                 (FIToFICstmrCdtTrf / PmtRtr / FIToFICstmrDrctDbt / CstmrCdtTrfInitn); \
                 pass one of those message types (the unified check_mx_address \
                 entry auto-detects which) before retrying"
            ),
        }
    }
}

impl std::error::Error for XformError {}

/// Compare a corresponding MT103 and pacs.008 for per-role truncation and
/// loss.
///
/// This is a **detector**: it reads values out of two messages the caller
/// already holds and classifies their relationship. It does not convert
/// either side and asserts no equivalence beyond byte/normalised string
/// comparison.
///
/// Extraction (paths verified against the upstream sources):
/// - **MT side**: works for either [`WfMt`] path. A structurally-parsed
///   message is read directly; a typed body is re-serialised to MT wire
///   text via the upstream `to_mt_message` and then tokenised
///   structurally. Block-4 fields are read by tag — `50K`/`59`/`70` for
///   the names and remittance, `32A` for amount and currency. For the
///   multi-line fields the raw value's lines are joined with a single
///   space after stripping any leading `/account` line (50K / 59 may
///   carry an account on their first line).
/// - **MX side**: the document must be [`Document::Pacs008`]; values are
///   read from `cdt_trf_tx_inf.{dbtr.nm, cdtr.nm, rmt_inf.ustrd,
///   intr_bk_sttlm_amt.{value, ccy}}`.
///
/// Classification per role: both present & equal → [`FieldDiff::Equal`];
/// equal after whitespace/line normalisation → [`FieldDiff::Reformatted`];
/// MX char count > MT cited capacity → [`FieldDiff::Truncated`] with the
/// lost suffix; MX-only → [`FieldDiff::Dropped`]; MT-only →
/// [`FieldDiff::Added`]; absent on both sides → [`FieldDiff::BothAbsent`];
/// otherwise → [`FieldDiff::Mismatch`]. Roles whose MT capacity is
/// [`MaxLen::Unknown`] (settlement amount) are never reported `Truncated`.
///
/// This never panics on caller input: every failure mode is a `Result` or
/// an `Option`.
pub fn diff_mt_mx(mt: &WfMt, mx: &WfMx) -> Result<DiffReport, XformError> {
    let mt_msg = structural_view(mt)?;

    // Confirm the MX side is the only document type this detector maps.
    // The transaction values are read inside `extract_mx`, which keeps the
    // (non-re-exported) upstream transaction type out of this signature.
    match mx.document() {
        Document::Pacs008(_) => {}
        other => return Err(XformError::mx_not_pacs008(document_kind(other))),
    }

    let mut rows = Vec::with_capacity(Role::ALL.len());
    for role in Role::ALL {
        let mt_value = extract_mt(role, &mt_msg);
        let mx_value = extract_mx(role, mx.document());
        let diff = classify(role, mt_value.as_deref(), mx_value.as_deref());
        rows.push(DiffRow {
            role,
            mt_value,
            mx_value,
            diff,
        });
    }
    Ok(DiffReport { rows })
}

/// Obtain an owned structural [`MtMessage`] from either [`WfMt`] path.
///
/// The structural path is cloned as-is. The typed path is re-serialised to
/// MT wire text (upstream `SwiftMessage::to_mt_message`) and tokenised
/// structurally; if the typed body is not an MT103, or its re-serialised
/// text is not well-framed, the message is unusable for this detector and
/// an [`XformError`] is returned rather than panicking.
///
/// Only MT103 is mapped here because the role-map is MT103↔pacs.008. The
/// upstream `ParsedSwiftMessage` exposes per-type accessors but no
/// whole-enum serialiser, so we reach the `SwiftMessage<MT103>` body via
/// `as_mt103()` and call its `to_mt_message()`.
fn structural_view(mt: &WfMt) -> Result<MtMessage, XformError> {
    match mt {
        WfMt::Structural(m) => Ok(m.clone()),
        WfMt::Typed(typed) => {
            let wire = match typed.as_mt103() {
                Some(mt103) => mt103.to_mt_message(),
                None => {
                    return Err(XformError::mt_unreadable(format!(
                        "typed MT body is type {}, but only MT103 is mapped \
                         to pacs.008",
                        typed.message_type()
                    )));
                }
            };
            wf_codec::swift::parse(&wire).map_err(|e| {
                XformError::mt_unreadable(format!(
                    "typed MT103 body re-serialised to text that did not tokenise: {e:?}"
                ))
            })
        }
    }
}

/// A short label for a `Document` variant, used only in error messages.
fn document_kind(doc: &Document) -> &'static str {
    match doc {
        Document::Pacs008(_) => "pacs.008",
        _ => "a non-pacs.008 MX document",
    }
}

/// Extract a role's value from the structural MT message, or `None` if the
/// field is absent.
fn extract_mt(role: Role, mt: &MtMessage) -> Option<String> {
    match role {
        Role::DebtorName => mt.field("50K").map(|f| join_party_lines(&f.value)),
        Role::CreditorName => mt.field("59").map(|f| join_party_lines(&f.value)),
        Role::RemittanceInfo => mt.field("70").map(|f| join_text_lines(&f.value)),
        Role::SettlementAmount => mt
            .field("32A")
            .and_then(|f| amount_component(&f.value))
            .map(normalize_amount),
        Role::SettlementCurrency => mt.field("32A").and_then(|f| currency_component(&f.value)),
    }
}

/// Extract a role's value from the typed MX document, or `None`.
///
/// Only the pacs.008 variant carries the mapped roles; any other document
/// yields `None` for every role (callers reach this only after
/// [`diff_mt_mx`] has confirmed the pacs.008 variant). Keeping the match
/// here avoids naming the upstream transaction type — which `wf-mx` does
/// not re-export — in any public signature.
fn extract_mx(role: Role, doc: &Document) -> Option<String> {
    let Document::Pacs008(body) = doc else {
        return None;
    };
    let tx = &body.cdt_trf_tx_inf;
    match role {
        Role::DebtorName => tx.dbtr.nm.clone(),
        Role::CreditorName => tx.cdtr.nm.clone(),
        Role::RemittanceInfo => tx.rmt_inf.as_ref().and_then(|r| r.ustrd.clone()),
        Role::SettlementAmount => Some(normalize_amount(format_f64(tx.intr_bk_sttlm_amt.value))),
        Role::SettlementCurrency => {
            let ccy = tx.intr_bk_sttlm_amt.ccy.trim();
            if ccy.is_empty() {
                None
            } else {
                Some(ccy.to_string())
            }
        }
    }
}

/// Classify the relationship between an MT value and an MX value for one
/// role.
///
/// The MT-side cited capacity ([`Role::mt_max_len`]) is the receiving
/// limit used to decide truncation: the question this detector answers is
/// "would the MX value fit in the MT field?".
fn classify(role: Role, mt: Option<&str>, mx: Option<&str>) -> FieldDiff {
    match (mt, mx) {
        // Both absent: nothing to compare for this role — not a loss and
        // not a disagreement.
        (None, None) => FieldDiff::BothAbsent,
        // MT-only: would be lost going to MX.
        (Some(_), None) => FieldDiff::Added,
        // MX-only: would be lost going to MT.
        (None, Some(_)) => FieldDiff::Dropped,
        (Some(mt_v), Some(mx_v)) => {
            if mt_v == mx_v {
                return FieldDiff::Equal;
            }
            // Length overflow takes precedence over a plain mismatch, but
            // only when we have a cited MT capacity to measure against.
            if let Some(cap) = role.mt_max_len().capacity() {
                let mx_chars: Vec<char> = mx_v.chars().collect();
                if mx_chars.len() > cap {
                    let lost_suffix: String = mx_chars[cap..].iter().collect();
                    return FieldDiff::Truncated { lost_suffix };
                }
            }
            if normalize_ws(mt_v) == normalize_ws(mx_v) {
                FieldDiff::Reformatted
            } else {
                FieldDiff::Mismatch
            }
        }
    }
}

/// Drop a leading `/account` line (50K / 59 first line) if present, then
/// join the remaining name/address lines with a single space.
fn join_party_lines(raw: &str) -> String {
    let mut lines = raw.split('\n').map(strip_cr_and_trim_end);
    let mut name_lines: Vec<&str> = Vec::new();
    if let Some(first) = lines.next() {
        if !first.starts_with('/') {
            name_lines.push(first);
        }
    }
    for line in lines {
        name_lines.push(line);
    }
    name_lines.join(" ").trim().to_string()
}

/// Join unstructured text lines (field 70) with a single space.
fn join_text_lines(raw: &str) -> String {
    raw.split('\n')
        .map(strip_cr_and_trim_end)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

/// Strip a single trailing carriage return (from a `\r\n` split on `\n`)
/// and any trailing spaces from one line.
fn strip_cr_and_trim_end(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line).trim_end()
}

/// Collapse runs of whitespace to single spaces and trim, for
/// reformatting comparison.
fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The amount component of a 32A value (`6!n 3!a 15d`): the substring
/// after the 6-digit date and 3-letter currency.
fn amount_component(value: &str) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    // 6 date + 3 currency = 9 leading chars before the amount.
    const PREFIX: usize = 9;
    if chars.len() <= PREFIX {
        return None;
    }
    let amount: String = chars[PREFIX..].iter().collect();
    if amount.is_empty() {
        None
    } else {
        Some(amount)
    }
}

/// The currency component of a 32A value: chars 6..9 (the 3-letter code).
fn currency_component(value: &str) -> Option<String> {
    let chars: Vec<char> = value.chars().collect();
    const DATE_LEN: usize = 6;
    const CCY_LEN: usize = 3;
    if chars.len() < DATE_LEN + CCY_LEN {
        return None;
    }
    let ccy: String = chars[DATE_LEN..DATE_LEN + CCY_LEN].iter().collect();
    Some(ccy)
}

/// Normalise an amount to a canonical decimal string for comparison:
/// unify the decimal separator to `.` and strip trailing zeros / a
/// trailing separator so `100`, `100,00`, and `100.0` compare equal.
fn normalize_amount(raw: String) -> String {
    let unified = raw.trim().replace(',', ".");
    if let Some((int_part, frac_part)) = unified.split_once('.') {
        let trimmed_frac = frac_part.trim_end_matches('0');
        if trimmed_frac.is_empty() {
            int_part.to_string()
        } else {
            format!("{int_part}.{trimmed_frac}")
        }
    } else {
        unified
    }
}

/// Format an `f64` amount without a forced fixed precision, so it can be
/// normalised the same way as the MT string.
fn format_f64(v: f64) -> String {
    // `{}` on f64 avoids trailing zeros and uses `.` as the separator.
    format!("{v}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // ---- External anchors -------------------------------------------------
    //
    // The MT and MX inputs below are concrete fills of the documented
    // message shapes. The TRUNCATION EXPECTATIONS are derived from the
    // CITED max-lengths, not from this crate's classifier:
    //   * MT 50K / 59 / 70 hold 4*35 = 140 chars (SWIFT MT103 format spec;
    //     mirrored by wf-codec field_50k.rs / field_32a.rs constants).
    //   * MX Dbtr/Nm, Cdtr/Nm, RmtInf/Ustrd each have maxLength 140
    //     (mx-message 3.1.4 pacs_008_001_08.rs: PartyIdentification1352
    //     validate_length "Nm" Some(140); PartyIdentification1353
    //     validate_length "Nm" Some(140); RemittanceInformation161
    //     validate_length "Ustrd" Some(140)).
    // So a 140-char value is the boundary; a 141-char MX name is a KNOWN
    // truncation against the 140-char MT capacity, with exactly the 141st
    // char lost.

    /// Build a pacs.008 envelope with a given debtor name, creditor name,
    /// and optional remittance Ustrd. Structure is the documented
    /// `mx-message` pacs.008 shape (same as the wf-mx facade's own test
    /// envelope); values are concrete fills.
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
    <BizMsgIdr>MSG-XF-001</BizMsgIdr>
    <MsgDefIdr>pacs.008.001.08</MsgDefIdr>
    <BizSvc>swift.cbprplus.02</BizSvc>
    <CreDt>2024-01-15T09:00:00+00:00</CreDt>
  </AppHdr>
  <Document>
    <FIToFICstmrCdtTrf>
      <GrpHdr>
        <MsgId>XF-PAY-001</MsgId>
        <CreDtTm>2024-01-15T09:00:00+00:00</CreDtTm>
        <NbOfTxs>1</NbOfTxs>
        <SttlmInf><SttlmMtd>INDA</SttlmMtd></SttlmInf>
      </GrpHdr>
      <CdtTrfTxInf>
        <PmtId>
          <InstrId>INSTR-XF-001</InstrId>
          <EndToEndId>E2E-XF-001</EndToEndId>
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

    /// Build an MT103 with the given 50K name, 59 name, optional field 70,
    /// and a fixed 32A. Lines are the raw on-the-wire multi-line layout.
    fn mt103(name_50k: &str, name_59: &str, field70: Option<&str>) -> String {
        let f70 = match field70 {
            Some(t) => format!(":70:{t}\r\n"),
            None => String::new(),
        };
        format!(
            "{{1:F01BANKUS33AXXX0000000000}}{{2:I103BANKGB22XXXXN}}{{4:\r\n\
             :20:REF-XF-001\r\n\
             :23B:CRED\r\n\
             :32A:240115USD1234,56\r\n\
             :50K:{name_50k}\r\n\
             :59:{name_59}\r\n\
             {f70}\
             :71A:OUR\r\n\
             -}}"
        )
    }

    fn parse_pair(mt: &str, mx: &str) -> (WfMt, WfMx) {
        let mt = wf_swift::parse(mt).expect("MT must parse");
        let mx = WfMx::from_xml(mx).expect("MX must parse");
        (mt, mx)
    }

    fn row(report: &DiffReport, role: Role) -> &DiffRow {
        report
            .rows
            .iter()
            .find(|r| r.role == role)
            .expect("role present in report")
    }

    /// Parse a hand-built MT103 + pacs.008 pair. Depending on field
    /// contents the upstream typed parser may accept the MT103 (Typed
    /// path) or reject it and fall back to the Structural path — e.g. a
    /// deliberately over-long 50K name violates the typed 50K line rule.
    /// `diff_mt_mx` handles BOTH paths, so this helper does not assert a
    /// particular one; the dedicated `normal_mt103_takes_typed_path` test
    /// pins the Typed extraction route specifically.
    fn built_pair(mt: &str, mx: &str) -> (WfMt, WfMx) {
        parse_pair(mt, mx)
    }

    #[test]
    fn equal_names_classified_equal() {
        // Same name on both sides, well within the 140-char cap → Equal.
        let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
        let mt = mt103("JOHN DOE", "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        assert_eq!(row(&report, Role::DebtorName).diff, FieldDiff::Equal);
        assert_eq!(row(&report, Role::CreditorName).diff, FieldDiff::Equal);
    }

    #[test]
    fn genuinely_different_name_is_mismatch() {
        // Different (not just reformatted, not over-length) creditor name.
        let mx = pacs008_envelope("JOHN DOE", "WRONG PERSON", None);
        let mt = mt103("JOHN DOE", "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        assert_eq!(row(&report, Role::CreditorName).diff, FieldDiff::Mismatch);
    }

    #[test]
    fn normal_mt103_takes_typed_path_and_is_still_extracted() {
        // A normal-length MT103 is accepted by the upstream typed parser,
        // so it takes the Typed path. This pins the typed extraction route
        // (typed body -> as_mt103().to_mt_message() -> wf_codec tokenise):
        // diff_mt_mx must still read its 50K/59 names off the Typed body.
        let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
        let mt = mt103("JOHN DOE", "JANE SMITH", None);
        let (mt, mx) = parse_pair(&mt, &mx);
        assert!(
            mt.is_typed(),
            "a normal-length MT103 is expected to take the Typed path"
        );
        let report = diff_mt_mx(&mt, &mx).expect("diff");
        assert_eq!(
            row(&report, Role::DebtorName).mt_value.as_deref(),
            Some("JOHN DOE"),
            "Typed-path MT extraction must recover the 50K debtor name"
        );
        assert_eq!(row(&report, Role::DebtorName).diff, FieldDiff::Equal);
    }

    #[test]
    fn mx_name_one_char_over_cited_cap_is_truncated() {
        // ANTI-TAUTOLOGY: the expectation is computed from the CITED caps,
        // not from the classifier.
        //   - MX Dbtr/Nm maxLength = 140 (mx-message PartyIdentification1352).
        //   - MT 50K capacity      = 4*35 = 140 (SWIFT MT103 50K format).
        // A 141-char MX name therefore CANNOT fit the 140-char MT field;
        // exactly the 141st char is lost. We give the MT 50K a 140-char
        // value (so it does NOT itself overflow) and the MX a 141-char
        // value (the MT 140 + one extra 'Z'); the detector must report the
        // single trailing 'Z' as the lost suffix.
        let mt_name: String = "A".repeat(140);
        let mx_name = format!("{mt_name}Z"); // 141 chars
        assert_eq!(mt_name.chars().count(), 140, "MT name is the 140 cap");
        assert_eq!(mx_name.chars().count(), 141, "MX name is one over cap");

        let mx = pacs008_envelope(&mx_name, "JANE SMITH", None);
        let mt = mt103(&mt_name, "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        match &row(&report, Role::DebtorName).diff {
            FieldDiff::Truncated { lost_suffix } => {
                // The cited 140-char cap predicts exactly one lost char.
                assert_eq!(
                    lost_suffix, "Z",
                    "the 141st char (beyond the cited 140 cap) must be the lost suffix"
                );
            }
            other => panic!("expected Truncated against cited 140 cap, got {other:?}"),
        }
    }

    #[test]
    fn mx_name_at_exactly_cited_cap_is_not_truncated() {
        // Boundary: a 140-char MX name EQUALS the cited 140-char MT cap, so
        // it fits — must NOT be Truncated. With identical values it is
        // Equal; this proves the boundary is inclusive (cap of 140 admits
        // 140 chars), as the cited maxLength facet (Some(140)) requires.
        let name: String = "B".repeat(140);
        let mx = pacs008_envelope(&name, "JANE SMITH", None);
        let mt = mt103(&name, "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        assert_eq!(
            row(&report, Role::DebtorName).diff,
            FieldDiff::Equal,
            "a value exactly at the cited 140 cap fits and is not truncated"
        );
    }

    #[test]
    fn remittance_present_in_mx_absent_in_mt_is_dropped() {
        // MX carries RmtInf/Ustrd; MT has no field 70 → Dropped (MX-only).
        let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", Some("INVOICE 12345"));
        let mt = mt103("JOHN DOE", "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        assert_eq!(row(&report, Role::RemittanceInfo).diff, FieldDiff::Dropped);
        assert_eq!(
            row(&report, Role::RemittanceInfo).mx_value.as_deref(),
            Some("INVOICE 12345")
        );
        assert!(row(&report, Role::RemittanceInfo).mt_value.is_none());
    }

    #[test]
    fn settlement_currency_equal_amount_unknown_skips_truncation() {
        // Currency is 3 chars both sides (USD) → Equal. Amount has an
        // Unknown MT capacity (f64 has no cited maxLength), so it must
        // never be Truncated even though the strings are compared; with
        // matching 1234,56 / 1234.56 it normalises to Equal.
        let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", None);
        let mt = mt103("JOHN DOE", "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        assert_eq!(
            row(&report, Role::SettlementCurrency).diff,
            FieldDiff::Equal,
            "USD == USD"
        );
        let amt = &row(&report, Role::SettlementAmount).diff;
        assert!(
            !matches!(amt, FieldDiff::Truncated { .. }),
            "settlement amount has Unknown MT capacity and must never be Truncated; got {amt:?}"
        );
        assert_eq!(*amt, FieldDiff::Equal, "1234,56 normalises to 1234.56");
    }

    #[test]
    fn cited_caps_are_what_the_tests_assume() {
        // Guard: make the cited capacities explicit so a future edit that
        // changes a constant breaks here loudly. These mirror the inline
        // citations on Role::mt_max_len / Role::mx_max_len.
        assert_eq!(Role::DebtorName.mt_max_len().capacity(), Some(140));
        assert_eq!(Role::DebtorName.mx_max_len().capacity(), Some(140));
        assert_eq!(Role::CreditorName.mt_max_len().capacity(), Some(140));
        assert_eq!(Role::RemittanceInfo.mx_max_len().capacity(), Some(140));
        assert_eq!(Role::SettlementCurrency.mt_max_len().capacity(), Some(3));
        assert_eq!(
            Role::SettlementAmount.mt_max_len().capacity(),
            None,
            "amount MT capacity is Unknown (f64 has no cited maxLength)"
        );
    }

    #[test]
    fn non_pacs008_mx_is_rejected_with_three_element_error() {
        // A pacs.008 is the only mapped type. Assert the error Display is
        // three-element (what failed / expected / recourse) without needing
        // to construct a different supported MX type.
        let err = XformError::mx_not_pacs008("camt.053");
        let msg = err.to_string();
        assert!(msg.contains("cannot compare"), "what failed: {msg}");
        assert!(msg.contains("expected"), "what was expected: {msg}");
        assert!(
            msg.contains("pacs.008"),
            "names the only supported type: {msg}"
        );
    }

    #[test]
    fn mx_not_address_checkable_names_supported_set() {
        // The address-compliance family supports four ISO 20022 payment
        // messages; its wrong-type error must name ALL of them and be
        // three-element (what failed / expected / recourse).
        let err = XformError::mx_not_address_checkable("camt.053");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot run SR2026 address compliance"),
            "what failed: {msg}"
        );
        assert!(
            msg.contains("pacs.008.001.08")
                && msg.contains("pacs.004.001.09")
                && msg.contains("pacs.003.001.08")
                && msg.contains("pain.001.001.09"),
            "must name the full supported set: {msg}"
        );
        assert!(
            msg.contains("check_mx_address"),
            "must point at the unified entry: {msg}"
        );
    }

    #[test]
    fn sem_field_constructs_with_cited_capacity() {
        // SemField is part of the public model; exercise it so the type is
        // covered and its max_len carries a cited capacity.
        let sf = SemField {
            role: Role::DebtorName,
            value: "JOHN DOE".to_string(),
            max_len: Role::DebtorName.mt_max_len(),
        };
        assert_eq!(sf.role, Role::DebtorName);
        assert_eq!(sf.max_len.capacity(), Some(140));
    }

    #[test]
    fn lossy_rows_lists_dropped_and_truncated() {
        // A report with one Dropped role exposes it via lossy_rows().
        let mx = pacs008_envelope("JOHN DOE", "JANE SMITH", Some("INVOICE 12345"));
        let mt = mt103("JOHN DOE", "JANE SMITH", None);
        let (mt, mx) = built_pair(&mt, &mx);
        let report = diff_mt_mx(&mt, &mx).expect("diff");

        let lossy: Vec<Role> = report.lossy_rows().map(|r| r.role).collect();
        assert!(
            lossy.contains(&Role::RemittanceInfo),
            "dropped remittance must appear in lossy_rows; got {lossy:?}"
        );
    }
}
