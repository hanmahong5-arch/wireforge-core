//! Wireforge deterministic wire-conformance **EVIDENCE** engine.
//!
//! # What this crate is — and is NOT
//!
//! Given a captured **legacy** response and a **migrated** system's response
//! to the same request (Mode-A replay), this engine compares them field by
//! field, under an operator-approved [`OracleSpec`] mask, and emits a
//! coverage-metered [`OracleReport`] plus a 0 / 1 / 2 [`ConformanceGate`].
//!
//! It produces **regression-conformance EVIDENCE** — it does **not** prove,
//! certify, or assert equivalence. A `Conformant` gate means every
//! value-bearing baseline field matched its mask rule *under this capture* —
//! nothing about the systems in general. The words proof / certification /
//! equivalence appear in the code and rendered output only inside the negative
//! disclaimer.
//!
//! # Format-agnostic core
//!
//! The masked-diff core ([`check_conformance_views`]) works on any
//! [`WireMessage`]. Two implementations exist: [`Iso8583View`] (with
//! [`check_conformance`] as its byte-parsing front door) and
//! [`fixed::FixedView`] (fixed-length record frames tiled by a
//! [`fixed::FixedLayout`]). MX paths plug in behind the same trait later
//! without touching the core.
//!
//! # Scope (honest)
//!
//! ISO 8583 + fixed-length record views only; **SYNTHETIC** fixtures (no real
//! captures exist yet); Mode-A replay only. Crypto re-derivation, signing the
//! evidence, request-side diffing, and Mode-B are deferred. An MCP
//! `wf_oracle_check` tool is intentionally **deferred** to keep the server's
//! 12-tool surface stable — the engine is CLI-first.

pub mod fixed;
pub mod iso8583;
pub mod mask;
pub mod verdict;
pub mod wire;

pub use fixed::{FixedField, FixedLayout, FixedLen, FixedView};
pub use iso8583::Iso8583View;
pub use mask::{FieldMask, MaskType, OracleSpec};
pub use verdict::{
    ConformanceGate, Coverage, FieldVerdict, OracleReport, OracleRow, UnexplainedReason,
};
pub use wire::{FieldKey, WireMessage};

use std::collections::BTreeSet;

/// Compare a captured legacy ISO 8583 response against a migrated response
/// under `spec`, returning coverage-metered EVIDENCE.
///
/// `req` is the request that produced both responses; it is parsed **for
/// validity only** (a malformed request makes the capture uncheckable) and is
/// not diffed in this PoC. `legacy_resp` and `migrated_resp` are the two
/// responses to compare.
///
/// Returns `Err` — which the caller maps to gate **2** (uncheckable) — when
/// the spec is malformed ([`OracleSpec::validate`]) or any of the three
/// messages fails to parse. Otherwise returns an [`OracleReport`] whose gate
/// is `Conformant` (no drift) or `FoundDrift`.
pub fn check_conformance(
    req: &[u8],
    legacy_resp: &[u8],
    migrated_resp: &[u8],
    spec: &OracleSpec,
) -> Result<OracleReport, String> {
    spec.validate()?;
    // The request is parsed to confirm the capture is well-formed, but the
    // PoC diffs only the two responses (request-side diffing is deferred).
    let _req = Iso8583View::parse(req).map_err(|e| format!("request parse: {e}"))?;
    let legacy =
        Iso8583View::parse(legacy_resp).map_err(|e| format!("legacy response parse: {e}"))?;
    let migrated =
        Iso8583View::parse(migrated_resp).map_err(|e| format!("migrated response parse: {e}"))?;
    Ok(diff_masked(&spec.interface, &legacy, &migrated, spec))
}

/// The format-agnostic engine: run the masked diff over two already-parsed
/// [`WireMessage`]s.
///
/// [`check_conformance`] is the ISO 8583 byte-parsing front door to this; call
/// this directly when you already hold parsed views (e.g. an alternate format
/// implementation, or a test stub exercising the multi-occurrence path).
/// Still validates `spec` (→ `Err`, gate 2) before diffing.
pub fn check_conformance_views(
    legacy: &dyn WireMessage,
    migrated: &dyn WireMessage,
    spec: &OracleSpec,
) -> Result<OracleReport, String> {
    spec.validate()?;
    Ok(diff_masked(&spec.interface, legacy, migrated, spec))
}

/// The masked-diff core. Pure: the verdict for every key is a function of
/// (resolved mask, the two sides' bytes, spec `expect`) — never read from the
/// fixture — which is what keeps the evidence non-tautological.
fn diff_masked(
    interface: &str,
    legacy: &dyn WireMessage,
    migrated: &dyn WireMessage,
    spec: &OracleSpec,
) -> OracleReport {
    // 1. Deterministic key universe: every key either side carries, plus
    //    every key the spec names. BTreeSet keeps the rows sorted.
    let mut keys: BTreeSet<FieldKey> = BTreeSet::new();
    keys.extend(legacy.field_keys());
    keys.extend(migrated.field_keys());
    keys.extend(spec.masks.iter().map(|m| m.key));

    let mut rows = Vec::with_capacity(keys.len());
    let mut checked = 0usize;
    let mut total = 0usize;
    for key in keys {
        let (mask, expect) = spec.resolve(key);
        let legacy_occ = legacy.field_occurrences(key);
        let migrated_occ = migrated.field_occurrences(key);
        let verdict = classify(mask, expect, legacy_occ, migrated_occ);

        // Coverage denominator: value-bearing baseline = Stable/IntendedDelta
        // AND present on the legacy side. Volatile/Crypto never count — the
        // engine does not claim to verify their value.
        let value_bearing =
            matches!(mask, MaskType::Stable | MaskType::IntendedDelta) && !legacy_occ.is_empty();
        if value_bearing {
            total += 1;
            if verdict.is_accounted() {
                checked += 1;
            }
        }

        rows.push(OracleRow {
            key,
            label: legacy.field_label(key),
            mask,
            verdict,
        });
    }

    let gate = if rows.iter().any(|r| r.verdict.is_drift()) {
        ConformanceGate::FoundDrift
    } else {
        ConformanceGate::Conformant
    };

    OracleReport {
        interface: interface.to_string(),
        rows,
        coverage: Coverage { checked, total },
        gate,
    }
}

/// Classify one field across both sides under `mask`. See the module-level
/// algorithm: presence matrix first, then occurrence count, then the per-occ
/// mask→verdict table.
fn classify(
    mask: MaskType,
    expect: Option<&[u8]>,
    legacy: &[Vec<u8>],
    migrated: &[Vec<u8>],
) -> FieldVerdict {
    let legacy_present = !legacy.is_empty();
    let migrated_present = !migrated.is_empty();
    match (legacy_present, migrated_present) {
        // Both absent: nothing on either side. Non-counting (legacy absent →
        // not value-bearing) pass for Stable/Volatile/Crypto; for
        // IntendedDelta we still expected `expect` on the migrated side.
        (false, false) => match mask {
            MaskType::Stable => FieldVerdict::Equal,
            MaskType::Volatile => FieldVerdict::VolatileNormalized,
            MaskType::Crypto => FieldVerdict::CryptoExcluded,
            MaskType::IntendedDelta => intended_delta_verdict(expect, None),
        },
        // Present on exactly one side.
        (true, false) | (false, true) => match mask {
            MaskType::Volatile => FieldVerdict::VolatileNormalized,
            MaskType::Crypto => FieldVerdict::CryptoExcluded,
            MaskType::IntendedDelta => {
                intended_delta_verdict(expect, migrated.first().map(Vec::as_slice))
            }
            MaskType::Stable => FieldVerdict::Unexplained {
                reason: UnexplainedReason::PresenceMismatch {
                    legacy_present,
                    migrated_present,
                },
            },
        },
        // Present on both sides.
        (true, true) => {
            if legacy.len() != migrated.len() {
                count_mismatch(mask, legacy.len(), migrated.len())
            } else if legacy.len() == 1 {
                single_occ(mask, expect, &legacy[0], &migrated[0])
            } else {
                worst_wins(mask, expect, legacy, migrated)
            }
        }
    }
}

/// Verdict for a single both-present occurrence under `mask`.
///
/// The **min_len rule** lives here: a Stable field is compared over its
/// **full** slices — never sliced to a common length — and a length change is
/// reported as its own [`UnexplainedReason::LengthDiff`], not folded into
/// `ValueDiff`. This kills the truncation blind spot where two values that
/// agree on a shared prefix but differ in length would falsely read `Equal`.
fn single_occ(
    mask: MaskType,
    expect: Option<&[u8]>,
    legacy: &[u8],
    migrated: &[u8],
) -> FieldVerdict {
    match mask {
        MaskType::Stable => {
            if legacy == migrated {
                FieldVerdict::Equal
            } else if legacy.len() != migrated.len() {
                FieldVerdict::Unexplained {
                    reason: UnexplainedReason::LengthDiff {
                        legacy: legacy.len(),
                        migrated: migrated.len(),
                    },
                }
            } else {
                FieldVerdict::Unexplained {
                    reason: UnexplainedReason::ValueDiff,
                }
            }
        }
        MaskType::Volatile => FieldVerdict::VolatileNormalized,
        MaskType::Crypto => FieldVerdict::CryptoExcluded,
        MaskType::IntendedDelta => intended_delta_verdict(expect, Some(migrated)),
    }
}

/// Verdict for an intended-delta field: conformant iff the migrated value
/// equals `expect`. `migrated_val` is `None` when the field is absent on the
/// migrated side. The comparison is against the spec `expect`, never against
/// the legacy bytes — an intended delta is an *approved change*, so legacy is
/// irrelevant to the verdict.
fn intended_delta_verdict(expect: Option<&[u8]>, migrated_val: Option<&[u8]>) -> FieldVerdict {
    match expect {
        Some(exp) if migrated_val == Some(exp) => FieldVerdict::IntendedDelta,
        // Wrong value, absent, or (defensively — spec validation forbids it)
        // a missing `expect`: unmet.
        _ => FieldVerdict::Unexplained {
            reason: UnexplainedReason::IntendedDeltaUnmet {
                expected: expect.unwrap_or(&[]).to_vec(),
                got: migrated_val.map(<[u8]>::to_vec),
            },
        },
    }
}

/// Verdict when both sides are present but with different occurrence counts.
fn count_mismatch(mask: MaskType, legacy: usize, migrated: usize) -> FieldVerdict {
    match mask {
        MaskType::Stable | MaskType::IntendedDelta => FieldVerdict::Unexplained {
            reason: UnexplainedReason::OccurrenceCountDiff { legacy, migrated },
        },
        MaskType::Volatile => FieldVerdict::VolatileNormalized,
        MaskType::Crypto => FieldVerdict::CryptoExcluded,
    }
}

/// Verdict for equal-but-greater-than-one occurrence counts: compare each
/// occurrence pair and let the **worst** verdict win (any `Unexplained`
/// dominates a conformant one).
fn worst_wins(
    mask: MaskType,
    expect: Option<&[u8]>,
    legacy: &[Vec<u8>],
    migrated: &[Vec<u8>],
) -> FieldVerdict {
    let mut chosen: Option<FieldVerdict> = None;
    for (l, m) in legacy.iter().zip(migrated.iter()) {
        let v = single_occ(mask, expect, l, m);
        match &chosen {
            None => chosen = Some(v),
            Some(prev) if v.is_drift() && !prev.is_drift() => chosen = Some(v),
            Some(_) => {}
        }
    }
    // Counts are > 1 here, so the loop always set `chosen`; the fallback keeps
    // the function total without an `unwrap`.
    chosen.unwrap_or(FieldVerdict::Equal)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn occ(values: &[&[u8]]) -> Vec<Vec<u8>> {
        values.iter().map(|v| v.to_vec()).collect()
    }

    // ---- Anti-tautology guard: pin the mask→verdict table -----------------
    //
    // Mirrors wf-xform's `cited_caps_are_what_the_tests_assume`. Each
    // expectation is computed by hand from (mask rule, bytes, expect), NOT
    // read from any fixture, so a future change to the classifier breaks here
    // loudly. This is the single source of truth the synthetic ISO fixtures
    // assume.
    #[test]
    fn mask_to_verdict_table_is_pinned() {
        // Stable: full-slice compare, value AND length.
        assert_eq!(
            single_occ(MaskType::Stable, None, b"A", b"A"),
            FieldVerdict::Equal
        );
        assert_eq!(
            single_occ(MaskType::Stable, None, b"AB", b"AC"),
            FieldVerdict::Unexplained {
                reason: UnexplainedReason::ValueDiff
            }
        );
        // The min_len guard: a shared prefix but different length is a
        // LengthDiff, never Equal.
        assert_eq!(
            single_occ(MaskType::Stable, None, b"AB", b"ABC"),
            FieldVerdict::Unexplained {
                reason: UnexplainedReason::LengthDiff {
                    legacy: 2,
                    migrated: 3
                }
            }
        );
        // Volatile / Crypto: difference is normalised / excluded, never drift.
        assert_eq!(
            single_occ(MaskType::Volatile, None, b"X", b"Y"),
            FieldVerdict::VolatileNormalized
        );
        assert_eq!(
            single_occ(MaskType::Crypto, None, b"X", b"Y"),
            FieldVerdict::CryptoExcluded
        );
        // IntendedDelta: checked against `expect`, not legacy.
        assert_eq!(
            single_occ(MaskType::IntendedDelta, Some(b"01"), b"00", b"01"),
            FieldVerdict::IntendedDelta
        );
        assert_eq!(
            single_occ(MaskType::IntendedDelta, Some(b"01"), b"00", b"02"),
            FieldVerdict::Unexplained {
                reason: UnexplainedReason::IntendedDeltaUnmet {
                    expected: b"01".to_vec(),
                    got: Some(b"02".to_vec())
                }
            }
        );
    }

    #[test]
    fn presence_matrix_is_pinned() {
        // Both absent: Stable is a (non-counting) pass.
        assert_eq!(
            classify(MaskType::Stable, None, &[], &[]),
            FieldVerdict::Equal
        );
        // Both absent + IntendedDelta: we expected `expect` on migrated → unmet.
        assert_eq!(
            classify(MaskType::IntendedDelta, Some(b"01"), &[], &[]),
            FieldVerdict::Unexplained {
                reason: UnexplainedReason::IntendedDeltaUnmet {
                    expected: b"01".to_vec(),
                    got: None
                }
            }
        );
        // Stable present on one side only → presence mismatch (drift).
        assert_eq!(
            classify(MaskType::Stable, None, &occ(&[b"A"]), &[]),
            FieldVerdict::Unexplained {
                reason: UnexplainedReason::PresenceMismatch {
                    legacy_present: true,
                    migrated_present: false
                }
            }
        );
    }

    #[test]
    fn occurrence_count_mismatch_is_drift_for_stable() {
        let legacy = occ(&[b"A", b"B"]);
        let migrated = occ(&[b"A"]);
        assert_eq!(
            classify(MaskType::Stable, None, &legacy, &migrated),
            FieldVerdict::Unexplained {
                reason: UnexplainedReason::OccurrenceCountDiff {
                    legacy: 2,
                    migrated: 1
                }
            }
        );
    }

    #[test]
    fn coverage_pct_is_integer_and_zero_for_empty() {
        assert_eq!(
            Coverage {
                checked: 0,
                total: 0
            }
            .pct(),
            0
        );
        assert_eq!(
            Coverage {
                checked: 1,
                total: 1
            }
            .pct(),
            100
        );
        assert_eq!(
            Coverage {
                checked: 1,
                total: 3
            }
            .pct(),
            33
        );
        assert_eq!(
            Coverage {
                checked: 2,
                total: 3
            }
            .pct(),
            66
        );
    }

    #[test]
    fn gate_codes_are_zero_one_two() {
        assert_eq!(ConformanceGate::Conformant.code(), 0);
        assert_eq!(ConformanceGate::FoundDrift.code(), 1);
        assert_eq!(ConformanceGate::HadErrors.code(), 2);
    }

    #[test]
    fn spec_validate_rejects_intended_delta_without_expect() {
        let spec = OracleSpec::new("iso8583").with_mask(FieldMask {
            key: FieldKey::Iso8583(39),
            mask: MaskType::IntendedDelta,
            expect: None,
        });
        assert!(spec.validate().is_err());
    }

    #[test]
    fn spec_validate_rejects_expect_on_non_intended_delta() {
        let spec = OracleSpec::new("iso8583").with_mask(FieldMask {
            key: FieldKey::Iso8583(39),
            mask: MaskType::Stable,
            expect: Some(b"00".to_vec()),
        });
        assert!(spec.validate().is_err());
    }
}
