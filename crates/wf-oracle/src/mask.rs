//! The masking layer: how an operator declares, per field, what counts as
//! conformant drift versus an expected change.
//!
//! This is the engine's net-new IP. A raw byte diff of two financial messages
//! is useless — timestamps, trace numbers, and MACs differ on every wire even
//! between two correct systems. The mask is the operator-approved statement of
//! *intent* that turns a noisy diff into evidence:
//!
//! - [`MaskType::Stable`] — must be byte-identical. Any difference is drift.
//! - [`MaskType::Volatile`] — expected to differ run-to-run (timestamps,
//!   trace numbers); differences are normalised away, never counted as drift
//!   and never counted as *verified* either.
//! - [`MaskType::Crypto`] — a value the migrated system legitimately
//!   re-derives (MAC, PIN block); excluded from the value comparison. (Crypto
//!   re-derivation is deferred future work — for now the field is simply not
//!   claimed.)
//! - [`MaskType::IntendedDelta`] — a value the migration *intends* to change;
//!   conformant only when the migrated side equals the operator-supplied
//!   [`FieldMask::expect`].
//!
//! The default mask is **[`MaskType::Stable`]** so a field the operator never
//! considered **fails closed**: its drift surfaces as `Unexplained` rather
//! than silently passing.

use crate::wire::FieldKey;

/// How a field is treated by the masked diff. See the module docs for the
/// meaning of each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskType {
    /// Must match byte-for-byte; any difference is unexplained drift.
    Stable,
    /// Expected to vary; normalised away (neither drift nor verified).
    Volatile,
    /// Re-derived by the migrated system; excluded from value comparison.
    Crypto,
    /// Intentionally changed; conformant iff the migrated value equals
    /// [`FieldMask::expect`].
    IntendedDelta,
}

/// One field's operator-approved treatment.
///
/// `expect` is required **iff** `mask` is [`MaskType::IntendedDelta`]: an
/// intended-delta mask without the expected bytes is meaningless, and an
/// `expect` on any other mask is a category error. [`OracleSpec::validate`]
/// enforces both directions, so a malformed spec is rejected (gate 2) rather
/// than silently mis-applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMask {
    /// The field this mask governs.
    pub key: FieldKey,
    /// How the field is treated.
    pub mask: MaskType,
    /// The expected migrated bytes — present iff `mask == IntendedDelta`.
    pub expect: Option<Vec<u8>>,
}

impl FieldMask {
    /// A [`MaskType::Stable`] mask for `key`.
    pub fn stable(key: FieldKey) -> Self {
        FieldMask {
            key,
            mask: MaskType::Stable,
            expect: None,
        }
    }

    /// A [`MaskType::Volatile`] mask for `key`.
    pub fn volatile(key: FieldKey) -> Self {
        FieldMask {
            key,
            mask: MaskType::Volatile,
            expect: None,
        }
    }

    /// A [`MaskType::Crypto`] mask for `key`.
    pub fn crypto(key: FieldKey) -> Self {
        FieldMask {
            key,
            mask: MaskType::Crypto,
            expect: None,
        }
    }

    /// A [`MaskType::IntendedDelta`] mask for `key` expecting `expect` on the
    /// migrated side.
    pub fn intended_delta(key: FieldKey, expect: impl Into<Vec<u8>>) -> Self {
        FieldMask {
            key,
            mask: MaskType::IntendedDelta,
            expect: Some(expect.into()),
        }
    }
}

/// An operator-approved conformance specification: the interface label, the
/// per-field masks, and the default mask for everything not listed.
///
/// Build one with [`OracleSpec::new`] (default mask = [`MaskType::Stable`])
/// then chain [`OracleSpec::with_mask`] / [`OracleSpec::with_default_mask`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleSpec {
    /// The interface family this spec describes (e.g. `"iso8583"`). Carried
    /// into the report header.
    pub interface: String,
    /// Per-field masks. The first mask matching a key wins; a key with no
    /// mask resolves to [`OracleSpec::default_mask`].
    pub masks: Vec<FieldMask>,
    /// The mask applied to any key not named in `masks`. Defaults to
    /// [`MaskType::Stable`] so unconsidered fields fail closed.
    pub default_mask: MaskType,
}

impl OracleSpec {
    /// A spec for `interface` with no masks and a [`MaskType::Stable`]
    /// default (fail-closed).
    pub fn new(interface: impl Into<String>) -> Self {
        OracleSpec {
            interface: interface.into(),
            masks: Vec::new(),
            default_mask: MaskType::Stable,
        }
    }

    /// Add `mask` and return `self` (builder style).
    pub fn with_mask(mut self, mask: FieldMask) -> Self {
        self.masks.push(mask);
        self
    }

    /// Set the default mask and return `self` (builder style).
    pub fn with_default_mask(mut self, default_mask: MaskType) -> Self {
        self.default_mask = default_mask;
        self
    }

    /// Resolve a key to its `(mask, expect)`: the first matching [`FieldMask`]
    /// if one exists, else [`OracleSpec::default_mask`] with no `expect`.
    pub(crate) fn resolve(&self, key: FieldKey) -> (MaskType, Option<&[u8]>) {
        match self.masks.iter().find(|m| m.key == key) {
            Some(m) => (m.mask, m.expect.as_deref()),
            None => (self.default_mask, None),
        }
    }

    /// Reject a malformed spec: an [`MaskType::IntendedDelta`] mask **must**
    /// carry `expect`, and every other mask **must not**. Returns a
    /// human-readable reason on the first offending mask so the caller can map
    /// it to gate 2 (uncheckable input).
    pub fn validate(&self) -> Result<(), String> {
        for m in &self.masks {
            let field = m.key.number();
            match (m.mask, m.expect.is_some()) {
                (MaskType::IntendedDelta, false) => {
                    return Err(format!(
                        "field {field} is masked IntendedDelta but carries no `expect` value; \
                         an intended-delta mask requires the operator-approved expected bytes"
                    ));
                }
                (mask, true) if !matches!(mask, MaskType::IntendedDelta) => {
                    return Err(format!(
                        "field {field} carries an `expect` value but is not masked IntendedDelta; \
                         `expect` is only meaningful for an intended-delta mask"
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }
}
