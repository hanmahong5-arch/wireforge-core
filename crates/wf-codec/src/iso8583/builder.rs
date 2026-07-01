//! ISO 8583 message builder. Inverse of [`super::parser::parse`] /
//! [`super::parser::parse_with`].
//!
//! `build_with(parse_with(bytes, d)?, d)?` reproduces the original `bytes` for
//! any structurally-valid input, and `parse_with(build_with(msg, d), d)?`
//! round-trips any `Iso8583Message` that satisfies the field-length contracts.
//! See [`super::dialect::Dialect`] for the supported wire flavours.

use crate::iso8583::bcd;
use crate::iso8583::dialect::Dialect;
use crate::iso8583::field::{DataType, LengthSpec};
use crate::iso8583::parser::{field_wire_bytes, Iso8583Message};
use crate::iso8583::spec::FieldSpec;
use core::fmt;
use wf_bitmap::{Bitmap8583, BitmapError};

/// Failure modes for [`build`] / [`build_with`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildError {
    /// MTI bytes were not all ASCII digits `'0'..='9'`.
    InvalidMti([u8; 4]),
    /// `fields` contained field number 0, 1 (reserved secondary-bitmap
    /// indicator — auto-managed by the encoder, must not be supplied by the
    /// caller), or > 128 (outside the addressable bitmap range).
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
    /// bytes — the length prefix can't represent it.
    LengthOverflow {
        field: u8,
        actual: usize,
        prefix_digits: u8,
    },
    /// Bitmap construction failed (propagated from `wf_bitmap`).
    BitmapError(BitmapError),
    /// `FullBinary` build saw a Numeric field whose payload contained a
    /// byte that is not an ASCII digit. The field is reported alongside
    /// the offending byte value so the caller can pinpoint which field's
    /// data is malformed without re-scanning.
    InvalidBcdDigit { field: u8, byte: u8 },
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuildError::InvalidMti(b) => {
                write!(f, "invalid MTI {:?}: not all ASCII digits", b)
            }
            BuildError::InvalidFieldNumber(n) => {
                write!(
                    f,
                    "invalid field number {}: must be 2..=128 \
                     (0 is out of range; 1 is the auto-managed secondary-bitmap \
                     indicator and must not be supplied by the caller; \
                     >128 is outside the addressable bitmap range)",
                    n
                )
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
                "field {}: payload length {} does not fit in a {}-digit prefix",
                field, actual, prefix_digits
            ),
            BuildError::BitmapError(e) => write!(f, "bitmap build failed: {}", e),
            BuildError::InvalidBcdDigit { field, byte } => write!(
                f,
                "field {}: Numeric payload byte {:#x} is not an ASCII digit (cannot BCD-pack)",
                field, byte
            ),
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
/// - `FullBinary`:  `MTI (2B BCD)   || bitmap (8 or 16B raw) || BCD-prefixed fields`
///
/// The field-section layout shares structure across dialects (emit each
/// field's length prefix followed by its payload) but the encoding of both
/// the prefix and the Numeric payload depends on dialect.
pub fn build_with(msg: &Iso8583Message, dialect: Dialect) -> Result<Vec<u8>, BuildError> {
    build_with_spec(msg, dialect, FieldSpec::builtin())
}

/// Encode an [`Iso8583Message`] in the specified dialect using a
/// caller-supplied [`FieldSpec`].
///
/// The runtime-configurable counterpart to [`build_with`]: pass a spec built
/// with [`FieldSpec::extending_builtin`] / [`FieldSpec::closed`] to emit a
/// national / private dialect. With [`FieldSpec::builtin`] it is byte-for-byte
/// identical to [`build_with`].
pub fn build_with_spec(
    msg: &Iso8583Message,
    dialect: Dialect,
    spec: &FieldSpec,
) -> Result<Vec<u8>, BuildError> {
    // 1. MTI validation — the ASCII representation in `msg.mti` is the
    //    canonical form regardless of dialect; we only re-encode it on the
    //    wire side for `FullBinary`.
    if !msg.mti.iter().all(|b| b.is_ascii_digit()) {
        return Err(BuildError::InvalidMti(msg.mti));
    }

    // 2. Build the bitmap from the field set. Pre-validate every field's
    //    payload against its spec definition BEFORE serialising so a bad
    //    field doesn't leave us with a half-written buffer.
    let mut bitmap = Bitmap8583::new();
    for (&n, data) in &msg.fields {
        // n == 0  : not a valid ISO 8583 field number.
        // n == 1  : secondary-bitmap indicator — auto-managed by the encoder;
        //           a caller-supplied value would set bit 0x80 AND emit 8 payload
        //           bytes, breaking the round-trip (encode() clears 0x80 when no
        //           field > 64 is present; re-parse then skips field 1 and
        //           mis-frames every subsequent field).
        // n > 128 : outside the range a 16-byte bitmap can address.
        if n == 0 || n == 1 || n > 128 {
            return Err(BuildError::InvalidFieldNumber(n));
        }
        let meta = spec.lookup(n).ok_or(BuildError::UnknownField(n))?;
        match meta.length {
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
        bitmap.set(u16::from(n))?;
    }

    // 3. Serialise.
    let bitmap_bytes = bitmap.encode();
    let bitmap_wire_len = match dialect {
        Dialect::HybridAscii | Dialect::FullBinary => bitmap_bytes.len(),
        Dialect::FullAscii => bitmap_bytes.len() * 2,
    };
    let mti_wire_len = match dialect {
        Dialect::HybridAscii | Dialect::FullAscii => 4,
        Dialect::FullBinary => 2,
    };
    let mut out: Vec<u8> =
        Vec::with_capacity(mti_wire_len + bitmap_wire_len + msg.fields.len() * 16);
    write_mti(&mut out, &msg.mti, dialect);
    match dialect {
        Dialect::HybridAscii | Dialect::FullBinary => out.extend_from_slice(&bitmap_bytes),
        Dialect::FullAscii => write_hex_uppercase(&mut out, &bitmap_bytes),
    }

    // BTreeMap iteration is ascending by key — matches ISO 8583 ordering.
    for (&n, data) in &msg.fields {
        let meta = match spec.lookup(n) {
            Some(m) => m,
            None => return Err(BuildError::UnknownField(n)),
        };
        match meta.length {
            LengthSpec::Fixed(_) => {}
            LengthSpec::LLVAR { .. } => write_var_len(&mut out, data.len(), 2, dialect),
            LengthSpec::LLLVAR { .. } => write_var_len(&mut out, data.len(), 3, dialect),
        }
        write_field_payload(&mut out, data, meta.data_type, dialect, n)?;
        // Sanity: emitted exactly `field_wire_bytes(...)` bytes for the
        // payload. If not, parse / build asymmetry would surface as a
        // round-trip failure — caught by the dialect tests.
        let _wire = field_wire_bytes(meta.data_type, data.len(), dialect);
    }

    Ok(out)
}

/// Append the MTI in the dialect's wire convention.
fn write_mti(out: &mut Vec<u8>, mti: &[u8; 4], dialect: Dialect) {
    match dialect {
        Dialect::HybridAscii | Dialect::FullAscii => out.extend_from_slice(mti),
        Dialect::FullBinary => {
            // MTI is always 4 ASCII digits in the message representation;
            // bcd::encode_bcd succeeds because we just validated above.
            //
            // SAFETY: encode_bcd returns None only if any digit byte is
            // non-numeric or the digit count exceeds capacity. We checked
            // `mti.iter().all(is_ascii_digit)` at the top of build_with and
            // 4 digits exactly fit in 2 bytes; thus encode_bcd is total here.
            if let Some(bcd_bytes) = bcd::encode_bcd(mti, 2) {
                out.extend_from_slice(&bcd_bytes);
            }
        }
    }
}

/// Append a variable-length length prefix in the dialect's convention.
///
/// `digits` is 2 for LLVAR, 3 for LLLVAR. Caller must have validated that
/// `len` fits in `digits` decimal digits.
fn write_var_len(out: &mut Vec<u8>, len: usize, digits: usize, dialect: Dialect) {
    match dialect {
        Dialect::HybridAscii | Dialect::FullAscii => write_ascii_len(out, len, digits),
        Dialect::FullBinary => write_bcd_len(out, len, digits),
    }
}

/// Append a `digits`-wide ASCII decimal length prefix.
fn write_ascii_len(out: &mut Vec<u8>, len: usize, digits: usize) {
    let mut buf = [b'0'; 4];
    let mut v = len;
    for i in (0..digits).rev() {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.extend_from_slice(&buf[..digits]);
}

/// Append a `digits`-wide right-justified BCD length prefix
/// (`digits.div_ceil(2)` bytes).
fn write_bcd_len(out: &mut Vec<u8>, len: usize, digits: usize) {
    let mut buf = [b'0'; 3];
    let mut v = len;
    for i in (0..digits).rev() {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let prefix_bytes = digits.div_ceil(2);
    if let Some(bcd_bytes) = bcd::encode_bcd(&buf[..digits], prefix_bytes) {
        out.extend_from_slice(&bcd_bytes);
    }
}

/// Append `bytes` to `out` as uppercase ASCII hex.
fn write_hex_uppercase(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize]);
        out.push(HEX[(b & 0x0f) as usize]);
    }
}

/// Append a field's payload bytes, BCD-packing Numeric data in
/// `FullBinary` and passing through every other combination verbatim.
fn write_field_payload(
    out: &mut Vec<u8>,
    data: &[u8],
    data_type: DataType,
    dialect: Dialect,
    field: u8,
) -> Result<(), BuildError> {
    match (dialect, data_type) {
        (Dialect::FullBinary, DataType::Numeric) => {
            let wire_bytes = data.len().div_ceil(2);
            let encoded = bcd::encode_bcd(data, wire_bytes).ok_or_else(|| {
                let bad = data
                    .iter()
                    .copied()
                    .find(|b| !b.is_ascii_digit())
                    .unwrap_or(0);
                BuildError::InvalidBcdDigit { field, byte: bad }
            })?;
            out.extend_from_slice(&encoded);
            Ok(())
        }
        _ => {
            out.extend_from_slice(data);
            Ok(())
        }
    }
}
