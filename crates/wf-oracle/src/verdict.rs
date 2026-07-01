//! The verdict model and the rendered EVIDENCE report.
//!
//! Every per-field outcome is a [`FieldVerdict`]; the whole comparison is an
//! [`OracleReport`] carrying the rows, a [`Coverage`] meter, and a
//! [`ConformanceGate`] (0 / 1 / 2). [`OracleReport::render`] turns it into the
//! human-facing artifact — which states it is **EVIDENCE**, never proof,
//! certification, or equivalence.

use std::fmt::Write as _;

use crate::mask::MaskType;
use crate::wire::FieldKey;

/// The outcome of comparing one field across the legacy and migrated sides
/// under its resolved mask.
///
/// Only [`FieldVerdict::Unexplained`] is drift; the other four are
/// conformant. Crucially `Volatile`/`Crypto` results are *not* claimed as
/// verified — they are excluded from the coverage denominator — so a high
/// coverage number cannot be manufactured by masking everything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldVerdict {
    /// Stable field, byte-identical on both sides (or both-absent).
    Equal,
    /// Volatile field — difference normalised away (not drift, not verified).
    VolatileNormalized,
    /// Crypto field — excluded from value comparison (not drift, not
    /// verified).
    CryptoExcluded,
    /// Intended-delta field whose migrated value matched the spec `expect`.
    IntendedDelta,
    /// Drift: the field did not conform to its mask rule.
    Unexplained {
        /// Why the field is unexplained.
        reason: UnexplainedReason,
    },
}

impl FieldVerdict {
    /// Whether this verdict is drift ([`FieldVerdict::Unexplained`]).
    pub fn is_drift(&self) -> bool {
        matches!(self, FieldVerdict::Unexplained { .. })
    }

    /// Whether this verdict positively accounts for a value-bearing field
    /// (`Equal` or `IntendedDelta`) — i.e. counts toward `checked` coverage.
    pub fn is_accounted(&self) -> bool {
        matches!(self, FieldVerdict::Equal | FieldVerdict::IntendedDelta)
    }
}

/// Why a field was classified [`FieldVerdict::Unexplained`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnexplainedReason {
    /// Both present, same length, different bytes.
    ValueDiff,
    /// Both present but different lengths. A dedicated variant (rather than
    /// folding into `ValueDiff`) because the engine **never** truncates to a
    /// common length before comparing — a length change is its own, clearly
    /// reported drift.
    LengthDiff {
        /// Legacy-side byte length.
        legacy: usize,
        /// Migrated-side byte length.
        migrated: usize,
    },
    /// Intended-delta field whose migrated value did not equal `expect`.
    IntendedDeltaUnmet {
        /// The operator-approved expected bytes.
        expected: Vec<u8>,
        /// The migrated bytes, or `None` if the field was absent on the
        /// migrated side.
        got: Option<Vec<u8>>,
    },
    /// A stable field present on exactly one side.
    PresenceMismatch {
        /// Whether the field was present on the legacy side.
        legacy_present: bool,
        /// Whether the field was present on the migrated side.
        migrated_present: bool,
    },
    /// Both present but a different number of occurrences.
    OccurrenceCountDiff {
        /// Legacy-side occurrence count.
        legacy: usize,
        /// Migrated-side occurrence count.
        migrated: usize,
    },
}

/// One row of the report: a field, its resolved mask, and its verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleRow {
    /// The field key.
    pub key: FieldKey,
    /// Human-readable field label (presentation only).
    pub label: String,
    /// The mask that was applied.
    pub mask: MaskType,
    /// The verdict.
    pub verdict: FieldVerdict,
}

/// The coverage meter: how many value-bearing baseline fields were positively
/// accounted for, out of the total value-bearing baseline.
///
/// "Value-bearing baseline" = fields whose resolved mask is `Stable` or
/// `IntendedDelta` **and** which are present on the legacy side. Volatile and
/// Crypto fields are deliberately excluded from `total` — the engine does not
/// claim to verify their value, so counting them would inflate the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Coverage {
    /// Value-bearing baseline fields with a conformant value
    /// (`Equal`/`IntendedDelta`).
    pub checked: usize,
    /// Total value-bearing baseline fields.
    pub total: usize,
}

impl Coverage {
    /// Integer percentage `checked/total` (deterministic; no floating point).
    /// `total == 0` yields `0`, never a misleading 100 % — see
    /// [`OracleReport::render`] for the accompanying note.
    pub fn pct(&self) -> u8 {
        // checked <= total <= 129, so checked*100 fits easily and the quotient
        // is in 0..=100. `checked_div` yields None when total == 0 → 0 %,
        // never a misleading 100 % (see `OracleReport::render` for the note).
        (self.checked * 100).checked_div(self.total).unwrap_or(0) as u8
    }
}

/// The diff-style conformance gate, mapped to a process exit code.
///
/// The 0 / 1 / 2 split mirrors `diff(1)`: 0 = conformant, 1 = ran cleanly but
/// found drift, 2 = the comparison could not be performed (parse / spec
/// error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConformanceGate {
    /// No drift: every value-bearing baseline field conformed to its mask.
    Conformant,
    /// Ran cleanly but at least one field is `Unexplained`.
    FoundDrift,
    /// The comparison could not be performed (uncheckable input).
    HadErrors,
}

impl ConformanceGate {
    /// The process exit code this gate maps to: 0 / 1 / 2.
    pub fn code(self) -> u8 {
        match self {
            ConformanceGate::Conformant => 0,
            ConformanceGate::FoundDrift => 1,
            ConformanceGate::HadErrors => 2,
        }
    }
}

/// The full masked-diff comparison: interface label, sorted rows, coverage,
/// and gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleReport {
    /// The interface family compared (from the spec).
    pub interface: String,
    /// One row per key in the universe, sorted by [`FieldKey`].
    pub rows: Vec<OracleRow>,
    /// The coverage meter.
    pub coverage: Coverage,
    /// The conformance gate. A report produced by the engine is only ever
    /// `Conformant` or `FoundDrift`; `HadErrors` is reached at the caller when
    /// the inputs could not be parsed (the engine returns `Err` then).
    pub gate: ConformanceGate,
}

impl OracleReport {
    /// Render the report as the human-facing EVIDENCE artifact.
    ///
    /// The output frames itself as regression-conformance **EVIDENCE** and the
    /// only place the words proof / certification / equivalence appear is the
    /// negative disclaimer. The text is a pure function of the report, so two
    /// identical reports render byte-identically (determinism).
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("Wireforge Conformance EVIDENCE\n");
        let _ = writeln!(out, "  interface: {}", self.interface);
        out.push_str("  mode: Mode-A replay (captured legacy response vs migrated response)\n");
        out.push_str("  fixtures: SYNTHETIC\n");
        out.push_str("  note: regression-conformance EVIDENCE only — NOT a proof, NOT a\n");
        out.push_str("        certification, NOT an equivalence claim. A conformant gate means\n");
        out.push_str("        every value-bearing baseline field matched its operator-approved\n");
        out.push_str("        mask rule under this capture — nothing more.\n");
        let coverage_note = if self.coverage.total == 0 {
            " — no value-bearing baseline fields in this capture"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "  coverage: {}% ({}/{} value-bearing baseline fields accounted for){}",
            self.coverage.pct(),
            self.coverage.checked,
            self.coverage.total,
            coverage_note,
        );
        let _ = writeln!(
            out,
            "  gate: {} (exit {})",
            gate_label(self.gate),
            self.gate.code()
        );
        out.push_str("Fields:\n");
        let n = self.rows.len();
        for (i, r) in self.rows.iter().enumerate() {
            let last = i + 1 == n;
            let branch = if last { "└──" } else { "├──" };
            let _ = writeln!(
                out,
                "  {branch} [{:>3}] {} [{}]: {}{}",
                r.key.number(),
                r.label,
                mask_label(r.mask),
                verdict_label(&r.verdict),
                verdict_detail(&r.verdict),
            );
        }
        out
    }
}

/// Stable lowercase label for a [`ConformanceGate`].
fn gate_label(gate: ConformanceGate) -> &'static str {
    match gate {
        ConformanceGate::Conformant => "conformant",
        ConformanceGate::FoundDrift => "drift found",
        ConformanceGate::HadErrors => "errors",
    }
}

/// Stable lowercase label for a [`MaskType`].
fn mask_label(mask: MaskType) -> &'static str {
    match mask {
        MaskType::Stable => "stable",
        MaskType::Volatile => "volatile",
        MaskType::Crypto => "crypto",
        MaskType::IntendedDelta => "intended-delta",
    }
}

/// Stable label for a [`FieldVerdict`]. Drift renders as the uppercase
/// `UNEXPLAINED` so it stands out in a terminal scan.
fn verdict_label(verdict: &FieldVerdict) -> &'static str {
    match verdict {
        FieldVerdict::Equal => "equal",
        FieldVerdict::VolatileNormalized => "volatile-normalized",
        FieldVerdict::CryptoExcluded => "crypto-excluded",
        FieldVerdict::IntendedDelta => "intended-delta",
        FieldVerdict::Unexplained { .. } => "UNEXPLAINED",
    }
}

/// The verdict-specific detail suffix (empty for the conformant verdicts).
fn verdict_detail(verdict: &FieldVerdict) -> String {
    match verdict {
        FieldVerdict::Unexplained { reason } => format!(" ({})", reason_detail(reason)),
        _ => String::new(),
    }
}

/// Human-readable detail for an [`UnexplainedReason`].
fn reason_detail(reason: &UnexplainedReason) -> String {
    match reason {
        UnexplainedReason::ValueDiff => "value differs".to_string(),
        UnexplainedReason::LengthDiff { legacy, migrated } => {
            format!("length differs: legacy {legacy} vs migrated {migrated} bytes")
        }
        UnexplainedReason::IntendedDeltaUnmet { expected, got } => {
            let got = match got {
                Some(bytes) => show_bytes(bytes),
                None => "<absent>".to_string(),
            };
            format!(
                "intended delta unmet: expected {}, got {got}",
                show_bytes(expected)
            )
        }
        UnexplainedReason::PresenceMismatch {
            legacy_present,
            migrated_present,
        } => format!(
            "presence mismatch: legacy {}, migrated {}",
            present_word(*legacy_present),
            present_word(*migrated_present)
        ),
        UnexplainedReason::OccurrenceCountDiff { legacy, migrated } => {
            format!("occurrence count differs: legacy {legacy} vs migrated {migrated}")
        }
    }
}

/// `"present"` / `"absent"`.
fn present_word(present: bool) -> &'static str {
    if present {
        "present"
    } else {
        "absent"
    }
}

/// Render bytes as a quoted ASCII string when fully printable, else as
/// `hex:<…>`. Mirrors the wf-cli tree renderer so the two surfaces show field
/// values the same way.
fn show_bytes(bytes: &[u8]) -> String {
    if bytes.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        format!("{:?}", String::from_utf8_lossy(bytes))
    } else {
        let mut s = String::with_capacity(4 + bytes.len() * 2);
        s.push_str("hex:");
        for b in bytes {
            let _ = write!(s, "{b:02x}");
        }
        s
    }
}
