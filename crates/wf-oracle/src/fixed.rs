//! Fixed-length record implementation of [`WireMessage`] — the second
//! interface family, covering the classic "length-prefixed / fixed-offset
//! TCP" host interfaces that dominate legacy bank front-ends.
//!
//! A [`FixedLayout`] is an ordered list of named fields, each a fixed byte
//! length, optionally ending in one variable-length tail that consumes the
//! remaining bytes. [`FixedLayout::parse`] **tiles** a frame with those
//! lengths: the fields must account for every byte exactly (no silent
//! truncation, no trailing remainder), which makes a successful parse a
//! structural statement — "this layout explains this frame's bytes".
//!
//! That exact-tiling property is what the layout-draft check builds on: a
//! field table recovered from an interface spec can be validated against
//! captured frames *before* anyone trusts it. It is a **structural** check
//! only — it says nothing about field values or semantics.

use crate::wire::{FieldKey, WireMessage};

/// The byte length of one field in a [`FixedLayout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedLen {
    /// Exactly `n` bytes (`n > 0`).
    Bytes(usize),
    /// All remaining bytes of the frame (possibly zero). Only legal as the
    /// **last** field of a layout.
    Rest,
}

/// One named field of a fixed-length record layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedField {
    /// Human-readable field name (presentation only — the engine keys fields
    /// by ordinal position, so two fields may even share a name).
    pub name: String,
    /// The field's byte length.
    pub len: FixedLen,
}

impl FixedField {
    /// A fixed-width field of `len` bytes.
    pub fn bytes(name: impl Into<String>, len: usize) -> Self {
        FixedField {
            name: name.into(),
            len: FixedLen::Bytes(len),
        }
    }

    /// A variable tail consuming the rest of the frame.
    pub fn rest(name: impl Into<String>) -> Self {
        FixedField {
            name: name.into(),
            len: FixedLen::Rest,
        }
    }
}

/// An ordered fixed-length record layout: the recovered field table for one
/// message segment (a request, or one service's response).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedLayout {
    /// Layout label (e.g. `"cmc svc_00 response"`), carried into reports.
    pub name: String,
    /// The fields in wire order.
    pub fields: Vec<FixedField>,
}

impl FixedLayout {
    /// Build a layout and [`validate`](Self::validate) it in one step.
    pub fn new(
        name: impl Into<String>,
        fields: impl IntoIterator<Item = FixedField>,
    ) -> Result<Self, String> {
        let layout = FixedLayout {
            name: name.into(),
            fields: fields.into_iter().collect(),
        };
        layout.validate()?;
        Ok(layout)
    }

    /// Reject a malformed layout: it must have at least one field, no
    /// zero-length fixed field, at most one [`FixedLen::Rest`] and only in
    /// the last position, and at most `u16::MAX` fields (the ordinal key
    /// width).
    pub fn validate(&self) -> Result<(), String> {
        if self.fields.is_empty() {
            return Err(format!(
                "layout {:?} has no fields; a layout needs at least one field",
                self.name
            ));
        }
        if self.fields.len() > usize::from(u16::MAX) {
            return Err(format!(
                "layout {:?} has {} fields; at most {} are addressable",
                self.name,
                self.fields.len(),
                u16::MAX
            ));
        }
        let last = self.fields.len() - 1;
        for (i, f) in self.fields.iter().enumerate() {
            match f.len {
                FixedLen::Bytes(0) => {
                    return Err(format!(
                        "layout {:?} field {i} ({:?}) has length 0; every fixed field \
                         must be at least 1 byte",
                        self.name, f.name
                    ));
                }
                FixedLen::Rest if i != last => {
                    return Err(format!(
                        "layout {:?} field {i} ({:?}) is a variable tail but is not the \
                         last field; only the final field may consume the rest of the frame",
                        self.name, f.name
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The sum of the fixed field lengths (the frame length this layout
    /// tiles exactly, or — with a variable tail — the minimum frame length).
    pub fn fixed_len_sum(&self) -> usize {
        self.fields
            .iter()
            .map(|f| match f.len {
                FixedLen::Bytes(n) => n,
                FixedLen::Rest => 0,
            })
            .sum()
    }

    /// Whether the layout ends in a variable tail.
    pub fn has_rest(&self) -> bool {
        matches!(self.fields.last().map(|f| f.len), Some(FixedLen::Rest))
    }

    /// Tile `frame` with this layout's field lengths.
    ///
    /// Succeeds only when the lengths account for **every** byte: without a
    /// variable tail the frame length must equal
    /// [`fixed_len_sum`](Self::fixed_len_sum) exactly; with one it must be at
    /// least that (the tail takes the remainder, possibly empty). The error
    /// names the frame length and the layout's requirement, so a mismatch
    /// reads as evidence against the layout draft, not a crash.
    pub fn parse(&self, frame: &[u8]) -> Result<FixedView, String> {
        self.validate()?;
        let need = self.fixed_len_sum();
        if self.has_rest() {
            if frame.len() < need {
                return Err(format!(
                    "frame is {} byte(s) but layout {:?} needs at least {need} \
                     before its variable tail",
                    frame.len(),
                    self.name
                ));
            }
        } else if frame.len() != need {
            return Err(format!(
                "frame is {} byte(s) but layout {:?} tiles exactly {need}; \
                 the field lengths must account for every byte",
                frame.len(),
                self.name
            ));
        }
        let mut fields = Vec::with_capacity(self.fields.len());
        let mut offset = 0usize;
        for f in &self.fields {
            let take = match f.len {
                FixedLen::Bytes(n) => n,
                FixedLen::Rest => frame.len() - offset,
            };
            fields.push((f.name.clone(), vec![frame[offset..offset + take].to_vec()]));
            offset += take;
        }
        Ok(FixedView { fields })
    }
}

/// A frame parsed under a [`FixedLayout`], viewed through the engine's
/// [`WireMessage`] trait. Fields are keyed by [`FieldKey::Ordinal`] (their
/// zero-based position in the layout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedView {
    /// `(name, occurrences)` per layout position. Fixed-length records have
    /// exactly one occurrence per field; the `Vec<Vec<u8>>` shape matches the
    /// trait's structural multi-occurrence model.
    fields: Vec<(String, Vec<Vec<u8>>)>,
}

impl WireMessage for FixedView {
    fn field_keys(&self) -> Vec<FieldKey> {
        (0..self.fields.len())
            .map(|i| FieldKey::Ordinal(i as u16))
            .collect()
    }

    fn field_occurrences(&self, key: FieldKey) -> &[Vec<u8>] {
        match key {
            FieldKey::Ordinal(i) => self
                .fields
                .get(usize::from(i))
                .map_or(&[], |(_, occ)| occ.as_slice()),
            _ => &[],
        }
    }

    fn field_label(&self, key: FieldKey) -> String {
        match key {
            FieldKey::Ordinal(i) => self
                .fields
                .get(usize::from(i))
                .map_or_else(|| "Unknown".to_string(), |(name, _)| name.clone()),
            _ => "Unknown".to_string(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn layout() -> FixedLayout {
        // 4 + 2 + 6 = 12 bytes, hand-summed — the tiling anchor every
        // expectation below is computed from.
        FixedLayout::new(
            "t",
            [
                FixedField::bytes("len", 4),
                FixedField::bytes("code", 2),
                FixedField::bytes("body", 6),
            ],
        )
        .expect("valid layout")
    }

    #[test]
    fn parse_tiles_exact_frame() {
        let view = layout().parse(b"0012OKABCDEF").expect("12 bytes tile");
        assert_eq!(
            view.field_occurrences(FieldKey::Ordinal(0)),
            &[b"0012".to_vec()]
        );
        assert_eq!(
            view.field_occurrences(FieldKey::Ordinal(1)),
            &[b"OK".to_vec()]
        );
        assert_eq!(
            view.field_occurrences(FieldKey::Ordinal(2)),
            &[b"ABCDEF".to_vec()]
        );
        assert_eq!(view.field_label(FieldKey::Ordinal(2)), "body");
    }

    #[test]
    fn parse_rejects_short_and_long_frames() {
        // 11 or 13 bytes cannot tile a 12-byte layout — no silent truncation.
        assert!(layout().parse(b"0012OKABCDE").is_err());
        assert!(layout().parse(b"0012OKABCDEFG").is_err());
    }

    #[test]
    fn rest_tail_takes_remainder() {
        let l = FixedLayout::new("v", [FixedField::bytes("len", 4), FixedField::rest("body")])
            .expect("valid layout");
        let view = l.parse(b"0003abc").expect("4 + rest");
        assert_eq!(
            view.field_occurrences(FieldKey::Ordinal(1)),
            &[b"abc".to_vec()]
        );
        // Frame shorter than the fixed prefix cannot parse.
        assert!(l.parse(b"001").is_err());
    }

    #[test]
    fn validate_rejects_mid_layout_rest_and_zero_len() {
        assert!(FixedLayout::new(
            "bad",
            [FixedField::rest("tail"), FixedField::bytes("after", 2)],
        )
        .is_err());
        assert!(FixedLayout::new("bad", [FixedField::bytes("zero", 0)]).is_err());
        assert!(FixedLayout::new("bad", []).is_err());
    }
}
