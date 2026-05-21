//! ISO 8583 message parser.
//!
//! Wire formats supported (see [`Dialect`] for the cheat sheet):
//!
//! - `HybridAscii`: `[MTI 4B ASCII] [Bitmap 8 or 16B BINARY] [Field data ASCII]`
//! - `FullAscii`:   `[MTI 4B ASCII] [Bitmap 16 or 32B ASCII hex] [Field data ASCII]`
//!
//! Field data is held as raw bytes; semantic interpretation (numeric vs
//! alpha vs binary) is deferred to a later sprint. LLVAR / LLLVAR length
//! prefixes are ASCII digit characters in BOTH dialects — the only axis
//! of variation between the two is the bitmap encoding.

use crate::iso8583::dialect::Dialect;
use crate::iso8583::field::{field_def, LengthSpec};
use core::fmt;
use std::collections::BTreeMap;
use wf_bitmap::{Bitmap8583, BitmapError, PRIMARY_LEN, TOTAL_LEN};

/// Logical ISO 8583 message: MTI + sparse field map.
///
/// `fields` maps field number → raw payload bytes. For LLVAR / LLLVAR
/// fields the stored value does **not** include the length prefix — the
/// prefix is re-derived on encode.
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
    // 1. MTI is the same in every supported dialect: 4 ASCII digit bytes.
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
    let mut offset = 4usize;

    // 2. Bitmap: dialect-specific. Returns the parsed bitmap and the number
    //    of input bytes it consumed (8/16 for HybridAscii, 16/32 for FullAscii).
    let (bitmap, bitmap_bytes) = read_bitmap(input, offset, dialect)?;
    offset += bitmap_bytes;

    // 3. Walk set fields in ascending order — identical for every dialect
    //    because LLVAR / LLLVAR length prefixes and field bytes are ASCII in
    //    both. iter_set_fields already skips field 1 (secondary indicator).
    let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for field_u16 in bitmap.iter_set_fields() {
        // SPEC: bitmap only sets 1..=128, so this fits in u8 (128 fits).
        let n: u8 = match u8::try_from(field_u16) {
            Ok(v) => v,
            Err(_) => return Err(ParseError::UnknownField(0)),
        };
        let def = field_def(n).ok_or(ParseError::UnknownField(n))?;

        let (data_len, prefix_len) = match def.length {
            LengthSpec::Fixed(n_bytes) => (n_bytes, 0usize),
            LengthSpec::LLVAR { max } => {
                let l = read_var_len(input, offset, 2, n)?;
                if l > max {
                    return Err(ParseError::LengthExceedsMax {
                        field: n,
                        decoded: l,
                        max,
                    });
                }
                (l, 2)
            }
            LengthSpec::LLLVAR { max } => {
                let l = read_var_len(input, offset, 3, n)?;
                if l > max {
                    return Err(ParseError::LengthExceedsMax {
                        field: n,
                        decoded: l,
                        max,
                    });
                }
                (l, 3)
            }
        };
        offset += prefix_len;
        let data_end = offset
            .checked_add(data_len)
            .ok_or(ParseError::InsufficientBytes {
                offset,
                need: data_len,
            })?;
        if data_end > input.len() {
            return Err(ParseError::InsufficientBytes {
                offset,
                need: data_end - input.len(),
            });
        }
        fields.insert(n, input[offset..data_end].to_vec());
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

/// Decode the bitmap that starts at `offset` according to `dialect`. Returns
/// `(bitmap, consumed_bytes)` where `consumed_bytes` is how many bytes of the
/// input the bitmap occupied (8 or 16 for `HybridAscii`, 16 or 32 for
/// `FullAscii`).
fn read_bitmap(
    input: &[u8],
    offset: usize,
    dialect: Dialect,
) -> Result<(Bitmap8583, usize), ParseError> {
    match dialect {
        Dialect::HybridAscii => read_bitmap_hybrid(input, offset),
        Dialect::FullAscii => read_bitmap_full_ascii(input, offset),
    }
}

/// `HybridAscii`: the bitmap is 8 or 16 raw binary bytes. The high bit of the
/// first byte tells us whether a secondary bitmap follows. We probe that bit
/// on the raw input byte BEFORE delegating to `wf_bitmap::decode`, because
/// `decode` normalises that bit out of the stored form.
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
    // Decode the first byte to learn whether a secondary bitmap follows.
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
    // Decode the full hex run into a fresh byte buffer, then hand that buffer
    // to `wf_bitmap::decode` exactly as the `HybridAscii` path would.
    let mut decoded = Vec::with_capacity(hex_chars_needed / 2);
    let mut i = 0;
    while i < hex_chars_needed {
        decoded.push(decode_hex_byte(input, offset + i)?);
        i += 2;
    }
    let bitmap = Bitmap8583::decode(&decoded)?;
    Ok((bitmap, hex_chars_needed))
}

/// Decode the two hex chars at `input[at..at+2]` into a byte. Hex is case
/// insensitive. Reports the offending byte position on failure so error
/// messages can pin-point the malformed character without scanning twice.
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

/// Read a `digits`-wide ASCII length prefix at `offset` and return the
/// decoded numeric value. `field` is only used for error reporting.
fn read_var_len(
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
    // ASCII digit → numeric. Safe because we just validated all bytes.
    let mut value = 0usize;
    for &b in bytes {
        value = value * 10 + (b - b'0') as usize;
    }
    Ok(value)
}
