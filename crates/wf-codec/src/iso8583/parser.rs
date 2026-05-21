//! ISO 8583 message parser.
//!
//! Wire formats supported (see [`Dialect`] for the cheat sheet):
//!
//! - `HybridAscii`: `[MTI 4B ASCII] [Bitmap 8 or 16B BINARY] [Field data ASCII]`
//! - `FullAscii`:   `[MTI 4B ASCII] [Bitmap 16 or 32B ASCII hex] [Field data ASCII]`
//! - `FullBinary`:  `[MTI 2B BCD]  [Bitmap 8 or 16B BINARY] [BCD length + BCD/Alpha data]`
//!
//! Field data is held in [`Iso8583Message::fields`] as the **decoded ASCII**
//! payload regardless of dialect — Numeric / Track payloads in `FullBinary`
//! are BCD-unpacked at parse time so downstream callers see one canonical
//! representation. Non-numeric data types (Alpha, AlphaNumericSpecial,
//! Binary, …) are stored verbatim in every dialect.

use crate::iso8583::bcd;
use crate::iso8583::dialect::Dialect;
use crate::iso8583::field::{field_def, DataType, FieldDef, LengthSpec};
use core::fmt;
use std::collections::BTreeMap;
use wf_bitmap::{Bitmap8583, BitmapError, PRIMARY_LEN, TOTAL_LEN};

/// Logical ISO 8583 message: MTI + sparse field map.
///
/// `fields` maps field number → decoded payload bytes. For Numeric / Track
/// data the payload is the digit-by-digit ASCII representation regardless
/// of dialect; for LLVAR / LLLVAR fields the stored value does **not**
/// include the length prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Iso8583Message {
    pub mti: [u8; 4],
    pub fields: BTreeMap<u8, Vec<u8>>,
}

/// Failure modes for [`parse`] / [`parse_with`] / [`parse_any`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input ended before expected. `need` is how many more bytes were
    /// expected at `offset`.
    InsufficientBytes { offset: usize, need: usize },
    /// MTI bytes were not all ASCII digits `'0'..='9'`.
    InvalidMti([u8; 4]),
    /// Bitmap parse failed (propagated from `wf_bitmap`).
    BitmapError(BitmapError),
    /// Bitmap set a field number that has no [`FieldDef`](super::field::FieldDef).
    /// With the current table this only happens for field 0 (which the
    /// bitmap never sets) — included for completeness.
    UnknownField(u8),
    /// LLVAR / LLLVAR length prefix bytes were not ASCII digits.
    InvalidLengthPrefix { field: u8, bytes: Vec<u8> },
    /// LLVAR / LLLVAR decoded length exceeded the field's spec max.
    LengthExceedsMax {
        field: u8,
        decoded: usize,
        max: usize,
    },
    /// Input had trailing bytes after the last field — the strict parser
    /// rejects this so that parse and build are exact inverses.
    TrailingBytes { remaining: usize },
    /// `FullAscii` bitmap contained a non-hex character.
    InvalidBitmapHex { offset: usize, byte: u8 },
    /// `FullBinary` encountered a BCD byte with a nibble outside `0..=9`.
    /// Reports the byte position relative to the input start and the
    /// offending byte value.
    InvalidBcdNibble { offset: usize, byte: u8 },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InsufficientBytes { offset, need } => write!(
                f,
                "insufficient bytes at offset {}: need {} more",
                offset, need
            ),
            ParseError::InvalidMti(b) => {
                write!(f, "invalid MTI {:?}: not all ASCII digits", b)
            }
            ParseError::BitmapError(e) => write!(f, "bitmap parse failed: {}", e),
            ParseError::UnknownField(n) => {
                write!(f, "bitmap set field {} which has no FieldDef", n)
            }
            ParseError::InvalidLengthPrefix { field, bytes } => write!(
                f,
                "field {}: length prefix {:?} is not all ASCII digits",
                field, bytes
            ),
            ParseError::LengthExceedsMax {
                field,
                decoded,
                max,
            } => write!(
                f,
                "field {}: decoded length {} exceeds spec max {}",
                field, decoded, max
            ),
            ParseError::TrailingBytes { remaining } => {
                write!(
                    f,
                    "trailing bytes after last field: {} byte(s) left",
                    remaining
                )
            }
            ParseError::InvalidBitmapHex { offset, byte } => write!(
                f,
                "FullAscii bitmap: non-hex byte {:#x} at offset {}",
                byte, offset
            ),
            ParseError::InvalidBcdNibble { offset, byte } => write!(
                f,
                "FullBinary: byte {:#x} at offset {} has a nibble > 9",
                byte, offset
            ),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<BitmapError> for ParseError {
    fn from(err: BitmapError) -> Self {
        ParseError::BitmapError(err)
    }
}

/// Parse a complete ISO 8583 message, auto-detecting the dialect.
///
/// Tries each dialect in [`Dialect::ALL`] order and returns the first that
/// fully consumes the input. Existing `HybridAscii` callers see no
/// behaviour change — `HybridAscii` is tried first and succeeds on every
/// vector that historically worked.
pub fn parse(input: &[u8]) -> Result<Iso8583Message, ParseError> {
    parse_any(input).map(|(msg, _)| msg)
}

/// Like [`parse`] but also returns which dialect won. Callers that need to
/// round-trip back to the wire (e.g. the sanitize tool) use this so they
/// can hand the same dialect to [`super::build_with`].
///
/// On failure, returns the error from the **first** dialect tried (currently
/// [`Dialect::HybridAscii`]). This preserves backward compatibility: every
/// historical caller that pattern-matched on `ParseError::TrailingBytes` /
/// `ParseError::InvalidMti` etc. against the old single-dialect `parse`
/// continues to see those exact variants for the same bad inputs.
pub fn parse_any(input: &[u8]) -> Result<(Iso8583Message, Dialect), ParseError> {
    let mut first_err: Option<ParseError> = None;
    for &dialect in Dialect::ALL {
        match parse_with(input, dialect) {
            Ok(msg) => return Ok((msg, dialect)),
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    Err(first_err.unwrap_or(ParseError::InsufficientBytes { offset: 0, need: 0 }))
}

/// Parse using a specific dialect. Use this when you already know the source
/// system's wire convention (e.g. tests, audited captures).
pub fn parse_with(input: &[u8], dialect: Dialect) -> Result<Iso8583Message, ParseError> {
    // 1. MTI — width and encoding both depend on dialect.
    let (mti, mti_bytes) = read_mti(input, dialect)?;
    let mut offset = mti_bytes;

    // 2. Bitmap: dialect-specific.
    let (bitmap, bitmap_bytes) = read_bitmap(input, offset, dialect)?;
    offset += bitmap_bytes;

    // 3. Walk set fields in ascending order. iter_set_fields skips field 1
    //    (the secondary-bitmap indicator).
    let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for field_u16 in bitmap.iter_set_fields() {
        let n: u8 = u8::try_from(field_u16).map_err(|_| ParseError::UnknownField(0))?;
        let def = field_def(n).ok_or(ParseError::UnknownField(n))?;

        let (logical_len, prefix_len) = read_length(input, offset, dialect, def)?;
        offset += prefix_len;

        let wire_len = field_wire_bytes(def.data_type, logical_len, dialect);
        let data_end = offset
            .checked_add(wire_len)
            .ok_or(ParseError::InsufficientBytes {
                offset,
                need: wire_len,
            })?;
        if data_end > input.len() {
            return Err(ParseError::InsufficientBytes {
                offset,
                need: data_end - input.len(),
            });
        }
        let raw = &input[offset..data_end];
        let payload = decode_field_payload(raw, def.data_type, logical_len, dialect, offset)?;
        fields.insert(n, payload);
        offset = data_end;
    }

    // 4. Strict tail check — keeps parse and build exact inverses.
    if offset != input.len() {
        return Err(ParseError::TrailingBytes {
            remaining: input.len() - offset,
        });
    }

    Ok(Iso8583Message { mti, fields })
}

/// Decode the MTI in the dialect's convention. Returns `(mti_ascii, consumed)`
/// where `mti_ascii` is the 4 ASCII digit bytes representation regardless of
/// dialect, and `consumed` is the number of wire bytes the MTI occupied.
fn read_mti(input: &[u8], dialect: Dialect) -> Result<([u8; 4], usize), ParseError> {
    match dialect {
        Dialect::HybridAscii | Dialect::FullAscii => {
            if input.len() < 4 {
                return Err(ParseError::InsufficientBytes {
                    offset: 0,
                    need: 4 - input.len(),
                });
            }
            let mut mti = [0u8; 4];
            mti.copy_from_slice(&input[..4]);
            if !mti.iter().all(|b| b.is_ascii_digit()) {
                return Err(ParseError::InvalidMti(mti));
            }
            Ok((mti, 4))
        }
        Dialect::FullBinary => {
            if input.len() < 2 {
                return Err(ParseError::InsufficientBytes {
                    offset: 0,
                    need: 2 - input.len(),
                });
            }
            let decoded = bcd::decode_bcd(&input[..2], 4).ok_or_else(|| {
                // Find which byte's nibble is invalid for a precise error.
                for (i, &b) in input[..2].iter().enumerate() {
                    if (b >> 4) > 9 || (b & 0x0f) > 9 {
                        return ParseError::InvalidBcdNibble { offset: i, byte: b };
                    }
                }
                // Fallback: shouldn't be reachable since decode_bcd only
                // fails on nibble validity for a 2-byte / 4-digit request.
                ParseError::InvalidBcdNibble {
                    offset: 0,
                    byte: input[0],
                }
            })?;
            let mut mti = [0u8; 4];
            mti.copy_from_slice(&decoded);
            Ok((mti, 2))
        }
    }
}

/// Read a length prefix (Fixed / LLVAR / LLLVAR) and return
/// `(decoded_logical_length, prefix_bytes_consumed)`.
///
/// `decoded_logical_length` is always the digit / char count of the data,
/// not the wire byte count — i.e. it matches `FieldDef::length` semantics
/// across every dialect. For `Fixed(N)`, no bytes are consumed.
fn read_length(
    input: &[u8],
    offset: usize,
    dialect: Dialect,
    def: &FieldDef,
) -> Result<(usize, usize), ParseError> {
    match def.length {
        LengthSpec::Fixed(n) => Ok((n, 0)),
        LengthSpec::LLVAR { max } => {
            let (l, prefix) = read_var_len(input, offset, dialect, 2, def.number)?;
            if l > max {
                return Err(ParseError::LengthExceedsMax {
                    field: def.number,
                    decoded: l,
                    max,
                });
            }
            Ok((l, prefix))
        }
        LengthSpec::LLLVAR { max } => {
            let (l, prefix) = read_var_len(input, offset, dialect, 3, def.number)?;
            if l > max {
                return Err(ParseError::LengthExceedsMax {
                    field: def.number,
                    decoded: l,
                    max,
                });
            }
            Ok((l, prefix))
        }
    }
}

/// Decode the bitmap that starts at `offset` according to `dialect`. Returns
/// `(bitmap, consumed_bytes)`.
fn read_bitmap(
    input: &[u8],
    offset: usize,
    dialect: Dialect,
) -> Result<(Bitmap8583, usize), ParseError> {
    match dialect {
        Dialect::HybridAscii | Dialect::FullBinary => read_bitmap_hybrid(input, offset),
        Dialect::FullAscii => read_bitmap_full_ascii(input, offset),
    }
}

/// 8 or 16 raw binary bytes. Shared by `HybridAscii` and `FullBinary` — the
/// only difference between those two is the MTI and field-payload encoding,
/// not the bitmap.
fn read_bitmap_hybrid(input: &[u8], offset: usize) -> Result<(Bitmap8583, usize), ParseError> {
    if input.len() < offset + PRIMARY_LEN {
        return Err(ParseError::InsufficientBytes {
            offset,
            need: PRIMARY_LEN - (input.len() - offset),
        });
    }
    let bitmap_len = if input[offset] & 0x80 != 0 {
        TOTAL_LEN
    } else {
        PRIMARY_LEN
    };
    if input.len() < offset + bitmap_len {
        return Err(ParseError::InsufficientBytes {
            offset,
            need: bitmap_len - (input.len() - offset),
        });
    }
    let bitmap = Bitmap8583::decode(&input[offset..offset + bitmap_len])?;
    Ok((bitmap, bitmap_len))
}

/// `FullAscii`: the bitmap is 16 or 32 ASCII hex characters (`'0'..='9'`,
/// `'A'..='F'`, `'a'..='f'`). We decode the first two hex chars first to
/// discover whether a secondary bitmap is present (high bit of the first
/// decoded byte), then read enough more characters to cover it.
fn read_bitmap_full_ascii(input: &[u8], offset: usize) -> Result<(Bitmap8583, usize), ParseError> {
    // 16 hex chars represent the primary 8-byte bitmap.
    const PRIMARY_HEX_CHARS: usize = PRIMARY_LEN * 2;
    const TOTAL_HEX_CHARS: usize = TOTAL_LEN * 2;

    if input.len() < offset + PRIMARY_HEX_CHARS {
        return Err(ParseError::InsufficientBytes {
            offset,
            need: PRIMARY_HEX_CHARS - (input.len() - offset),
        });
    }
    let first_byte = decode_hex_byte(input, offset)?;
    let hex_chars_needed = if first_byte & 0x80 != 0 {
        TOTAL_HEX_CHARS
    } else {
        PRIMARY_HEX_CHARS
    };
    if input.len() < offset + hex_chars_needed {
        return Err(ParseError::InsufficientBytes {
            offset,
            need: hex_chars_needed - (input.len() - offset),
        });
    }
    let mut decoded = Vec::with_capacity(hex_chars_needed / 2);
    let mut i = 0;
    while i < hex_chars_needed {
        decoded.push(decode_hex_byte(input, offset + i)?);
        i += 2;
    }
    let bitmap = Bitmap8583::decode(&decoded)?;
    Ok((bitmap, hex_chars_needed))
}

/// Decode the two hex chars at `input[at..at+2]` into a byte.
fn decode_hex_byte(input: &[u8], at: usize) -> Result<u8, ParseError> {
    let hi = decode_hex_nibble(input[at]).ok_or(ParseError::InvalidBitmapHex {
        offset: at,
        byte: input[at],
    })?;
    let lo = decode_hex_nibble(input[at + 1]).ok_or(ParseError::InvalidBitmapHex {
        offset: at + 1,
        byte: input[at + 1],
    })?;
    Ok((hi << 4) | lo)
}

fn decode_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Read a variable-length length prefix in the dialect's convention.
///
/// `digits` is the digit-count width of the logical length (2 for LLVAR,
/// 3 for LLLVAR). Returns `(decoded_length, prefix_bytes_consumed)` where
/// `prefix_bytes_consumed` is `digits` for the ASCII dialects and
/// `digits.div_ceil(2)` for `FullBinary`.
fn read_var_len(
    input: &[u8],
    offset: usize,
    dialect: Dialect,
    digits: usize,
    field: u8,
) -> Result<(usize, usize), ParseError> {
    match dialect {
        Dialect::HybridAscii | Dialect::FullAscii => {
            let l = read_var_len_ascii(input, offset, digits, field)?;
            Ok((l, digits))
        }
        Dialect::FullBinary => {
            let prefix_bytes = digits.div_ceil(2);
            let end = offset
                .checked_add(prefix_bytes)
                .ok_or(ParseError::InsufficientBytes {
                    offset,
                    need: prefix_bytes,
                })?;
            if end > input.len() {
                return Err(ParseError::InsufficientBytes {
                    offset,
                    need: end - input.len(),
                });
            }
            let bytes = &input[offset..end];
            let decoded = bcd::decode_bcd(bytes, digits).ok_or_else(|| {
                // Map decode failure into the most useful error variant.
                for (i, &b) in bytes.iter().enumerate() {
                    if (b >> 4) > 9 || (b & 0x0f) > 9 {
                        return ParseError::InvalidBcdNibble {
                            offset: offset + i,
                            byte: b,
                        };
                    }
                }
                // Nibbles all valid but pad nibble was non-zero: surface as
                // a length-prefix-decode error so the caller sees a per-field
                // message rather than a generic "invalid BCD".
                ParseError::InvalidLengthPrefix {
                    field,
                    bytes: bytes.to_vec(),
                }
            })?;
            let mut value = 0usize;
            for &b in &decoded {
                value = value * 10 + (b - b'0') as usize;
            }
            Ok((value, prefix_bytes))
        }
    }
}

/// Read a `digits`-wide ASCII length prefix at `offset` and return the
/// decoded numeric value. `field` is only used for error reporting.
fn read_var_len_ascii(
    input: &[u8],
    offset: usize,
    digits: usize,
    field: u8,
) -> Result<usize, ParseError> {
    let end = offset
        .checked_add(digits)
        .ok_or(ParseError::InsufficientBytes {
            offset,
            need: digits,
        })?;
    if end > input.len() {
        return Err(ParseError::InsufficientBytes {
            offset,
            need: end - input.len(),
        });
    }
    let bytes = &input[offset..end];
    if !bytes.iter().all(|b| b.is_ascii_digit()) {
        return Err(ParseError::InvalidLengthPrefix {
            field,
            bytes: bytes.to_vec(),
        });
    }
    let mut value = 0usize;
    for &b in bytes {
        value = value * 10 + (b - b'0') as usize;
    }
    Ok(value)
}

/// Number of wire bytes a field with `logical_len` chars/digits occupies in
/// `dialect`. For BCD-packed Numeric data in `FullBinary`, this is
/// `logical_len.div_ceil(2)`; everywhere else it equals `logical_len`.
pub(crate) fn field_wire_bytes(data_type: DataType, logical_len: usize, dialect: Dialect) -> usize {
    match (dialect, data_type) {
        (Dialect::FullBinary, DataType::Numeric) => logical_len.div_ceil(2),
        _ => logical_len,
    }
}

/// Convert raw wire bytes to the canonical [`Iso8583Message::fields`]
/// representation. Numeric data in `FullBinary` is BCD-decoded to ASCII
/// digit bytes; every other combination is a pass-through.
fn decode_field_payload(
    raw: &[u8],
    data_type: DataType,
    logical_len: usize,
    dialect: Dialect,
    input_offset: usize,
) -> Result<Vec<u8>, ParseError> {
    match (dialect, data_type) {
        (Dialect::FullBinary, DataType::Numeric) => {
            bcd::decode_bcd(raw, logical_len).ok_or_else(|| {
                for (i, &b) in raw.iter().enumerate() {
                    if (b >> 4) > 9 || (b & 0x0f) > 9 {
                        return ParseError::InvalidBcdNibble {
                            offset: input_offset + i,
                            byte: b,
                        };
                    }
                }
                // Pad nibble non-zero — only possible when logical_len is
                // odd. Surface as the most informative variant available.
                ParseError::InvalidBcdNibble {
                    offset: input_offset,
                    byte: raw.first().copied().unwrap_or(0),
                }
            })
        }
        _ => Ok(raw.to_vec()),
    }
}
