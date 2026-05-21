//! ISO 8583 message builder. Inverse of [`super::parser::parse`] /
//! [`super::parser::parse_with`].
//!
//! `build_with(parse_with(bytes, d)?, d)?` reproduces the original `bytes` for
//! any structurally-valid input, and `parse_with(build_with(msg, d), d)?`
//! round-trips any `Iso8583Message` that satisfies the field-length contracts.
//! See [`super::dialect::Dialect`] for the supported wire flavours.

use crate::iso8583::dialect::Dialect;
use crate::iso8583::field::{field_def, LengthSpec};
use crate::iso8583::parser::Iso8583Message;
use core::fmt;
use wf_bitmap::{Bitmap8583, BitmapError};

/// Failure modes for [`build`] / [`build_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// MTI bytes were not all ASCII digits `'0'..='9'`.
    InvalidMti([u8; 4]),
    /// `fields` contained field number 0 or > 128.
    InvalidFieldNumber(u8),
    /// [`field_def`] returned `None` for the field number. With the
    /// current table this implies field 0 (rejected separately) or a
    /// programmer error; included for completeness.
    UnknownField(u8),
    /// A `Fixed(N)` field's payload did not have exactly `N` bytes.
    FixedLengthMismatch {
        field: u8,
        expected: usize,
        actual: usize,
    },
    /// A VAR field's payload exceeded its spec `max`.
    LengthExceedsMax {
        field: u8,
        actual: usize,
        max: usize,
    },
    /// LLVAR payload was >= 100 bytes, or LLLVAR payload was >= 1000
    /// bytes — the ASCII length prefix can't represent it.
    LengthOverflow {
        field: u8,
        actual: usize,
        prefix_digits: u8,
    },
    /// Bitmap construction failed (propagated from `wf_bitmap`).
    BitmapError(BitmapError),
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::InvalidMti(b) => {
                write!(f, "invalid MTI {:?}: not all ASCII digits", b)
            }
            BuildError::InvalidFieldNumber(n) => {
                write!(f, "invalid field number {}: must be 1..=128", n)
            }
            BuildError::UnknownField(n) => {
                write!(f, "field {} has no FieldDef in the table", n)
            }
            BuildError::FixedLengthMismatch {
                field,
                expected,
                actual,
            } => write!(
                f,
                "field {}: fixed length expected {} bytes, got {}",
                field, expected, actual
            ),
            BuildError::LengthExceedsMax { field, actual, max } => write!(
                f,
                "field {}: payload length {} exceeds spec max {}",
                field, actual, max
            ),
            BuildError::LengthOverflow {
                field,
                actual,
                prefix_digits,
            } => write!(
                f,
                "field {}: payload length {} does not fit in a {}-digit ASCII prefix",
                field, actual, prefix_digits
            ),
            BuildError::BitmapError(e) => write!(f, "bitmap build failed: {}", e),
        }
    }
}

impl std::error::Error for BuildError {}

impl From<BitmapError> for BuildError {
    fn from(err: BitmapError) -> Self {
        BuildError::BitmapError(err)
    }
}

/// Encode an [`Iso8583Message`] back to wire bytes using the [`Dialect::HybridAscii`]
/// convention. Kept as the back-compatible default — every historical caller
/// (`wf-cli`, `wf-mcp`, `tests/iso8583_message.rs`) calls this entry point.
pub fn build(msg: &Iso8583Message) -> Result<Vec<u8>, BuildError> {
    build_with(msg, Dialect::HybridAscii)
}

/// Encode an [`Iso8583Message`] back to wire bytes in the specified dialect.
///
/// Layout (per dialect):
/// - `HybridAscii`: `MTI (4B ASCII) || bitmap (8 or 16B raw) || fields`
/// - `FullAscii`:   `MTI (4B ASCII) || bitmap (16 or 32B ASCII hex) || fields`
///
/// The field-section layout is identical between dialects: for each field set
/// in the bitmap, in ascending order, emit the LLVAR/LLLVAR ASCII prefix (if
/// any) followed by the payload bytes.
pub fn build_with(msg: &Iso8583Message, dialect: Dialect) -> Result<Vec<u8>, BuildError> {
    // 1. MTI validation.
    if !msg.mti.iter().all(|b| b.is_ascii_digit()) {
        return Err(BuildError::InvalidMti(msg.mti));
    }

    // 2. Build the bitmap from the field set. Pre-validate every field's
    //    payload against its FieldDef BEFORE serialising so a bad field
    //    doesn't leave us with a half-written buffer.
    let mut bitmap = Bitmap8583::new();
    for (&n, data) in &msg.fields {
        if n == 0 {
            return Err(BuildError::InvalidFieldNumber(n));
        }
        let def = field_def(n).ok_or(BuildError::UnknownField(n))?;
        match def.length {
            LengthSpec::Fixed(expected) => {
                if data.len() != expected {
                    return Err(BuildError::FixedLengthMismatch {
                        field: n,
                        expected,
                        actual: data.len(),
                    });
                }
            }
            LengthSpec::LLVAR { max } => {
                if data.len() > max {
                    return Err(BuildError::LengthExceedsMax {
                        field: n,
                        actual: data.len(),
                        max,
                    });
                }
                if data.len() >= 100 {
                    return Err(BuildError::LengthOverflow {
                        field: n,
                        actual: data.len(),
                        prefix_digits: 2,
                    });
                }
            }
            LengthSpec::LLLVAR { max } => {
                if data.len() > max {
                    return Err(BuildError::LengthExceedsMax {
                        field: n,
                        actual: data.len(),
                        max,
                    });
                }
                if data.len() >= 1000 {
                    return Err(BuildError::LengthOverflow {
                        field: n,
                        actual: data.len(),
                        prefix_digits: 3,
                    });
                }
            }
        }
        // SPEC: Bitmap8583::set takes u16; field numbers 1..=128 fit.
        bitmap.set(u16::from(n))?;
    }

    // 3. Serialise.
    let bitmap_bytes = bitmap.encode();
    // Capacity hint: 4 (MTI) + bitmap-after-encoding + ~16/field rough average.
    let bitmap_wire_len = match dialect {
        Dialect::HybridAscii => bitmap_bytes.len(),
        Dialect::FullAscii => bitmap_bytes.len() * 2,
    };
    let mut out: Vec<u8> = Vec::with_capacity(4 + bitmap_wire_len + msg.fields.len() * 16);
    out.extend_from_slice(&msg.mti);
    match dialect {
        Dialect::HybridAscii => out.extend_from_slice(&bitmap_bytes),
        Dialect::FullAscii => write_hex_uppercase(&mut out, &bitmap_bytes),
    }

    // BTreeMap iteration is ascending by key — matches ISO 8583 ordering.
    for (&n, data) in &msg.fields {
        // field_def already verified above; re-lookup is cheap (array index).
        let def = match field_def(n) {
            Some(d) => d,
            None => return Err(BuildError::UnknownField(n)),
        };
        match def.length {
            LengthSpec::Fixed(_) => {}
            LengthSpec::LLVAR { .. } => write_ascii_len(&mut out, data.len(), 2),
            LengthSpec::LLLVAR { .. } => write_ascii_len(&mut out, data.len(), 3),
        }
        out.extend_from_slice(data);
    }

    Ok(out)
}

/// Append `bytes` to `out` as uppercase ASCII hex (`b'0'..=b'9'`, `b'A'..=b'F'`).
/// The de facto FullAscii convention (jpos, moov-io) is uppercase; the parser
/// accepts both cases, but the builder picks one for deterministic output.
fn write_hex_uppercase(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0x0f) as usize]);
    }
}

/// Append a `digits`-wide ASCII decimal length prefix. Caller must have
/// already verified that `len` fits in `digits` digits.
fn write_ascii_len(out: &mut Vec<u8>, len: usize, digits: usize) {
    // Render right-aligned, zero-padded. Safe because validated upstream.
    let mut buf = [b'0'; 4];
    let mut v = len;
    for i in (0..digits).rev() {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&buf[..digits]);
}
