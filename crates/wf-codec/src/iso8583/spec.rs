//! Runtime-loadable ISO 8583 field specifications.
//!
//! The built-in field table ([`super::field`]) is a compile-time constant
//! covering ISO 8583-1987. Real deployments routinely diverge: a national
//! scheme redefines field 48, a private network repurposes a reserved slot,
//! an acquirer pins a shorter PAN. [`FieldSpec`] lets a caller supply those
//! definitions **at runtime** without recompiling, while the default code
//! path stays bit-for-bit identical to the built-in table.
//!
//! # Why this lives beside [`field`](super::field) rather than replacing it
//!
//! The parser and builder only ever need a field's `(number, data_type,
//! length)` triple to (de)serialise it — never its human-readable name. So
//! the wire path is driven by the small `Copy` [`FieldMeta`], and the
//! built-in [`FieldDef`](super::field::FieldDef) (whose `name` is a
//! `&'static str`) is left exactly as published. A loaded definition
//! ([`SpecField`]) owns its name as a `String`, so specs can be built from
//! user input at runtime without leaking memory — no `Box::leak`, no
//! `&'static` laundering.
//!
//! # Resolution order
//!
//! [`FieldSpec::lookup`] resolves a field number to a [`FieldMeta`]:
//! 1. an explicit override in this spec, else
//! 2. the built-in table — **only** when the spec was built to extend it
//!    ([`FieldSpec::extending_builtin`]); a [`closed`](FieldSpec::closed)
//!    spec returns `None` for anything it does not list.
//!
//! The built-in spec ([`FieldSpec::builtin`]) is a process-wide singleton
//! with no overrides, so `parse_with` / `build_with` allocate nothing and
//! behave exactly as they did before this module existed.
//!
//! # Loading from a file
//!
//! With the `spec-load` feature enabled, [`FieldSpec::from_toml_str`] parses
//! a spec from a TOML document (see that method for the schema). The feature
//! is opt-in so the codec core stays zero-dependency for callers that only
//! use the built-in table or build specs programmatically.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::field::{field_def, DataType, LengthSpec};

/// Highest field number an ISO 8583 bitmap can address (fields 1..=128).
const MAX_FIELD: u8 = 128;

/// The minimal field description the codec needs to (de)serialise a field:
/// its number and its `(type, length)` envelope.
///
/// This is the hot-path view shared by the built-in table and runtime specs.
/// It is deliberately `Copy` and name-free — names are presentation metadata
/// carried by [`SpecField`] / [`FieldDef`](super::field::FieldDef), not
/// consulted on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldMeta {
    pub number: u8,
    pub data_type: DataType,
    pub length: LengthSpec,
}

/// A named field definition carried by a [`FieldSpec`].
///
/// Unlike the built-in [`FieldDef`](super::field::FieldDef) (whose `name` is
/// `&'static str`), a loaded definition owns its name, so a spec assembled
/// from a config file or operator input needs no `'static` storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpecField {
    pub number: u8,
    pub data_type: DataType,
    pub length: LengthSpec,
    pub name: String,
}

impl SpecField {
    /// Construct a definition. `name` is accepted by anything that converts
    /// into a `String` (`&str`, `String`, `Cow`).
    pub fn new(
        number: u8,
        data_type: DataType,
        length: LengthSpec,
        name: impl Into<String>,
    ) -> Self {
        SpecField {
            number,
            data_type,
            length,
            name: name.into(),
        }
    }

    fn meta(&self) -> FieldMeta {
        FieldMeta {
            number: self.number,
            data_type: self.data_type,
            length: self.length,
        }
    }
}

/// A resolved set of ISO 8583 field definitions used to drive
/// [`parse_with_spec`](super::parser::parse_with_spec) /
/// [`build_with_spec`](super::builder::build_with_spec).
///
/// Build one with [`extending_builtin`](Self::extending_builtin) (standard
/// ISO 8583 plus a few overrides) or [`closed`](Self::closed) (only the
/// fields you list are valid). The default codec entry points use
/// [`builtin`](Self::builtin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSpec {
    name: String,
    overrides: BTreeMap<u8, SpecField>,
    base_builtin: bool,
}

/// Failure modes for assembling a [`FieldSpec`]. Follows the project error
/// contract: what was wrong, what was expected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// A definition's field number was 0 or above 128 — outside the range an
    /// ISO 8583 bitmap can address. `field` is the offending value widened to
    /// `u16` so values above 255 (e.g. from an untyped loader) survive the
    /// report intact.
    FieldNumberOutOfRange { field: u16, min: u8, max: u8 },
    /// An LLVAR field's `max` exceeds 99 or an LLLVAR field's `max` exceeds
    /// 999 — the wire length prefix cannot represent values beyond those
    /// bounds (LLVAR is 2 decimal digits, LLLVAR is 3 decimal digits).
    /// Accepting such a spec would silently allow building messages that
    /// cannot be decoded by a compliant receiver.
    LengthMaxTooLarge {
        field: u8,
        prefix_digits: u8,
        max: usize,
    },
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecError::FieldNumberOutOfRange { field, min, max } => write!(
                f,
                "field number {field} is out of range: must be {min}..={max}"
            ),
            SpecError::LengthMaxTooLarge {
                field,
                prefix_digits,
                max,
            } => write!(
                f,
                "field {field}: length max {max} exceeds the capacity of a \
                 {prefix_digits}-digit wire prefix (max representable value is {}); \
                 reduce max or use a wider length-prefix variant",
                if *prefix_digits == 2 { 99 } else { 999 }
            ),
        }
    }
}

impl std::error::Error for SpecError {}

impl FieldSpec {
    /// The process-wide built-in ISO 8583-1987 spec.
    ///
    /// No overrides; every [`lookup`](Self::lookup) falls through to the
    /// compile-time [`field_def`] table. Returned by reference from a
    /// [`OnceLock`] so the default `parse_with` / `build_with` path is
    /// allocation-free and behaviourally identical to using the table
    /// directly.
    pub fn builtin() -> &'static FieldSpec {
        static BUILTIN: OnceLock<FieldSpec> = OnceLock::new();
        BUILTIN.get_or_init(|| FieldSpec {
            name: String::from("ISO 8583-1987 (built-in)"),
            overrides: BTreeMap::new(),
            base_builtin: true,
        })
    }

    /// Build a spec that **extends** the built-in table: the fields you list
    /// use your definitions, every other field falls back to the built-in
    /// one. The common "standard ISO 8583, but field 48 is ours" case.
    pub fn extending_builtin(
        defs: impl IntoIterator<Item = SpecField>,
    ) -> Result<FieldSpec, SpecError> {
        Self::assemble("custom (extends built-in)", defs, true)
    }

    /// Build a **closed** spec: only the fields you list are valid. Any field
    /// the wire or message references that is not listed is rejected as
    /// [`UnknownField`](super::parser::ParseError::UnknownField) /
    /// [`UnknownField`](super::builder::BuildError::UnknownField). Use this to
    /// pin a private scheme that must not silently accept ISO defaults.
    pub fn closed(
        name: impl Into<String>,
        defs: impl IntoIterator<Item = SpecField>,
    ) -> Result<FieldSpec, SpecError> {
        Self::assemble(name, defs, false)
    }

    fn assemble(
        name: impl Into<String>,
        defs: impl IntoIterator<Item = SpecField>,
        base_builtin: bool,
    ) -> Result<FieldSpec, SpecError> {
        let mut overrides = BTreeMap::new();
        for d in defs {
            if d.number == 0 || d.number > MAX_FIELD {
                return Err(SpecError::FieldNumberOutOfRange {
                    field: u16::from(d.number),
                    min: 1,
                    max: MAX_FIELD,
                });
            }
            // Validate that the LengthSpec max fits in the wire prefix.
            // LLVAR uses a 2-digit prefix (representable range 0..=99).
            // LLLVAR uses a 3-digit prefix (representable range 0..=999).
            // Accepting a larger max would let callers build messages that
            // cannot be decoded by a compliant receiver.
            match d.length {
                LengthSpec::LLVAR { max } if max > 99 => {
                    return Err(SpecError::LengthMaxTooLarge {
                        field: d.number,
                        prefix_digits: 2,
                        max,
                    });
                }
                LengthSpec::LLLVAR { max } if max > 999 => {
                    return Err(SpecError::LengthMaxTooLarge {
                        field: d.number,
                        prefix_digits: 3,
                        max,
                    });
                }
                _ => {}
            }
            // Later definitions for the same number win — last-wins matches
            // how a layered config (base then override file) reads.
            overrides.insert(d.number, d);
        }
        Ok(FieldSpec {
            name: name.into(),
            overrides,
            base_builtin,
        })
    }

    /// This spec's human-readable name (for diagnostics / UI).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether unlisted fields fall back to the built-in table.
    pub fn extends_builtin(&self) -> bool {
        self.base_builtin
    }

    /// Resolve a field number to its `(type, length)` envelope, or `None` if
    /// this spec does not define it (see the module-level resolution order).
    pub fn lookup(&self, n: u8) -> Option<FieldMeta> {
        if let Some(d) = self.overrides.get(&n) {
            return Some(d.meta());
        }
        if self.base_builtin {
            return field_def(n).map(|d| FieldMeta {
                number: d.number,
                data_type: d.data_type,
                length: d.length,
            });
        }
        None
    }

    /// Parse a [`FieldSpec`] from a TOML document.
    ///
    /// # Schema
    ///
    /// ```toml
    /// # Optional. Defaults to a generic label.
    /// name = "national scheme"
    /// # Optional, default true. When true, unlisted fields fall back to the
    /// # built-in ISO 8583-1987 table; when false the spec is closed.
    /// extends_builtin = true
    ///
    /// [[field]]
    /// number = 48                 # 1..=128
    /// type = "binary"             # see the data-type tokens below
    /// length = { fixed = 3 }      # or { llvar = 19 } / { lllvar = 999 }
    /// name = "Private TLV"
    /// ```
    ///
    /// `type` tokens: `numeric`, `alpha`, `special`, `alpha_numeric`,
    /// `alpha_special`, `numeric_special`, `alpha_numeric_special`, `binary`,
    /// `track`.
    ///
    /// Requires the `spec-load` cargo feature.
    #[cfg(feature = "spec-load")]
    pub fn from_toml_str(doc: &str) -> Result<FieldSpec, SpecLoadError> {
        load::from_toml_str(doc)
    }
}

/// Failure modes for [`FieldSpec::from_toml_str`]. Three-element errors: what
/// failed, where, and (for type tokens) what was expected.
#[cfg(feature = "spec-load")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecLoadError {
    /// The document was not valid TOML, or did not match the spec schema.
    /// Carries the underlying parser message.
    Parse(String),
    /// A field's `type` token was not one of the recognised data types.
    UnknownDataType { field: u8, got: String },
    /// The parsed definitions were structurally valid TOML but did not form a
    /// legal spec (e.g. a field number outside 1..=128).
    Invalid(SpecError),
}

#[cfg(feature = "spec-load")]
impl std::fmt::Display for SpecLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpecLoadError::Parse(e) => write!(f, "could not parse spec document: {e}"),
            SpecLoadError::UnknownDataType { field, got } => write!(
                f,
                "field {field}: unknown data type {got:?}; expected one of numeric, alpha, \
                 special, alpha_numeric, alpha_special, numeric_special, \
                 alpha_numeric_special, binary, track"
            ),
            SpecLoadError::Invalid(e) => write!(f, "invalid spec: {e}"),
        }
    }
}

#[cfg(feature = "spec-load")]
impl std::error::Error for SpecLoadError {}

/// TOML deserialisation glue. Kept in a child module so the serde-derived
/// representation types stay private and only compile under `spec-load`.
#[cfg(feature = "spec-load")]
mod load {
    use super::{DataType, FieldSpec, LengthSpec, SpecField, SpecLoadError};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct SpecFileRepr {
        #[serde(default)]
        name: Option<String>,
        #[serde(default = "default_extends")]
        extends_builtin: bool,
        #[serde(default, rename = "field")]
        fields: Vec<FieldRepr>,
    }

    fn default_extends() -> bool {
        true
    }

    #[derive(Debug, Deserialize)]
    struct FieldRepr {
        number: u8,
        #[serde(rename = "type")]
        data_type: String,
        length: LengthRepr,
        name: String,
    }

    /// `length = { fixed = N }` / `{ llvar = MAX }` / `{ lllvar = MAX }` — an
    /// externally-tagged enum, i.e. a single-key inline table.
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum LengthRepr {
        Fixed(usize),
        Llvar(usize),
        Lllvar(usize),
    }

    impl From<LengthRepr> for LengthSpec {
        fn from(r: LengthRepr) -> LengthSpec {
            match r {
                LengthRepr::Fixed(n) => LengthSpec::Fixed(n),
                LengthRepr::Llvar(max) => LengthSpec::LLVAR { max },
                LengthRepr::Lllvar(max) => LengthSpec::LLLVAR { max },
            }
        }
    }

    fn parse_data_type(token: &str) -> Option<DataType> {
        Some(match token {
            "numeric" => DataType::Numeric,
            "alpha" => DataType::Alpha,
            "special" => DataType::Special,
            "alpha_numeric" => DataType::AlphaNumeric,
            "alpha_special" => DataType::AlphaSpecial,
            "numeric_special" => DataType::NumericSpecial,
            "alpha_numeric_special" => DataType::AlphaNumericSpecial,
            "binary" => DataType::Binary,
            "track" => DataType::Track,
            _ => return None,
        })
    }

    pub(super) fn from_toml_str(doc: &str) -> Result<FieldSpec, SpecLoadError> {
        let repr: SpecFileRepr =
            toml::from_str(doc).map_err(|e| SpecLoadError::Parse(e.to_string()))?;
        let mut defs = Vec::with_capacity(repr.fields.len());
        for f in repr.fields {
            let data_type =
                parse_data_type(&f.data_type).ok_or(SpecLoadError::UnknownDataType {
                    field: f.number,
                    got: f.data_type.clone(),
                })?;
            defs.push(SpecField::new(f.number, data_type, f.length.into(), f.name));
        }
        let name = repr.name.unwrap_or_else(|| {
            String::from(if repr.extends_builtin {
                "custom (extends built-in)"
            } else {
                "custom (closed)"
            })
        });
        FieldSpec::assemble(name, defs, repr.extends_builtin).map_err(SpecLoadError::Invalid)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn builtin_lookup_matches_field_table() {
        let spec = FieldSpec::builtin();
        for n in 1u8..=MAX_FIELD {
            let table = field_def(n).map(|d| (d.number, d.data_type, d.length));
            let via_spec = spec.lookup(n).map(|m| (m.number, m.data_type, m.length));
            assert_eq!(table, via_spec, "field {n} disagrees with built-in table");
        }
        assert_eq!(spec.lookup(0), None);
        assert_eq!(spec.lookup(129), None);
    }

    #[test]
    fn extending_builtin_overrides_one_field_keeps_rest() {
        let spec = FieldSpec::extending_builtin([SpecField::new(
            48,
            DataType::Binary,
            LengthSpec::Fixed(3),
            "Private TLV envelope",
        )])
        .unwrap();
        // Overridden field reflects the new envelope.
        assert_eq!(
            spec.lookup(48).map(|m| m.length),
            Some(LengthSpec::Fixed(3))
        );
        // A non-overridden field still resolves from the built-in table.
        assert_eq!(
            spec.lookup(2).map(|m| m.length),
            Some(LengthSpec::LLVAR { max: 19 })
        );
    }

    #[test]
    fn closed_spec_rejects_unlisted_fields() {
        let spec = FieldSpec::closed(
            "tiny",
            [SpecField::new(
                2,
                DataType::Numeric,
                LengthSpec::LLVAR { max: 19 },
                "PAN",
            )],
        )
        .unwrap();
        assert!(spec.lookup(2).is_some());
        // Field 3 is in the built-in table but NOT this closed spec.
        assert_eq!(spec.lookup(3), None);
    }

    #[test]
    fn rejects_out_of_range_field_numbers() {
        assert_eq!(
            FieldSpec::closed(
                "bad",
                [SpecField::new(
                    0,
                    DataType::Numeric,
                    LengthSpec::Fixed(1),
                    "zero"
                )]
            ),
            Err(SpecError::FieldNumberOutOfRange {
                field: 0,
                min: 1,
                max: 128
            })
        );
    }

    #[test]
    fn last_definition_wins_for_duplicate_field() {
        let spec = FieldSpec::extending_builtin([
            SpecField::new(60, DataType::Numeric, LengthSpec::Fixed(3), "first"),
            SpecField::new(60, DataType::Binary, LengthSpec::Fixed(8), "second"),
        ])
        .unwrap();
        assert_eq!(
            spec.lookup(60).map(|m| m.length),
            Some(LengthSpec::Fixed(8))
        );
    }
}

#[cfg(all(test, feature = "spec-load"))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod load_tests {
    use super::*;
    use crate::iso8583::{build_with_spec, parse_with_spec, Dialect, Iso8583Message};
    use std::collections::BTreeMap;

    #[test]
    fn loads_extending_spec_from_toml() {
        let doc = r#"
            name = "national"
            extends_builtin = true

            [[field]]
            number = 48
            type = "binary"
            length = { fixed = 3 }
            name = "Private TLV"
        "#;
        let spec = FieldSpec::from_toml_str(doc).unwrap();
        assert_eq!(spec.name(), "national");
        assert!(spec.extends_builtin());
        assert_eq!(
            spec.lookup(48).map(|m| m.length),
            Some(LengthSpec::Fixed(3))
        );
        assert_eq!(
            spec.lookup(2).map(|m| m.length),
            Some(LengthSpec::LLVAR { max: 19 })
        );
    }

    #[test]
    fn loads_closed_spec_and_rejects_others() {
        let doc = r#"
            name = "pan-only"
            extends_builtin = false

            [[field]]
            number = 2
            type = "numeric"
            length = { llvar = 19 }
            name = "PAN"
        "#;
        let spec = FieldSpec::from_toml_str(doc).unwrap();
        assert!(!spec.extends_builtin());
        assert!(spec.lookup(2).is_some());
        assert_eq!(spec.lookup(3), None);
    }

    #[test]
    fn rejects_unknown_data_type() {
        let doc = r#"
            [[field]]
            number = 2
            type = "frobnicate"
            length = { llvar = 19 }
            name = "PAN"
        "#;
        assert!(matches!(
            FieldSpec::from_toml_str(doc),
            Err(SpecLoadError::UnknownDataType { field: 2, .. })
        ));
    }

    #[test]
    fn rejects_out_of_range_number_via_loader() {
        // 200 fits in a u8 so TOML parses it; assemble then rejects > 128.
        let doc = r#"
            [[field]]
            number = 200
            type = "numeric"
            length = { fixed = 1 }
            name = "bad"
        "#;
        assert!(matches!(
            FieldSpec::from_toml_str(doc),
            Err(SpecLoadError::Invalid(
                SpecError::FieldNumberOutOfRange { .. }
            ))
        ));
    }

    #[test]
    fn rejects_non_toml_document() {
        assert!(matches!(
            FieldSpec::from_toml_str("this is not = = toml"),
            Err(SpecLoadError::Parse(_))
        ));
    }

    #[test]
    fn loaded_spec_round_trips_a_message() {
        // Field 105 is "Reserved" in the built-in table; redefine it Fixed(3).
        let doc = r#"
            [[field]]
            number = 105
            type = "numeric"
            length = { fixed = 3 }
            name = "national 105"
        "#;
        let spec = FieldSpec::from_toml_str(doc).unwrap();
        let mut fields = BTreeMap::new();
        fields.insert(105u8, b"123".to_vec());
        let msg = Iso8583Message {
            mti: *b"0210",
            fields,
        };
        let wire = build_with_spec(&msg, Dialect::HybridAscii, &spec).unwrap();
        let back = parse_with_spec(&wire, Dialect::HybridAscii, &spec).unwrap();
        assert_eq!(back, msg);
    }
}
