//! ISO 8583 BitMap encoder / decoder.
//!
//! The primary bitmap (8 bytes) covers data fields 1..=64. The secondary
//! bitmap (8 bytes) covers fields 65..=128 and is only emitted when at
//! least one field in that range is set. Bit 1 of the primary bitmap is
//! reserved as the "secondary bitmap present" indicator and is managed
//! automatically by [`Bitmap8583::encode`].
//!
//! Bit layout follows the ISO 8583 convention: field `N` lives at
//! byte `(N-1)/8`, bit `7 - ((N-1) % 8)` (i.e. field 1 is the MSB of
//! byte 0).

use core::fmt;

pub const MAX_FIELDS: u16 = 128;
pub const PRIMARY_LEN: usize = 8;
pub const SECONDARY_LEN: usize = 8;
pub const TOTAL_LEN: usize = PRIMARY_LEN + SECONDARY_LEN;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BitmapError {
    /// Field index was 0 or > 128.
    FieldOutOfRange(u16),
    /// Decoder ran out of input.
    InsufficientBytes { got: usize, need: usize },
}

impl fmt::Display for BitmapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BitmapError::FieldOutOfRange(n) => {
                write!(f, "field {} out of range; valid range is 1..=128", n)
            }
            BitmapError::InsufficientBytes { got, need } => {
                write!(f, "insufficient bytes: got {}, need {}", got, need)
            }
        }
    }
}

impl std::error::Error for BitmapError {}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bitmap8583 {
    bytes: [u8; TOTAL_LEN],
}

impl Bitmap8583 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, field: u16) -> Result<(), BitmapError> {
        let (idx, mask) = Self::position(field)?;
        self.bytes[idx] |= mask;
        Ok(())
    }

    pub fn unset(&mut self, field: u16) -> Result<(), BitmapError> {
        let (idx, mask) = Self::position(field)?;
        self.bytes[idx] &= !mask;
        Ok(())
    }

    pub fn is_set(&self, field: u16) -> Result<bool, BitmapError> {
        let (idx, mask) = Self::position(field)?;
        Ok(self.bytes[idx] & mask != 0)
    }

    /// True when any field in 65..=128 is set.
    pub fn has_secondary(&self) -> bool {
        self.bytes[PRIMARY_LEN..].iter().any(|&b| b != 0)
    }

    /// Emit 8 bytes if only primary fields are set, else 16 bytes with
    /// bit 1 forced on (the secondary-bitmap-present indicator).
    pub fn encode(&self) -> Vec<u8> {
        if self.has_secondary() {
            let mut out = self.bytes.to_vec();
            out[0] |= 0x80;
            out
        } else {
            let mut out = self.bytes[..PRIMARY_LEN].to_vec();
            out[0] &= !0x80;
            out
        }
    }

    /// Decode 8 or 16 bytes. The required length is determined by bit 1
    /// of the first byte. The secondary indicator is normalised out of
    /// the stored form so callers compare by data fields, not by the
    /// auto-managed indicator bit.
    pub fn decode(input: &[u8]) -> Result<Self, BitmapError> {
        if input.len() < PRIMARY_LEN {
            return Err(BitmapError::InsufficientBytes {
                got: input.len(),
                need: PRIMARY_LEN,
            });
        }
        let need = if input[0] & 0x80 != 0 {
            TOTAL_LEN
        } else {
            PRIMARY_LEN
        };
        if input.len() < need {
            return Err(BitmapError::InsufficientBytes {
                got: input.len(),
                need,
            });
        }
        let mut bytes = [0u8; TOTAL_LEN];
        bytes[..need].copy_from_slice(&input[..need]);
        bytes[0] &= !0x80;
        Ok(Self { bytes })
    }

    /// Yield the data field numbers that are set, skipping field 1
    /// (which is the secondary indicator, not a data field).
    pub fn iter_set_fields(&self) -> impl Iterator<Item = u16> + '_ {
        let limit = if self.has_secondary() { MAX_FIELDS } else { 64 };
        (2..=limit).filter(move |f| matches!(self.is_set(*f), Ok(true)))
    }

    fn position(field: u16) -> Result<(usize, u8), BitmapError> {
        // Field 1 is the secondary-bitmap-present indicator, managed
        // exclusively by `encode`/`decode` — it is not addressable as a
        // data field. Accepting `set(1)` would store a bit that
        // `encode`/`decode` then clear (see `out[0] &= !0x80`), so the
        // value would round-trip to `false` with no error. Reject it
        // here so the silent-loss path is impossible.
        if field == 0 || field == 1 || field > MAX_FIELDS {
            return Err(BitmapError::FieldOutOfRange(field));
        }
        let zero_based = (field - 1) as usize;
        let idx = zero_based / 8;
        let bit = 7 - (zero_based % 8);
        Ok((idx, 1u8 << bit))
    }
}
