//! Semantic decoders for SWIFT MT block-4 fields.
//!
//! The structural parser in [`super`] returns block-4 fields as raw
//! `tag` + `value` string pairs — sufficient for tag-level diff and the
//! initial wf-cli display, but not enough to compare two messages by
//! *meaning* (a downstream PLAN-v0.4 S6 MT↔MX diff requirement).
//!
//! This module adds the semantic layer on top of that structural layer:
//! each supported tag has a small decoder that lifts the raw value into a
//! typed [`FieldSemantic`] variant. Unrecognised tags fall through as
//! [`FieldSemantic::Raw`] so the diff layer can still match by string —
//! the decoder is intentionally *additive*; a missing implementation
//! degrades gracefully rather than rejecting the message.
//!
//! # Extension
//!
//! Adding a new tag is a three-line change:
//! 1. Create `field_NN.rs` with a unit struct implementing
//!    [`MtFieldDecoder`].
//! 2. Register it in [`registry::decode_field`].
//! 3. Add round-trip tests against the SWIFT user handbook spec.
//!
//! The [`MtFieldDecoder`] trait is the explicit extension point — it
//! exists so additional crates (or sprint-late tag rollouts) can plug in
//! without touching the structural parser.

pub mod field_20;
pub mod field_32a;
pub mod field_50k;
pub mod registry;

pub use field_20::Field20;
pub use field_32a::Field32A;
pub use field_50k::Field50K;
pub use registry::decode_field;

/// Typed view of a single block-4 field's payload.
///
/// `Raw` is the fall-back for tags this build does not yet decode, so the
/// diff layer (PLAN-v0.4 S6) can still match on the string verbatim. New
/// variants are added without breaking existing matches via the standard
/// `#[non_exhaustive]` pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldSemantic {
    /// Field 20 / 21 style: a single reference identifier, SWIFT-X
    /// charset, length-bounded.
    Reference(String),
    /// Field 32A / 32B style: 6-digit value date + 3-letter currency +
    /// numeric amount with `,` decimal mark. Stored as decoded strings
    /// (rather than chrono `NaiveDate` and `rust_decimal::Decimal`) to
    /// keep this crate free of heavy date / decimal deps; downstream
    /// consumers can re-parse the typed strings.
    ValueDateAmount {
        /// `YYMMDD` — 6 ASCII digits, calendar validity already checked.
        date: String,
        /// Three uppercase ASCII letters (ISO 4217 code, validity
        /// *shape* checked but not against a code table).
        currency: String,
        /// Amount with `,` decimal separator preserved verbatim. The
        /// decoder validates that this is a non-empty numeric token
        /// with at most one `,` and no other punctuation.
        amount: String,
    },
    /// Field 50K-style party identifier: an optional `/account` line
    /// followed by up to 4 name/address lines of ≤ 35 chars each.
    Party {
        /// Account string (the substring after `/` on the first line)
        /// if the first line started with `/`; `None` otherwise.
        account: Option<String>,
        /// Name & address lines in source order, leading `/` stripped
        /// from the account line if present.
        lines: Vec<String>,
    },
    /// Tag is not (yet) decoded by this build. Holds the raw string
    /// value verbatim so callers can fall back to byte / string
    /// comparison without losing data.
    Raw(String),
}

/// Failure modes for [`MtFieldDecoder::decode`]. Each variant carries the
/// tag and the offending substring so error messages can point at the
/// exact piece of data that broke the contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Field value's length is outside the spec range. `max` is the
    /// per-tag spec maximum (e.g. 16 for tag 20).
    InvalidLength {
        tag: &'static str,
        got: usize,
        max: usize,
    },
    /// Field contained a character outside the spec's allowed charset
    /// (typically SWIFT X = `A-Z 0-9 / - ? : ( ) . , ' +` and space, but
    /// some tags allow lowercase or the wider SWIFT Y / Z sets).
    InvalidCharset { tag: &'static str, value: String },
    /// Date component did not parse as `YYMMDD` with calendar-valid
    /// month and day.
    InvalidDate { tag: &'static str, value: String },
    /// Amount component was not in the SWIFT numeric format (digits
    /// plus exactly one `,` as the decimal separator).
    InvalidAmount { tag: &'static str, value: String },
    /// Currency component was not 3 uppercase ASCII letters.
    InvalidCurrency { tag: &'static str, value: String },
}

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DecodeError::InvalidLength { tag, got, max } => {
                write!(f, "field {tag}: length {got} exceeds spec max {max}")
            }
            DecodeError::InvalidCharset { tag, value } => {
                write!(
                    f,
                    "field {tag}: value {value:?} contains out-of-charset bytes"
                )
            }
            DecodeError::InvalidDate { tag, value } => {
                write!(
                    f,
                    "field {tag}: date component {value:?} is not a valid YYMMDD"
                )
            }
            DecodeError::InvalidAmount { tag, value } => {
                write!(
                    f,
                    "field {tag}: amount component {value:?} is not a valid SWIFT numeric"
                )
            }
            DecodeError::InvalidCurrency { tag, value } => {
                write!(
                    f,
                    "field {tag}: currency component {value:?} is not 3 uppercase letters"
                )
            }
        }
    }
}

impl std::error::Error for DecodeError {}

/// Per-tag decoder contract.
///
/// Implementors are typically unit structs (zero-sized) so the trait can
/// be invoked through a `&Decoder` reference without runtime cost. The
/// trait is `pub` so external crates can plug in tags this build doesn't
/// ship — see the module-level docs for the registration recipe.
pub trait MtFieldDecoder {
    /// The MT tag this decoder handles, e.g. `"20"` or `"32A"`.
    fn tag(&self) -> &'static str;

    /// Decode a raw field value. The input does not include the leading
    /// `:tag:` framing — only the value that the structural parser put
    /// into [`super::MtField::value`].
    fn decode(&self, raw: &str) -> Result<FieldSemantic, DecodeError>;
}

/// `true` if `b` is in the SWIFT "X" character set as defined in the
/// SWIFT MT User Handbook. The X set is the most restrictive: uppercase
/// Latin letters, digits, and a fixed list of punctuation.
///
/// Tags that accept lowercase or wider charsets call [`is_swift_y`] or
/// [`is_swift_z`] instead (those helpers will land alongside their first
/// consumer — Y/Z were not needed for the three MVP tags).
pub(crate) fn is_swift_x(b: u8) -> bool {
    b.is_ascii_uppercase()
        || b.is_ascii_digit()
        || matches!(
            b,
            b'/' | b'-' | b'?' | b':' | b'(' | b')' | b'.' | b',' | b'\'' | b'+' | b' '
        )
}
