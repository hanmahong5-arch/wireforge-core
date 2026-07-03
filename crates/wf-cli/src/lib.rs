//! Testable core for the `wf` binary.
//!
//! Pure entry points the binary calls:
//! - [`parse_to_tree`] — ISO 8583 hex -> human-readable field tree
//! - [`parse_to_json`] — ISO 8583 hex -> JSON description
//! - [`build_from_json`] — JSON description -> ISO 8583 wire hex
//! - [`swift_parse_to_tree`] — SWIFT MT text -> human-readable block tree
//! - [`swift_parse_to_json`] — SWIFT MT text -> JSON block description
//! - [`ebcdic_decode_hex`] — EBCDIC bytes (as hex) -> decoded Unicode text
//! - [`ebcdic_encode_text`] — Unicode text -> EBCDIC bytes (as hex)
//! - [`sm3_digest`] — bytes -> lowercase SM3 hex digest
//! - [`mt_mx_truncation_diff`] — MT103 vs pacs.008 field truncation/loss
//!   DETECTOR report (not a converter)
//! - [`mt_mx_truncation_diff_from_wf`] — same detector, fed from a single
//!   `.wf` file holding a matched `swift-mt` + `mx` pair
//! - [`oracle_check`] / [`oracle_report`] — deterministic ISO 8583
//!   regression-conformance EVIDENCE: a captured legacy response vs a migrated
//!   response under an operator-approved mask spec (NOT proof / certification)
//! - [`oracle_check_from_wf`] / [`oracle_report_from_wf`] — same engine, fed
//!   from a single `.wf` holding a `req`/`legacy`/`migrated` triple +
//!   `oracle-spec`
//!
//! An MCP `wf_oracle_check` tool is intentionally **deferred** to keep the
//! server's 12-tool surface stable — the conformance engine is CLI-first.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wf_bitmap::Bitmap8583;
use wf_codec::ebcdic::{decode as ebcdic_decode, encode as ebcdic_encode, CodePage, EbcdicError};
use wf_codec::iso8583::{
    build,
    field::{field_def, DataType, FieldDef, LengthSpec},
    parse, BuildError, Iso8583Message, ParseError,
};
use wf_codec::swift::{parse as swift_parse, Block as SwiftBlock, MtMessage as SwiftMessage};
use wf_oracle::{
    check_conformance, ConformanceGate, FieldKey, FieldMask, FixedField, FixedLayout, MaskType,
    OracleReport, OracleSpec,
};
use wf_sm::sm3::sm3_hex;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

pub fn parse_to_tree(hex_input: &str) -> Result<String, String> {
    let bytes = hex_decode(&strip_whitespace(hex_input))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {}", display_parse(&e)))?;
    Ok(render_tree(&msg))
}

pub fn parse_to_json(hex_input: &str) -> Result<String, String> {
    let bytes = hex_decode(&strip_whitespace(hex_input))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {}", display_parse(&e)))?;
    let view = build_view(&msg)?;
    serde_json::to_string_pretty(&view).map_err(|e| format!("serialize json: {e}"))
}

pub fn swift_parse_to_tree(text: &str) -> Result<String, String> {
    let msg = swift_parse(text).map_err(|e| format!("swift parse: {e}"))?;
    Ok(render_swift_tree(&msg))
}

pub fn swift_parse_to_json(text: &str) -> Result<String, String> {
    let msg = swift_parse(text).map_err(|e| format!("swift parse: {e}"))?;
    let view = build_swift_view(&msg);
    serde_json::to_string_pretty(&view).map_err(|e| format!("serialize json: {e}"))
}

pub fn build_from_json(json_input: &str) -> Result<String, String> {
    let input: BuildInput =
        serde_json::from_str(json_input).map_err(|e| format!("parse json input: {e}"))?;
    if input.mti.len() != 4 || !input.mti.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "mti must be exactly 4 ASCII digits, got {:?}",
            input.mti
        ));
    }
    let mut mti = [0u8; 4];
    mti.copy_from_slice(input.mti.as_bytes());
    let mut fields = BTreeMap::new();
    for (n, value) in input.fields {
        let bytes = match value {
            FieldValue::Ascii(s) => s.into_bytes(),
            FieldValue::Hex { hex } => {
                hex_decode(&hex).map_err(|e| format!("field {n} hex: {e}"))?
            }
        };
        fields.insert(n, bytes);
    }
    let msg = Iso8583Message { mti, fields };
    let wire = build(&msg).map_err(|e| format!("build: {}", display_build(&e)))?;
    Ok(hex_encode(&wire))
}

// ---------------------------------------------------------------------------
// EBCDIC entry points
// ---------------------------------------------------------------------------

/// Decode EBCDIC bytes (supplied as a hex string) into Unicode text.
///
/// `cp` selects the code page (`"037"` or `"500"`); EBCDIC decode is total,
/// so every byte maps to some character and this only fails on bad hex or an
/// unknown code page.
pub fn ebcdic_decode_hex(hex_input: &str, cp: &str) -> Result<String, String> {
    let code_page = parse_code_page(cp)?;
    let bytes = hex_decode(&strip_whitespace(hex_input))?;
    Ok(ebcdic_decode(&bytes, code_page))
}

/// Encode Unicode text into EBCDIC bytes, returned as a lowercase hex string.
///
/// `cp` selects the code page (`"037"` or `"500"`). Fails if a character has
/// no representation in the chosen code page (the error names the character,
/// its position, and the code page).
pub fn ebcdic_encode_text(text: &str, cp: &str) -> Result<String, String> {
    let code_page = parse_code_page(cp)?;
    let bytes = ebcdic_encode(text, code_page).map_err(|e| display_ebcdic(&e))?;
    Ok(hex_encode(&bytes))
}

/// Map a `--cp` argument to a [`CodePage`]. Accepts `"037"` and `"500"`.
fn parse_code_page(cp: &str) -> Result<CodePage, String> {
    match cp {
        "037" => Ok(CodePage::Cp037),
        "500" => Ok(CodePage::Cp500),
        other => Err(format!(
            "unknown code page {other:?} — supported values are 037 and 500"
        )),
    }
}

fn display_ebcdic(e: &EbcdicError) -> String {
    match e {
        EbcdicError::Unrepresentable {
            ch,
            position,
            code_page,
        } => format!(
            "character {ch:?} at position {position} is not representable in code page {code_page:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// SM3 entry point
// ---------------------------------------------------------------------------

/// Compute the lowercase SM3 (GM/T 0004-2012) hex digest of an input.
///
/// `is_text` decides how `input` is interpreted: `true` hashes the UTF-8 bytes
/// of the string as-is; `false` hashes the bytes decoded from a hex string
/// (whitespace ignored), matching how `wf parse` reads hex. SM3 here is a plain
/// hash function with no compliance claim.
pub fn sm3_digest(input: &str, is_text: bool) -> Result<String, String> {
    let bytes = if is_text {
        input.as_bytes().to_vec()
    } else {
        hex_decode(&strip_whitespace(input))?
    };
    Ok(sm3_hex(&bytes))
}

// ---------------------------------------------------------------------------
// MX address compliance entry point
// ---------------------------------------------------------------------------

/// Scope statement for the address compliance checker, reused in the rendered
/// output header. Mirrors the MCP tool description so the two surfaces state
/// the same honest scope.
const ADDRESS_SCOPE: &str =
    "Structural CBPR+ SR2026 presence check: TwnNm + Ctry in pacs.008.001.08 / \
     pacs.004.001.09 / pacs.003.001.08 / pain.001.001.09 PstlAdr (mandatory \
     2026-11-14). NOT a full CBPR+ validation, NOT a certification.";

/// Check debtor/creditor postal-address compliance for a pacs.008.001.08,
/// pacs.004.001.09, pacs.003.001.08 or pain.001.001.09 envelope, returning a
/// readable per-party report.
///
/// `mx` is raw ISO 20022 XML (a full `<AppHdr>` + `<Document>` envelope); the
/// message type is auto-detected. This is a **structural presence check**
/// against the CBPR+ SR2026 rule that `TwnNm` and `Ctry` appear in dedicated
/// structured fields. It does not perform a full CBPR+ validation and makes no
/// certification claim.
pub fn mx_address_compliance(mx: &str) -> Result<String, String> {
    let report = mx_address_report(mx)?;
    Ok(render_address_report(&report))
}

/// Parse an MX envelope and run the SR2026 structural address check,
/// returning the structured [`wf_xform::AddressComplianceReport`].
///
/// This is the parse+check half of [`mx_address_compliance`] — that function
/// is `mx_address_report` followed by [`render_address_report`], so the
/// single-file output stays byte-identical. Exposed so a batch scan
/// ([`render_address_scan`]) can collect many reports before rendering them.
pub fn mx_address_report(mx: &str) -> Result<wf_xform::AddressComplianceReport, String> {
    let parsed_mx = wf_mx::WfMx::from_xml(mx).map_err(|e| e.to_string())?;
    wf_xform::check_mx_address(&parsed_mx).map_err(|e| e.to_string())
}

/// The diff-style outcome of an address-compliance scan, mapped to a process
/// exit code so the check can gate CI. The 0/1/2 split mirrors `diff(1)`:
/// 0 = every input compliant, 1 = ran cleanly but found non-compliance,
/// 2 = at least one input could not be checked at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressGate {
    /// Every scanned input is SR2026-compliant (both parties carry
    /// `TwnNm` + `Ctry`).
    AllCompliant,
    /// Every input was checkable, but at least one party is non-compliant.
    FoundNonCompliant,
    /// At least one input could not be checked (read / parse / unsupported
    /// message type).
    HadErrors,
}

impl AddressGate {
    /// The process exit code this gate maps to: 0 / 1 / 2.
    pub fn code(self) -> u8 {
        match self {
            AddressGate::AllCompliant => 0,
            AddressGate::FoundNonCompliant => 1,
            AddressGate::HadErrors => 2,
        }
    }
}

/// One input's place in an address-compliance scan: a display label (the file
/// path, or `-` for stdin) and the per-input check result.
///
/// A read / parse / unsupported-type failure is captured here as `Err` rather
/// than aborting the batch, so one bad file does not hide the other files'
/// verdicts and still surfaces in the aggregate exit code.
#[derive(Debug)]
pub struct ScanEntry {
    /// Display label for this input (file path, or `-` for stdin).
    pub label: String,
    /// The per-input check result, or the error that made it uncheckable.
    pub result: Result<wf_xform::AddressComplianceReport, String>,
}

/// Render a batch of [`ScanEntry`] into a report body plus the [`AddressGate`]
/// the batch implies.
///
/// A single entry preserves the original single-file UX: the full per-party
/// tree on success ([`render_address_report`], byte-identical to
/// [`mx_address_compliance`]), or a one-line `✗ <label>: <error>` on failure.
/// Two-or-more entries render one compact line per file
/// (`STATUS  <label>  <message_type>  debtor=… creditor=…`), then a summary
/// footer and the [`ADDRESS_SCOPE`] line (which carries the 2026-11-14
/// deadline as a static string — no wall-clock, so output stays
/// deterministic).
///
/// The gate is derived from the SR2026 verdicts, not from any per-fixture
/// assumption: any `Err` entry dominates (→ [`AddressGate::HadErrors`]), else
/// any non-compliant report (→ [`AddressGate::FoundNonCompliant`]), else
/// [`AddressGate::AllCompliant`].
pub fn render_address_scan(entries: &[ScanEntry]) -> (String, AddressGate) {
    use std::fmt::Write as _;

    let (compliant, non_compliant, errors, gate) = tally_address_scan(entries);

    // Single input: keep the original single-file experience exactly.
    if entries.len() == 1 {
        let e = &entries[0];
        let body = match &e.result {
            Ok(report) => render_address_report(report),
            Err(err) => format!("✗ {}: {err}\n", e.label),
        };
        return (body, gate);
    }

    // Multi-input: one compact line per file, then a summary + scope footer.
    let mut out = String::new();
    out.push_str("MX Address Compliance Scan (CBPR+ SR2026)\n");
    for e in entries {
        match &e.result {
            Ok(report) => {
                let status = if report.all_compliant() {
                    "PASS"
                } else {
                    "FAIL"
                };
                let mut parties = String::new();
                for r in &report.rows {
                    let _ = write!(
                        parties,
                        "{}={} ",
                        r.party.as_str(),
                        address_verdict_label(&r.verdict)
                    );
                }
                let _ = writeln!(
                    out,
                    "{status:<5} {}  {}  {}",
                    e.label,
                    report.message_type,
                    parties.trim_end()
                );
            }
            Err(err) => {
                let _ = writeln!(out, "{:<5} {}  {err}", "ERROR", e.label);
            }
        }
    }
    let _ = writeln!(
        out,
        "scanned {} · compliant {compliant} · non-compliant {non_compliant} · errors {errors}",
        entries.len()
    );
    let _ = writeln!(out, "scope: {ADDRESS_SCOPE}");
    (out, gate)
}

/// Tally a batch of [`ScanEntry`] into `(compliant, non_compliant, errors,
/// gate)`. The single source of truth for the SR2026 gate rule (any `Err`
/// dominates → [`AddressGate::HadErrors`]; else any non-compliant report →
/// [`AddressGate::FoundNonCompliant`]; else [`AddressGate::AllCompliant`]), so
/// [`render_address_scan`], [`render_address_scan_json`] and
/// [`render_address_scan_csv`] cannot disagree on the gate for the same
/// entries.
fn tally_address_scan(entries: &[ScanEntry]) -> (usize, usize, usize, AddressGate) {
    let mut compliant = 0usize;
    let mut non_compliant = 0usize;
    let mut errors = 0usize;
    for e in entries {
        match &e.result {
            Err(_) => errors += 1,
            Ok(report) if report.all_compliant() => compliant += 1,
            Ok(_) => non_compliant += 1,
        }
    }
    let gate = if errors > 0 {
        AddressGate::HadErrors
    } else if non_compliant > 0 {
        AddressGate::FoundNonCompliant
    } else {
        AddressGate::AllCompliant
    };
    (compliant, non_compliant, errors, gate)
}

/// Render a batch of [`ScanEntry`] as the same SR2026 gate in machine-readable
/// JSON, alongside the [`AddressGate`] the batch implies (identical to
/// [`render_address_scan`]'s gate for the same entries — both derive from
/// [`tally_address_scan`]).
///
/// Shape: `{ "schema_version": "1.0", "tool": "wf xform address-check",
/// "scope": <ADDRESS_SCOPE>, "gate": "all_compliant"|"found_non_compliant"|
/// "had_errors", "exit_code": 0|1|2, "summary": {"scanned","compliant",
/// "non_compliant","errors"}, "results": [ per-entry ] }`. Each ok entry
/// merges in `report.to_json()` verbatim (message_type, compliant, rows) so
/// the row shape is defined once, in wf-xform; an error entry instead carries
/// `"status": "error"` and `"error": <msg>`.
pub fn render_address_scan_json(entries: &[ScanEntry]) -> (String, AddressGate) {
    let (compliant, non_compliant, errors, gate) = tally_address_scan(entries);

    let gate_str = match gate {
        AddressGate::AllCompliant => "all_compliant",
        AddressGate::FoundNonCompliant => "found_non_compliant",
        AddressGate::HadErrors => "had_errors",
    };

    let results: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| match &e.result {
            Ok(report) => {
                let mut v = report.to_json();
                if let serde_json::Value::Object(map) = &mut v {
                    map.insert("label".to_string(), serde_json::Value::String(e.label.clone()));
                    map.insert(
                        "status".to_string(),
                        serde_json::Value::String("ok".to_string()),
                    );
                }
                v
            }
            Err(err) => serde_json::json!({
                "label": e.label,
                "status": "error",
                "error": err,
            }),
        })
        .collect();

    let doc = serde_json::json!({
        "schema_version": "1.0",
        "tool": "wf xform address-check",
        "scope": ADDRESS_SCOPE,
        "gate": gate_str,
        "exit_code": gate.code(),
        "summary": {
            "scanned": entries.len(),
            "compliant": compliant,
            "non_compliant": non_compliant,
            "errors": errors,
        },
        "results": results,
    });
    let body = serde_json::to_string_pretty(&doc)
        .unwrap_or_else(|e| format!("{{\"error\": \"serialize json: {e}\"}}"));
    (body, gate)
}

/// Render a batch of [`ScanEntry`] as RFC-4180 CSV, alongside the
/// [`AddressGate`] the batch implies (identical to [`render_address_scan`]'s
/// gate for the same entries — both derive from [`tally_address_scan`]).
///
/// Header: `file,status,message_type,party,verdict,town_name,country,
/// unstructured_lines,remediation`. One row per `(file, party)` for an ok
/// entry (built from `report.to_json()`'s rows — the row shape is not
/// re-derived here); one row per error entry (`status=error`, the error text
/// in the `remediation` column, other data cells empty). No summary/footer
/// line.
pub fn render_address_scan_csv(entries: &[ScanEntry]) -> (String, AddressGate) {
    use std::fmt::Write as _;

    let (_compliant, _non_compliant, _errors, gate) = tally_address_scan(entries);

    let mut out = String::new();
    out.push_str(
        "file,status,message_type,party,verdict,town_name,country,unstructured_lines,remediation\r\n",
    );
    for e in entries {
        match &e.result {
            Ok(report) => {
                let json = report.to_json();
                let message_type = json
                    .get("message_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let rows = json.get("rows").and_then(|v| v.as_array());
                if let Some(rows) = rows {
                    for row in rows {
                        let party = json_str_field(row, "party");
                        let verdict = json_str_field(row, "verdict");
                        let town_name = json_str_field(row, "town_name");
                        let country = json_str_field(row, "country");
                        let unstructured_lines = row
                            .get("unstructured_lines")
                            .map(|v| v.to_string())
                            .unwrap_or_default();
                        let remediation = json_str_field(row, "remediation");
                        let _ = writeln!(
                            out,
                            "{},{},{},{},{},{},{},{},{}\r",
                            csv_field(&e.label),
                            csv_field("ok"),
                            csv_field(message_type),
                            csv_field(&party),
                            csv_field(&verdict),
                            csv_field(&town_name),
                            csv_field(&country),
                            csv_field(&unstructured_lines),
                            csv_field(&remediation),
                        );
                    }
                }
            }
            Err(err) => {
                let _ = writeln!(
                    out,
                    "{},{},,,,,,,{}\r",
                    csv_field(&e.label),
                    csv_field("error"),
                    csv_field(err),
                );
            }
        }
    }
    (out, gate)
}

/// Read a row field as a string, `""` for JSON `null` / missing / non-string.
fn json_str_field(row: &serde_json::Value, key: &str) -> String {
    row.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// RFC-4180 quote a single CSV field: quoted (inner `"` doubled) iff it
/// contains a comma, double-quote, CR, or LF.
fn csv_field(value: &str) -> String {
    if value.contains(['"', ',', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Filter a set of names to `*.xml` (case-insensitive) and return them sorted.
///
/// Pure, so the directory-scan selection is unit-testable without touching the
/// filesystem; the sort makes a directory scan's per-file order deterministic
/// regardless of the platform's `read_dir` ordering.
pub fn select_xml(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut selected: Vec<String> = names
        .into_iter()
        .filter(|n| {
            std::path::Path::new(n)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        })
        .collect();
    selected.sort();
    selected
}

/// Render an [`wf_xform::AddressComplianceReport`] as a per-party tree with a
/// scope header naming the detected message type.
fn render_address_report(report: &wf_xform::AddressComplianceReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("MX Address Compliance (CBPR+ SR2026)\n");
    let _ = writeln!(out, "  message_type: {}", report.message_type);
    let _ = writeln!(out, "  scope: {ADDRESS_SCOPE}");
    out.push_str("Parties:\n");
    let n = report.rows.len();
    for (i, r) in report.rows.iter().enumerate() {
        let last = i + 1 == n;
        let branch = if last { "└──" } else { "├──" };
        let verdict = address_verdict_label(&r.verdict);
        let detail = address_detail(r);
        let _ = writeln!(out, "  {branch} {}: {verdict}{detail}", r.party.as_str());
    }
    out
}

/// Stable lowercase verdict label for a [`wf_xform::AddressVerdict`].
fn address_verdict_label(v: &wf_xform::AddressVerdict) -> &'static str {
    use wf_xform::AddressVerdict;
    match v {
        AddressVerdict::Compliant => "compliant",
        AddressVerdict::MissingStructured { .. } => "missing_structured",
        AddressVerdict::NoAddress => "no_address",
    }
}

/// Detail suffix for one address row (empty for `Compliant`).
fn address_detail(row: &wf_xform::AddressRow) -> String {
    use wf_xform::AddressVerdict;
    match &row.verdict {
        AddressVerdict::Compliant => {
            let tn = row.town_name.as_deref().unwrap_or("?");
            let ct = row.country.as_deref().unwrap_or("?");
            format!(" (TwnNm={tn:?}, Ctry={ct:?})")
        }
        AddressVerdict::MissingStructured {
            town_name_present,
            country_present,
            unstructured_lines,
        } => {
            format!(
                " (TwnNm={town_name_present}, Ctry={country_present}, \
                 AdrLines={unstructured_lines})"
            )
        }
        AddressVerdict::NoAddress => " (no PstlAdr element)".to_string(),
    }
}

// ---------------------------------------------------------------------------
// MT/MX truncation diff entry point
// ---------------------------------------------------------------------------

/// One-line scope statement reused in the rendered output header. Mirrors
/// the MCP tool description and the CLI long help so the three surfaces
/// state the same honest scope.
const XFORM_SCOPE: &str = "DETECTOR (not a converter): MT103 vs pacs.008.001.08, five roles only. \
No certification, conformance, or equivalence claim.";

/// Detect field truncation / loss between a SWIFT MT103 and an ISO 20022
/// pacs.008.001.08, returning a readable per-role report.
///
/// `mt` is raw SWIFT MT103 wire text; `mx` is raw ISO 20022 XML (a full
/// `<AppHdr>` + `<Document>` envelope). This is a **detector**: it does not
/// convert either side and makes no certification or equivalence claim.
/// Parse / diff failures are returned as `Err` (the binary turns these into
/// a non-zero exit), carrying the facades' three-element messages.
pub fn mt_mx_truncation_diff(mt: &str, mx: &str) -> Result<String, String> {
    let parsed_mt = wf_swift::parse(mt).map_err(|e| e.to_string())?;
    let parsed_mx = wf_mx::WfMx::from_xml(mx).map_err(|e| e.to_string())?;
    let report = wf_xform::diff_mt_mx(&parsed_mt, &parsed_mx).map_err(|e| e.to_string())?;
    Ok(render_xform_report(&report))
}

/// Run the MT/MX truncation detector against a single `.wf` source string
/// that holds a matched `swift-mt` + `mx` pair.
///
/// Parses the `.wf` text, extracts the reconstructed MT FIN wire and the
/// opaque MX envelope, then defers to [`mt_mx_truncation_diff`] — the
/// detector logic is not duplicated. Parse / extraction / diff failures
/// are returned as `Err` carrying the underlying three-element message.
pub fn mt_mx_truncation_diff_from_wf(wf_src: &str) -> Result<String, String> {
    let file = wf_format::parse(wf_src).map_err(|e| e.to_string())?;
    let (mt_wire, mx_xml) = wf_format::extract_mt_mx_pair(&file).map_err(|e| e.to_string())?;
    mt_mx_truncation_diff(&mt_wire, &mx_xml)
}

/// Render a [`wf_xform::DiffReport`] as a per-role tree with a header line
/// stating the detector scope.
fn render_xform_report(report: &wf_xform::DiffReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("MT/MX Truncation & Loss Detector\n");
    let _ = writeln!(out, "  scope: {XFORM_SCOPE}");
    out.push_str("Roles:\n");
    let n = report.rows.len();
    for (i, r) in report.rows.iter().enumerate() {
        let last = i + 1 == n;
        let branch = if last { "└──" } else { "├──" };
        let _ = writeln!(
            out,
            "  {branch} {}: {}{}",
            r.role.as_str(),
            xform_verdict(&r.diff),
            xform_detail(r)
        );
    }
    out
}

/// Stable lowercase verdict label for a [`wf_xform::FieldDiff`].
fn xform_verdict(diff: &wf_xform::FieldDiff) -> &'static str {
    use wf_xform::FieldDiff;
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

/// The verdict-specific detail suffix for one role row.
fn xform_detail(row: &wf_xform::DiffRow) -> String {
    use wf_xform::FieldDiff;
    match &row.diff {
        FieldDiff::Truncated { lost_suffix } => {
            format!(
                " (lost {} char(s): {lost_suffix:?})",
                lost_suffix.chars().count()
            )
        }
        FieldDiff::Dropped => match &row.mx_value {
            Some(v) => format!(" (MX-only: {v:?})"),
            None => String::new(),
        },
        FieldDiff::Added => match &row.mt_value {
            Some(v) => format!(" (MT-only: {v:?})"),
            None => String::new(),
        },
        FieldDiff::Mismatch | FieldDiff::Reformatted => match (&row.mt_value, &row.mx_value) {
            (Some(mt), Some(mx)) => format!(" (MT {mt:?} vs MX {mx:?})"),
            _ => String::new(),
        },
        FieldDiff::Equal => String::new(),
        FieldDiff::BothAbsent => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Oracle conformance EVIDENCE entry points
// ---------------------------------------------------------------------------

/// Produce ISO 8583 regression-conformance EVIDENCE comparing a captured
/// legacy response against a migrated response, returning the structured
/// [`OracleReport`].
///
/// `req` / `legacy` / `migrated` are raw ISO 8583 wire bytes; `spec_toml` is
/// the operator-approved mask specification (see [`parse_oracle_spec_toml`]).
/// This is the parse+check half of [`oracle_check`] — exposed so the binary
/// can read the report's gate before rendering. Returns `Err` (→ CLI exit 2)
/// when the spec is malformed or any message fails to parse.
pub fn oracle_report(
    req: &[u8],
    legacy: &[u8],
    migrated: &[u8],
    spec_toml: &str,
) -> Result<OracleReport, String> {
    let spec = parse_oracle_spec_toml(spec_toml)?;
    check_conformance(req, legacy, migrated, &spec)
}

/// Render ISO 8583 regression-conformance EVIDENCE as text.
///
/// `oracle_report` followed by [`OracleReport::render`], so a single-input
/// invocation prints the same body [`render_oracle_scan`] produces for one
/// entry. This is **EVIDENCE**, not a proof, certification, or equivalence
/// claim.
pub fn oracle_check(
    req: &[u8],
    legacy: &[u8],
    migrated: &[u8],
    spec_toml: &str,
) -> Result<String, String> {
    Ok(oracle_report(req, legacy, migrated, spec_toml)?.render())
}

/// Produce the conformance EVIDENCE [`OracleReport`] from a single `.wf`
/// source that holds a `req`/`legacy`/`migrated` ISO 8583 triple plus an
/// `oracle-spec` block.
///
/// Parses the `.wf`, extracts the three role-tagged `iso8583` bodies
/// ([`wf_format::extract_oracle_triple`]), reconstructs each to wire bytes,
/// builds the [`OracleSpec`] from the `oracle-spec` block, then defers to the
/// engine. The `.wf`→`OracleSpec` adapter lives here (not in wf-format) so
/// wf-format stays zero-dependency.
pub fn oracle_report_from_wf(wf_src: &str) -> Result<OracleReport, String> {
    let file = wf_format::parse(wf_src).map_err(|e| e.to_string())?;
    let (req_body, legacy_body, migrated_body) =
        wf_format::extract_oracle_triple(&file).map_err(|e| e.to_string())?;
    let spec = oracle_spec_from_wf(&file)?;
    let req = iso8583_body_to_wire(&req_body)?;
    let legacy = iso8583_body_to_wire(&legacy_body)?;
    let migrated = iso8583_body_to_wire(&migrated_body)?;
    check_conformance(&req, &legacy, &migrated, &spec)
}

/// Render conformance EVIDENCE from a single `.wf` oracle source.
///
/// [`oracle_report_from_wf`] followed by [`OracleReport::render`].
pub fn oracle_check_from_wf(wf_src: &str) -> Result<String, String> {
    Ok(oracle_report_from_wf(wf_src)?.render())
}

/// One input's place in a conformance scan: a display label and the per-input
/// EVIDENCE report (or the error that made it uncheckable).
///
/// A parse / spec / read failure is captured here as `Err` rather than
/// aborting, so it folds into the aggregate exit code via
/// [`render_oracle_scan`] (→ gate 2) instead of being lost.
#[derive(Debug)]
pub struct OracleEntry {
    /// Display label for this input (file path, `.wf` path, or a synthetic
    /// label for the four-flag form).
    pub label: String,
    /// The per-input EVIDENCE report, or the error that made it uncheckable.
    pub result: Result<OracleReport, String>,
}

/// Fold a batch of [`OracleEntry`] into a rendered body plus the
/// [`ConformanceGate`] the batch implies.
///
/// A single entry preserves the full EVIDENCE artifact on success
/// ([`OracleReport::render`]) or a one-line `✗ <label>: <error>` on failure —
/// exactly the `render_address_scan` precedent. The gate folds in diff order:
/// any `Err` dominates (→ [`ConformanceGate::HadErrors`], exit 2), else any
/// `FoundDrift` report (→ exit 1), else [`ConformanceGate::Conformant`]
/// (exit 0).
pub fn render_oracle_scan(entries: &[OracleEntry]) -> (String, ConformanceGate) {
    use std::fmt::Write as _;

    // Tally first so the gate reflects the whole batch regardless of body.
    let mut drift = false;
    let mut errors = false;
    for e in entries {
        match &e.result {
            Err(_) => errors = true,
            Ok(report) if matches!(report.gate, ConformanceGate::FoundDrift) => drift = true,
            Ok(_) => {}
        }
    }
    let gate = if errors {
        ConformanceGate::HadErrors
    } else if drift {
        ConformanceGate::FoundDrift
    } else {
        ConformanceGate::Conformant
    };

    // Single input: keep the full single-file EVIDENCE artifact.
    if entries.len() == 1 {
        let e = &entries[0];
        let body = match &e.result {
            Ok(report) => report.render(),
            Err(err) => format!("✗ {}: {err}\n", e.label),
        };
        return (body, gate);
    }

    // Multi-input: one compact line per input, then a summary footer.
    let mut out = String::new();
    out.push_str("Wireforge Conformance EVIDENCE Scan (Mode-A replay, SYNTHETIC)\n");
    for e in entries {
        match &e.result {
            Ok(report) => {
                let status = if matches!(report.gate, ConformanceGate::Conformant) {
                    "PASS"
                } else {
                    "DRIFT"
                };
                let _ = writeln!(
                    out,
                    "{status:<5} {}  {}  coverage {}% ({}/{})",
                    e.label,
                    report.interface,
                    report.coverage.pct(),
                    report.coverage.checked,
                    report.coverage.total
                );
            }
            Err(err) => {
                let _ = writeln!(out, "{:<5} {}  {err}", "ERROR", e.label);
            }
        }
    }
    (out, gate)
}

/// Parse an [`OracleSpec`] from a TOML document.
///
/// # Schema
///
/// ```toml
/// interface = "iso8583"     # optional, default "iso8583"
/// default_mask = "stable"   # optional, default "stable" (fail-closed)
///
/// [[mask]]
/// field = 11                # 0 = MTI, 1..=128 = data element
/// mask = "volatile"         # stable | volatile | crypto | intended-delta
///
/// [[mask]]
/// field = 39
/// mask = "intended-delta"
/// expect = "00"             # ASCII bytes; or `expect_hex = "3030"`
/// ```
///
/// `expect` (ASCII) and `expect_hex` (hex) are mutually exclusive and only
/// meaningful for an `intended-delta` mask; [`check_conformance`] validates
/// that contract and rejects a malformed spec (→ gate 2).
pub fn parse_oracle_spec_toml(doc: &str) -> Result<OracleSpec, String> {
    let repr: OracleSpecRepr =
        toml::from_str(doc).map_err(|e| format!("parse oracle spec toml: {e}"))?;
    let default_mask = parse_mask_token(&repr.default_mask)
        .ok_or_else(|| format!("unknown default_mask {:?}", repr.default_mask))?;
    let mut spec = OracleSpec::new(repr.interface).with_default_mask(default_mask);
    for m in repr.masks {
        let mask = parse_mask_token(&m.mask)
            .ok_or_else(|| format!("field {}: unknown mask {:?}", m.field, m.mask))?;
        let expect = resolve_expect(m.field, m.expect, m.expect_hex)?;
        spec = spec.with_mask(FieldMask {
            key: FieldKey::Iso8583(m.field),
            mask,
            expect,
        });
    }
    Ok(spec)
}

/// Map a mask token to a [`MaskType`]. Accepts both `intended-delta` and
/// `intended_delta`.
fn parse_mask_token(token: &str) -> Option<MaskType> {
    match token {
        "stable" => Some(MaskType::Stable),
        "volatile" => Some(MaskType::Volatile),
        "crypto" => Some(MaskType::Crypto),
        "intended-delta" | "intended_delta" => Some(MaskType::IntendedDelta),
        _ => None,
    }
}

/// Resolve a mask's expected bytes from the mutually-exclusive `expect`
/// (ASCII) / `expect_hex` (hex) fields.
fn resolve_expect(
    field: u8,
    expect: Option<String>,
    expect_hex: Option<String>,
) -> Result<Option<Vec<u8>>, String> {
    match (expect, expect_hex) {
        (Some(_), Some(_)) => Err(format!(
            "field {field}: set at most one of `expect` / `expect_hex`"
        )),
        (Some(s), None) => Ok(Some(s.into_bytes())),
        (None, Some(h)) => Ok(Some(
            hex_decode(&strip_whitespace(&h))
                .map_err(|e| format!("field {field} expect_hex: {e}"))?,
        )),
        (None, None) => Ok(None),
    }
}

/// Build an [`OracleSpec`] from a `.wf` `oracle-spec` raw block.
///
/// Recognised entries: `interface: <name>`, `default: <mask>`, and one
/// `field <N>: <mask> [<expect>]` per masked field, where `<expect>` is an
/// ASCII string or `hex:<…>`. Unknown entries are ignored. A `.wf` with no
/// `oracle-spec` block is an error (the spec must be explicit — no silent
/// all-stable default for a whole capture).
fn oracle_spec_from_wf(file: &wf_format::WfFile) -> Result<OracleSpec, String> {
    let raw = file
        .bodies
        .iter()
        .find_map(|b| match b {
            wf_format::Body::Raw(r) if r.name == "oracle-spec" => Some(r),
            _ => None,
        })
        .ok_or(
            "no `oracle-spec` block found; expected a `.wf` holding three role-tagged `iso8583` \
             blocks plus an `oracle-spec` block; add the missing `oracle-spec` block",
        )?;

    let interface = raw
        .entries
        .get("interface")
        .cloned()
        .unwrap_or_else(|| "iso8583".to_string());
    let default_mask = match raw.entries.get("default") {
        Some(tok) => parse_mask_token(tok)
            .ok_or_else(|| format!("oracle-spec `default`: unknown mask {tok:?}"))?,
        None => MaskType::Stable,
    };
    let mut spec = OracleSpec::new(interface).with_default_mask(default_mask);
    for (key, value) in &raw.entries {
        // Only `field <N>: …` lines define masks; interface/default are
        // handled above and any other key is ignored.
        let Some(n_str) = key.strip_prefix("field ") else {
            continue;
        };
        let n: u8 = n_str
            .parse()
            .map_err(|_| format!("oracle-spec `{key}`: field number must be 0..=255"))?;
        let (mask, expect) = parse_wf_mask_value(n, value)?;
        spec = spec.with_mask(FieldMask {
            key: FieldKey::Iso8583(n),
            mask,
            expect,
        });
    }
    Ok(spec)
}

/// Parse a `.wf` `oracle-spec` mask value: a mask token optionally followed by
/// whitespace and an `expect` value (ASCII, or `hex:<…>`).
fn parse_wf_mask_value(field: u8, value: &str) -> Result<(MaskType, Option<Vec<u8>>), String> {
    let mut parts = value.splitn(2, char::is_whitespace);
    let token = parts.next().unwrap_or("");
    let rest = parts.next().map(str::trim);
    let mask = parse_mask_token(token)
        .ok_or_else(|| format!("oracle-spec field {field}: unknown mask {token:?}"))?;
    let expect = match rest {
        Some(r) if !r.is_empty() => {
            let bytes = match r.strip_prefix("hex:") {
                Some(h) => hex_decode(&strip_whitespace(h))
                    .map_err(|e| format!("oracle-spec field {field} expect hex: {e}"))?,
                None => r.as_bytes().to_vec(),
            };
            Some(bytes)
        }
        _ => None,
    };
    Ok((mask, expect))
}

/// Reconstruct ISO 8583 wire bytes from a `.wf` [`wf_format::Iso8583Body`].
///
/// Field values are carried verbatim as their UTF-8 bytes (the same decoded
/// payload representation the codec uses), mirroring how `build_from_json`
/// treats ASCII field values. Build failures carry the codec's three-element
/// message.
fn iso8583_body_to_wire(body: &wf_format::Iso8583Body) -> Result<Vec<u8>, String> {
    let mti_str = body
        .mti
        .as_deref()
        .ok_or("iso8583 block is missing a `mti:` line")?;
    if mti_str.len() != 4 || !mti_str.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "mti must be exactly 4 ASCII digits, got {mti_str:?}"
        ));
    }
    let mut mti = [0u8; 4];
    mti.copy_from_slice(mti_str.as_bytes());
    let mut fields = BTreeMap::new();
    for (n, value) in &body.fields {
        fields.insert(*n, value.as_bytes().to_vec());
    }
    let msg = Iso8583Message { mti, fields };
    build(&msg).map_err(|e| format!("build: {}", display_build(&e)))
}

/// TOML deserialisation shape for an [`OracleSpec`]. See
/// [`parse_oracle_spec_toml`] for the schema.
#[derive(Debug, Deserialize)]
struct OracleSpecRepr {
    #[serde(default = "default_interface")]
    interface: String,
    #[serde(default = "default_mask_token")]
    default_mask: String,
    #[serde(default, rename = "mask")]
    masks: Vec<MaskRepr>,
}

#[derive(Debug, Deserialize)]
struct MaskRepr {
    field: u8,
    mask: String,
    #[serde(default)]
    expect: Option<String>,
    #[serde(default)]
    expect_hex: Option<String>,
}

fn default_interface() -> String {
    "iso8583".to_string()
}

fn default_mask_token() -> String {
    "stable".to_string()
}

// ---------------------------------------------------------------------------
// Fixed-layout structural check entry points
// ---------------------------------------------------------------------------

/// Check a fixed-length layout draft against every frame found in a
/// `bcl_dump`-style trace, returning the rendered report plus the diff-style
/// exit code (0 = at least one frame tiled, 1 = no frame tiled, 2 =
/// uncheckable input).
///
/// This is the "one-command verification" step of the spec-recovery loop: a
/// field table drafted from an interface spec is checked against captured
/// bytes **before** anyone trusts it. It is a **structural** check only — a
/// layout "matches" a frame when its declared field lengths account for every
/// byte; nothing is claimed about field values or semantics.
pub fn layout_check_trace(layout_toml: &str, trace: &[u8]) -> (String, u8) {
    let layout = match parse_fixed_layout_toml(layout_toml) {
        Ok(l) => l,
        Err(e) => return (format!("✗ layout: {e}\n"), 2),
    };
    let (frames, dropped) = extract_bcl_frames(trace);
    render_layout_check(&layout, &frames, dropped)
}

/// Check a fixed-length layout draft against a single raw frame. Same
/// semantics and exit-code mapping as [`layout_check_trace`].
pub fn layout_check_frame(layout_toml: &str, frame: &[u8]) -> (String, u8) {
    let layout = match parse_fixed_layout_toml(layout_toml) {
        Ok(l) => l,
        Err(e) => return (format!("✗ layout: {e}\n"), 2),
    };
    render_layout_check(&layout, std::slice::from_ref(&frame.to_vec()), 0)
}

/// Parse a [`FixedLayout`] from a TOML document.
///
/// # Schema
///
/// ```toml
/// name = "cmc svc_00 response"   # optional label
///
/// [[field]]
/// name = "msg_len"
/// len = 4                        # fixed byte length (> 0)
///
/// [[field]]
/// name = "body"
/// rest = true                    # variable tail; last field only
/// ```
///
/// Each `[[field]]` must set exactly one of `len` / `rest`.
pub fn parse_fixed_layout_toml(doc: &str) -> Result<FixedLayout, String> {
    let repr: LayoutRepr = toml::from_str(doc).map_err(|e| format!("parse layout toml: {e}"))?;
    let mut fields = Vec::with_capacity(repr.fields.len());
    for (i, f) in repr.fields.into_iter().enumerate() {
        let field = match (f.len, f.rest) {
            (Some(n), false) => FixedField::bytes(f.name, n),
            (None, true) => FixedField::rest(f.name),
            _ => {
                return Err(format!(
                    "layout field {i} ({:?}): set exactly one of `len` / `rest = true`",
                    f.name
                ));
            }
        };
        fields.push(field);
    }
    FixedLayout::new(repr.name.unwrap_or_else(|| "layout".to_string()), fields)
}

/// Extract raw frames from a Starring `bcl_dump`-style trace: blocks framed by
/// `[buffer dump: … length=N]` … `[buffer dump end]`, whose body lines carry
/// up to 16 space-separated hex byte pairs followed by an ASCII gutter.
///
/// Returns `(frames, dropped)` where `dropped` counts dumps that ended (or hit
/// EOF / a corrupt line) before `N` bytes were read — reported, never silently
/// swallowed. The trace may contain non-UTF-8 (GBK) log text; parsing is
/// line-wise over raw bytes, so that text is skipped, not fatal.
pub fn extract_bcl_frames(trace: &[u8]) -> (Vec<Vec<u8>>, usize) {
    const DUMP_HEADER: &[u8] = b"[buffer dump:";
    const DUMP_END: &[u8] = b"[buffer dump end]";
    const BYTES_PER_LINE: usize = 16;

    let mut frames: Vec<Vec<u8>> = Vec::new();
    let mut dropped = 0usize;
    // (remaining bytes, accumulated frame) while inside a dump block.
    let mut active: Option<(usize, Vec<u8>)> = None;

    for line in trace.split(|&b| b == b'\n') {
        if line.starts_with(DUMP_HEADER) {
            // A new header while a dump is still open ⇒ the previous dump was
            // truncated.
            if active.is_some() {
                dropped += 1;
            }
            active = parse_dump_length(line).map(|n| (n, Vec::with_capacity(n)));
            continue;
        }
        let Some((remaining, mut buf)) = active.take() else {
            continue;
        };
        if line.starts_with(DUMP_END) {
            // End marker before all N bytes were seen ⇒ truncated dump.
            dropped += 1;
            continue;
        }
        // Take up to min(16, remaining) hex byte pairs from the line start.
        // The gutter's ASCII column never gets read: the hex region always
        // emits exactly that many tokens first.
        let want = remaining.min(BYTES_PER_LINE);
        let mut took = 0usize;
        let mut ok = true;
        for token in line.split(|&b| b == b' ').filter(|t| !t.is_empty()) {
            if took == want {
                break;
            }
            match parse_hex_pair(token) {
                Some(byte) => {
                    buf.push(byte);
                    took += 1;
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || (took != want && took != 0) {
            // Corrupt hex region: abandon this dump, keep scanning.
            dropped += 1;
            continue;
        }
        let remaining = remaining - took;
        if remaining == 0 {
            frames.push(buf);
        } else {
            active = Some((remaining, buf));
        }
    }
    if active.is_some() {
        dropped += 1;
    }
    (frames, dropped)
}

/// Parse the `length=N` value out of a `[buffer dump: …]` header line.
/// Returns `None` for a missing / non-numeric / zero length (an empty dump
/// carries no frame).
fn parse_dump_length(line: &[u8]) -> Option<usize> {
    const KEY: &[u8] = b"length=";
    let start = line
        .windows(KEY.len())
        .position(|w| w == KEY)
        .map(|p| p + KEY.len())?;
    let digits: Vec<u8> = line[start..]
        .iter()
        .copied()
        .take_while(u8::is_ascii_digit)
        .collect();
    let n: usize = std::str::from_utf8(&digits).ok()?.parse().ok()?;
    if n == 0 {
        None
    } else {
        Some(n)
    }
}

/// Parse a two-character hex token (`"3f"` / `"3F"`) into its byte.
fn parse_hex_pair(token: &[u8]) -> Option<u8> {
    if token.len() != 2 {
        return None;
    }
    let hi = decode_hex_digit(token[0])?;
    let lo = decode_hex_digit(token[1])?;
    Some((hi << 4) | lo)
}

fn decode_hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Render the structural check of `layout` against `frames` and fold the
/// result into the diff-style exit code.
///
/// Per-frame "matched" means [`FixedLayout::parse`] tiled the frame exactly.
/// Frames are grouped by byte length (a trace mixes segments — requests,
/// different services' responses — and a layout is only expected to explain
/// its own segment's frames), so the report reads as "this layout explains
/// all N frames of length L".
fn render_layout_check(layout: &FixedLayout, frames: &[Vec<u8>], dropped: usize) -> (String, u8) {
    use std::fmt::Write as _;

    let mut out = String::new();
    out.push_str("Fixed-Layout Structural Check\n");
    let shape = if layout.has_rest() {
        format!("fixed {} byte(s) + variable tail", layout.fixed_len_sum())
    } else {
        format!("tiles exactly {} byte(s)", layout.fixed_len_sum())
    };
    let _ = writeln!(
        out,
        "  layout: {} ({} fields, {shape})",
        layout.name,
        layout.fields.len()
    );
    out.push_str(
        "  note: structural check only — a match means the declared field lengths\n\
         \x20       account for every byte of a frame; field values and semantics are\n\
         \x20       NOT validated.\n",
    );

    if frames.is_empty() {
        let _ = writeln!(out, "  frames: 0 extracted{}", dropped_note(dropped));
        out.push_str("  gate: uncheckable — no frames found in the input (exit 2)\n");
        return (out, 2);
    }

    // Group by frame length: (count, matched) — deterministic BTreeMap order.
    let mut by_len: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut matched_total = 0usize;
    for frame in frames {
        let entry = by_len.entry(frame.len()).or_insert((0, 0));
        entry.0 += 1;
        if layout.parse(frame).is_ok() {
            entry.1 += 1;
            matched_total += 1;
        }
    }
    let _ = writeln!(
        out,
        "  frames: {} extracted{}",
        frames.len(),
        dropped_note(dropped)
    );
    let _ = writeln!(
        out,
        "  matched: {matched_total}/{} frames tiled by this layout",
        frames.len()
    );
    out.push_str("  by frame length:\n");
    for (len, (count, matched)) in &by_len {
        let verdict = if *matched == *count {
            "matched"
        } else if *matched == 0 {
            "-"
        } else {
            // With all-fixed layouts a length either tiles or not; a partial
            // split can only happen with a variable tail + corrupt frames.
            "partial"
        };
        let _ = writeln!(out, "    {len:>6} B x{count:<4} {verdict}");
    }
    let code = if matched_total > 0 { 0 } else { 1 };
    let gate_line = if code == 0 {
        "  gate: layout explains at least one captured frame (exit 0)\n"
    } else {
        "  gate: layout explains NO captured frame — the draft disagrees with the bytes (exit 1)\n"
    };
    out.push_str(gate_line);
    (out, code)
}

/// The `, N incomplete dump(s) discarded` suffix, empty when clean.
fn dropped_note(dropped: usize) -> String {
    if dropped == 0 {
        String::new()
    } else {
        format!(", {dropped} incomplete dump(s) discarded")
    }
}

/// TOML deserialisation shape for a [`FixedLayout`]. See
/// [`parse_fixed_layout_toml`] for the schema.
#[derive(Debug, Deserialize)]
struct LayoutRepr {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "field")]
    fields: Vec<LayoutFieldRepr>,
}

#[derive(Debug, Deserialize)]
struct LayoutFieldRepr {
    name: String,
    #[serde(default)]
    len: Option<usize>,
    #[serde(default)]
    rest: bool,
}

// ---------------------------------------------------------------------------
// JSON shapes
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct BuildInput {
    mti: String,
    #[serde(default)]
    fields: BTreeMap<u8, FieldValue>,
}

/// Field value: plain string = ASCII bytes; `{ "hex": "..." }` = hex-decoded
/// bytes. The latter is needed for truly binary fields (PIN block, MAC).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum FieldValue {
    Ascii(String),
    Hex { hex: String },
}

#[derive(Debug, Serialize)]
struct MessageView {
    mti: String,
    bitmap_hex: String,
    has_secondary: bool,
    fields: Vec<FieldView>,
}

#[derive(Debug, Serialize)]
struct FieldView {
    number: u8,
    name: &'static str,
    type_desc: String,
    length: usize,
    value_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_ascii: Option<String>,
}

fn build_view(msg: &Iso8583Message) -> Result<MessageView, String> {
    let bitmap_hex = encode_bitmap_for_display(msg)?;
    let has_secondary = msg.fields.keys().any(|n| *n > 64);
    let mut fields = Vec::with_capacity(msg.fields.len());
    for (number, data) in &msg.fields {
        fields.push(field_view(*number, data));
    }
    Ok(MessageView {
        mti: String::from_utf8_lossy(&msg.mti).into_owned(),
        bitmap_hex,
        has_secondary,
        fields,
    })
}

fn field_view(number: u8, data: &[u8]) -> FieldView {
    let def = field_def(number);
    let name = def.map_or("Unknown", |d| d.name);
    let type_desc = def.map_or_else(|| "?".to_string(), describe_field);
    let value_ascii = if data.iter().all(byte_is_printable) {
        Some(String::from_utf8_lossy(data).into_owned())
    } else {
        None
    };
    FieldView {
        number,
        name,
        type_desc,
        length: data.len(),
        value_hex: hex_encode(data),
        value_ascii,
    }
}

// ---------------------------------------------------------------------------
// Tree rendering
// ---------------------------------------------------------------------------

fn render_tree(msg: &Iso8583Message) -> String {
    let mut out = String::new();
    out.push_str("ISO 8583 Message\n");
    out.push_str(&format!("├── MTI: {}\n", String::from_utf8_lossy(&msg.mti)));
    let bitmap_hex = encode_bitmap_for_display(msg).unwrap_or_else(|_| "<error>".to_string());
    out.push_str(&format!("├── Bitmap: {}\n", bitmap_hex));
    let fields_set: Vec<String> = msg.fields.keys().map(|n| n.to_string()).collect();
    if fields_set.is_empty() {
        out.push_str("│   └── (no fields)\n");
    } else {
        out.push_str(&format!("│   └── Fields set: {}\n", fields_set.join(", ")));
    }
    out.push_str("└── Fields:\n");
    let n_total = msg.fields.len();
    for (i, (number, data)) in msg.fields.iter().enumerate() {
        let last = i + 1 == n_total;
        let branch = if last { "└──" } else { "├──" };
        let cont = if last { "    " } else { "│   " };
        let def = field_def(*number);
        let name = def.map_or("Unknown", |d| d.name);
        let type_desc = def.map_or_else(|| "?".to_string(), describe_field);
        out.push_str(&format!(
            "    {} [{:>3}] {} — {} ({} bytes)\n",
            branch,
            number,
            name,
            type_desc,
            data.len()
        ));
        let value = if data.iter().all(byte_is_printable) {
            format!("{:?}", String::from_utf8_lossy(data))
        } else {
            format!("hex:{}", hex_encode(data))
        };
        out.push_str(&format!("    {}    {}\n", cont, value));
    }
    out
}

fn describe_field(def: &FieldDef) -> String {
    let dt = data_type_short(def.data_type);
    match def.length {
        LengthSpec::Fixed(n) => format!("{dt}{n} fixed"),
        LengthSpec::LLVAR { max } => format!("LLVAR {dt}..{max}"),
        LengthSpec::LLLVAR { max } => format!("LLLVAR {dt}..{max}"),
    }
}

fn data_type_short(t: DataType) -> &'static str {
    match t {
        DataType::Numeric => "n",
        DataType::Alpha => "a",
        DataType::Special => "s",
        DataType::AlphaNumeric => "an",
        DataType::AlphaSpecial => "as",
        DataType::NumericSpecial => "ns",
        DataType::AlphaNumericSpecial => "ans",
        DataType::Binary => "b",
        DataType::Track => "z",
    }
}

fn encode_bitmap_for_display(msg: &Iso8583Message) -> Result<String, String> {
    let mut bm = Bitmap8583::new();
    for n in msg.fields.keys() {
        bm.set(u16::from(*n))
            .map_err(|e| format!("bitmap set field {n}: {e:?}"))?;
    }
    Ok(hex_encode(&bm.encode()))
}

// ---------------------------------------------------------------------------
// Error display
// ---------------------------------------------------------------------------

fn display_parse(e: &ParseError) -> String {
    match e {
        ParseError::InsufficientBytes { offset, need } => format!(
            "insufficient bytes at offset {offset}; need {need} more — check the input is complete and unencoded"
        ),
        ParseError::InvalidMti(b) => format!(
            "MTI must be 4 ASCII digits, got {:?} (hex {})",
            String::from_utf8_lossy(b),
            hex_encode(b)
        ),
        ParseError::BitmapError(b) => format!("bitmap: {b:?}"),
        ParseError::UnknownField(n) => format!("bitmap set unknown field {n} (must be 1..=128)"),
        ParseError::InvalidLengthPrefix { field, bytes } => format!(
            "field {field} length prefix not ASCII digits: hex {}",
            hex_encode(bytes)
        ),
        ParseError::LengthExceedsMax { field, decoded, max } => {
            format!("field {field} length {decoded} > spec max {max}")
        }
        ParseError::TrailingBytes { remaining } => format!(
            "{remaining} unexpected byte(s) after last field — message has trailing data"
        ),
        ParseError::InvalidBitmapHex { offset, byte } => format!(
            "FullAscii bitmap: byte {byte:#x} at offset {offset} is not a hex digit"
        ),
        ParseError::InvalidBcdNibble { offset, byte } => format!(
            "FullBinary: byte {byte:#x} at offset {offset} has a nibble outside 0..=9"
        ),
    }
}

fn display_build(e: &BuildError) -> String {
    match e {
        BuildError::InvalidMti(b) => format!(
            "MTI must be 4 ASCII digits, got {:?}",
            String::from_utf8_lossy(b)
        ),
        BuildError::InvalidFieldNumber(n) => format!("field number {n} out of range (1..=128)"),
        BuildError::UnknownField(n) => format!("no spec for field {n}"),
        BuildError::FixedLengthMismatch {
            field,
            expected,
            actual,
        } => format!("field {field}: fixed length expected {expected} bytes, got {actual}"),
        BuildError::LengthExceedsMax { field, actual, max } => {
            format!("field {field}: payload {actual} > spec max {max}")
        }
        BuildError::LengthOverflow {
            field,
            actual,
            prefix_digits,
        } => format!("field {field}: length {actual} cannot fit in {prefix_digits} digit prefix"),
        BuildError::BitmapError(b) => format!("bitmap: {b:?}"),
        BuildError::InvalidBcdDigit { field, byte } => format!(
            "field {field}: Numeric payload byte {byte:#x} is not an ASCII digit (cannot BCD-pack)"
        ),
    }
}

// ---------------------------------------------------------------------------
// Hex + byte helpers
// ---------------------------------------------------------------------------

/// Decode a hex string into bytes, ignoring embedded whitespace. The public
/// form of the internal `hex_decode`, used by the binary to resolve a
/// `hex:<…>` argument for `wf oracle check`.
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, String> {
    hex_decode(&strip_whitespace(hex))
}

fn strip_whitespace(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err(format!(
            "hex string has odd length {} — every byte needs 2 hex digits",
            s.len()
        ));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = nibble(bytes[i])?;
        let lo = nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!(
            "non-hex byte {:#04x} ({:?}) — strip non-hex chars first",
            b, b as char
        )),
    }
}

fn hex_encode(data: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(data.len() * 2);
    for byte in data {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn byte_is_printable(b: &u8) -> bool {
    b.is_ascii_graphic() || *b == b' '
}

// ---------------------------------------------------------------------------
// SWIFT MT rendering
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct SwiftMessageView {
    blocks: Vec<SwiftBlockView>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum SwiftBlockView {
    Raw {
        id: u8,
        value: String,
    },
    Text {
        id: u8,
        fields: Vec<SwiftFieldView>,
    },
    Tagged {
        id: u8,
        entries: Vec<SwiftFieldView>,
    },
}

#[derive(Debug, Serialize)]
struct SwiftFieldView {
    tag: String,
    value: String,
}

fn build_swift_view(msg: &SwiftMessage) -> SwiftMessageView {
    let blocks = msg
        .blocks
        .iter()
        .map(|(id, block)| match block {
            SwiftBlock::Raw(s) => SwiftBlockView::Raw {
                id: *id,
                value: s.clone(),
            },
            SwiftBlock::Text(fs) => SwiftBlockView::Text {
                id: *id,
                fields: fs
                    .iter()
                    .map(|f| SwiftFieldView {
                        tag: f.tag.clone(),
                        value: f.value.clone(),
                    })
                    .collect(),
            },
            SwiftBlock::Tagged(subs) => SwiftBlockView::Tagged {
                id: *id,
                entries: subs
                    .iter()
                    .map(|s| SwiftFieldView {
                        tag: s.tag.clone(),
                        value: s.value.clone(),
                    })
                    .collect(),
            },
        })
        .collect();
    SwiftMessageView { blocks }
}

fn render_swift_tree(msg: &SwiftMessage) -> String {
    let mut out = String::new();
    out.push_str("SWIFT MT Message\n");
    let n_blocks = msg.blocks.len();
    for (i, (id, block)) in msg.blocks.iter().enumerate() {
        let last_block = i + 1 == n_blocks;
        let branch = if last_block { "└──" } else { "├──" };
        let cont = if last_block { "    " } else { "│   " };
        match block {
            SwiftBlock::Raw(s) => {
                out.push_str(&format!("{branch} Block {id} (raw): {s:?}\n"));
            }
            SwiftBlock::Text(fields) => {
                out.push_str(&format!(
                    "{branch} Block {id} (text, {} fields)\n",
                    fields.len()
                ));
                let n_fields = fields.len();
                for (j, f) in fields.iter().enumerate() {
                    let last = j + 1 == n_fields;
                    let fbranch = if last { "└──" } else { "├──" };
                    let preview = preview_value(&f.value);
                    out.push_str(&format!("{cont}{fbranch} :{}:  {}\n", f.tag, preview));
                }
            }
            SwiftBlock::Tagged(subs) => {
                out.push_str(&format!(
                    "{branch} Block {id} (tagged, {} entries)\n",
                    subs.len()
                ));
                let n_subs = subs.len();
                for (j, s) in subs.iter().enumerate() {
                    let last = j + 1 == n_subs;
                    let fbranch = if last { "└──" } else { "├──" };
                    out.push_str(&format!("{cont}{fbranch} {{{}: {}}}\n", s.tag, s.value));
                }
            }
        }
    }
    out
}

/// Truncate a multi-line field value to a one-line preview for tree
/// rendering. Embedded newlines become `\\n` so the tree stays aligned;
/// values over 60 chars are tail-clipped to keep terminal lines bounded.
fn preview_value(value: &str) -> String {
    let escaped: String = value
        .chars()
        .flat_map(|c| match c {
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect();
    if escaped.chars().count() > 60 {
        let head: String = escaped.chars().take(57).collect();
        format!("{head}...")
    } else {
        escaped
    }
}
