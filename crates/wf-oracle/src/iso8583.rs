//! ISO 8583 implementation of [`WireMessage`] — the first interface family.
//!
//! [`Iso8583View`] wraps the decoded field map produced by
//! [`wf_codec::iso8583::parse`]. The MTI is exposed as
//! [`FieldKey::Iso8583(0)`](FieldKey) — field 0 is never a real data element,
//! so it is a safe synthetic slot that lets the engine diff the message type
//! with the same machinery as any other field. Field values are stored as the
//! codec's **decoded payload bytes** (no length prefix), so two captures that
//! differ only in wire dialect compare equal.

use std::collections::BTreeMap;

use wf_codec::iso8583::field::field_def;
use wf_codec::iso8583::{parse, Iso8583Message, ParseError};

use crate::wire::{FieldKey, WireMessage};

/// The synthetic ISO 8583 field key carrying the MTI.
const MTI_FIELD: u8 = 0;

/// A parsed ISO 8583 message viewed through the engine's [`WireMessage`]
/// trait.
///
/// Build one from raw wire bytes with [`Iso8583View::parse`] (the capture
/// path) or from an already-typed message with
/// [`Iso8583View::from_message`] (the `.wf` path, which is parsed upstream).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iso8583View {
    /// Field number → occurrences. Key `0` is the MTI (always one
    /// occurrence); keys `1..=128` are the present data elements. ISO 8583 is
    /// 0-or-1 per field, so every inner `Vec` has length 1 — but the
    /// `Vec<Vec<u8>>` shape keeps multi-occurrence structural for formats that
    /// repeat fields.
    occ: BTreeMap<u8, Vec<Vec<u8>>>,
}

impl Iso8583View {
    /// Parse raw ISO 8583 wire bytes (auto-detecting the dialect) into a view.
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        Ok(Self::from_message(parse(bytes)?))
    }

    /// Build a view from an already-parsed [`Iso8583Message`] (the `.wf`
    /// carriage path hands the engine typed messages, so no re-parse is
    /// needed).
    pub fn from_message(msg: Iso8583Message) -> Self {
        let mut occ: BTreeMap<u8, Vec<Vec<u8>>> = BTreeMap::new();
        occ.insert(MTI_FIELD, vec![msg.mti.to_vec()]);
        for (n, data) in msg.fields {
            occ.insert(n, vec![data]);
        }
        Iso8583View { occ }
    }
}

impl WireMessage for Iso8583View {
    fn field_keys(&self) -> Vec<FieldKey> {
        self.occ.keys().map(|&n| FieldKey::Iso8583(n)).collect()
    }

    fn field_occurrences(&self, key: FieldKey) -> &[Vec<u8>] {
        match key {
            FieldKey::Iso8583(n) => match self.occ.get(&n) {
                Some(v) => v.as_slice(),
                None => &[],
            },
            // A non-ISO key can only enter via a mixed-format spec; treat it
            // as absent rather than panicking.
            _ => &[],
        }
    }

    fn field_label(&self, key: FieldKey) -> String {
        match key {
            FieldKey::Iso8583(MTI_FIELD) => "MTI".to_string(),
            FieldKey::Iso8583(n) => {
                field_def(n).map_or_else(|| "Unknown".to_string(), |d| d.name.to_string())
            }
            _ => "Unknown".to_string(),
        }
    }
}
