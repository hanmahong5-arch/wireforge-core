//! Format-agnostic view of a parsed wire message.
//!
//! The conformance engine ([`crate::check_conformance`]) never looks at a
//! concrete codec type — it compares two values through the [`WireMessage`]
//! trait. ISO 8583 is the first (and, in this PoC, only) implementation
//! ([`crate::iso8583::Iso8583View`]); MX paths and fixed-offset records plug
//! in later behind the same trait without touching the masked-diff core.
//!
//! # Why occurrences are a slice, not a single value
//!
//! [`WireMessage::field_occurrences`] returns `&[Vec<u8>]` rather than
//! `Option<&[u8]>`. ISO 8583 fields are 0-or-1 (so the slice is always length
//! 0 or 1), but modelling multi-occurrence *structurally* — rather than
//! bolting it on when MX arrives — means the count-mismatch / per-occurrence
//! branches of the engine are real and exercised from day one (see the
//! multi-occurrence stub test), not vaporware.

/// A field address within a wire message.
///
/// A key is either an ISO 8583 data-element number or the ordinal position of
/// a field in a fixed-length record layout; the enum stays open (a future
/// `MxPath(String)` variant) so the engine's key universe is format-agnostic.
/// The two families are never mixed inside one comparison — a report's rows
/// come from one message format.
///
/// `Ord` is derived so the report's rows have a deterministic, stable order
/// regardless of the order fields appear on the wire or in the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldKey {
    /// An ISO 8583 field. `0` is a synthetic slot for the **MTI** (field 0 is
    /// never a real data element, so it is a safe carrier); `1..=128` are the
    /// data elements addressable by the bitmap.
    Iso8583(u8),
    /// The zero-based ordinal position of a field in a fixed-length record
    /// layout ([`crate::fixed::FixedLayout`]).
    Ordinal(u16),
}

impl FieldKey {
    /// The stable field number this key addresses, for row labels and error
    /// messages: the ISO 8583 data-element number (`0` = MTI), or the
    /// zero-based ordinal in a fixed-length layout.
    pub fn number(self) -> u16 {
        match self {
            FieldKey::Iso8583(n) => u16::from(n),
            FieldKey::Ordinal(i) => i,
        }
    }
}

/// A parsed wire message reduced to the minimum the engine needs: the set of
/// field keys present, the raw byte occurrences of each, and a human label.
///
/// Implementations carry **decoded payload bytes** (no length prefixes, no
/// bitmap framing) so the masked diff compares logical field values, not wire
/// encodings. Two messages that differ only in dialect (e.g. ASCII vs BCD
/// length prefixes) therefore compare equal.
pub trait WireMessage {
    /// Every field key this message carries, including any synthetic slots
    /// (the ISO 8583 view always reports the MTI as `FieldKey::Iso8583(0)`).
    fn field_keys(&self) -> Vec<FieldKey>;

    /// The byte occurrences of `key`, in wire order. An **absent** field
    /// returns an empty slice; a present single-occurrence field returns a
    /// length-1 slice; repeated fields return one entry per occurrence.
    fn field_occurrences(&self, key: FieldKey) -> &[Vec<u8>];

    /// A short, human-readable label for `key` (e.g. the ISO 8583 field name
    /// from the spec table). Presentation only — the engine never branches on
    /// it.
    fn field_label(&self, key: FieldKey) -> String;
}
