//! BCD (Binary-Coded Decimal) helpers for the [`FullBinary`] ISO 8583
//! dialect.
//!
//! BCD packs two decimal digits per byte: the high nibble is the more
//! significant digit, the low nibble the less significant. Every nibble
//! must be in `0..=9` — `0xA..=0xF` are invalid and surfaced as errors
//! rather than silently mapped to a Latin character.
//!
//! These helpers are kept independent of the parser / builder so callers
//! that want to sanity-check raw BCD blobs (or upgrade the sanitizer to
//! redact BCD payloads in a later sprint) can `pub use` them directly.
//!
//! # Convention
//!
//! The helpers treat BCD as **right-justified** — i.e. if a digit count is
//! odd, the highest (left-most) nibble is the zero pad. For example:
//!
//! | digit string | encoded bytes (right-justified) |
//! |--------------|----------------------------------|
//! | `"0800"` (4) | `[0x08, 0x00]`                   |
//! | `"16"` (2)   | `[0x16]`                         |
//! | `"100"` (3)  | `[0x01, 0x00]`                   |
//! | `"9"` (1)    | `[0x09]`                         |
//!
//! This matches the jpos `IFB_LLNUM` / `IFB_LLLNUM` packagers and is the
//! convention every BCD-packed length prefix in this codec relies on.
//!
//! [`FullBinary`]: super::dialect::Dialect::FullBinary
//!
//! Each function is intentionally small (≤30 lines) so a reviewer can audit
//! the nibble math at a glance.

/// Encode ASCII decimal digit bytes as right-justified BCD into exactly
/// `out_bytes` bytes.
///
/// If `digits.len() < out_bytes * 2`, the highest nibbles are zero-padded
/// (left padding). Returns `None` if any byte in `digits` is not an ASCII
/// digit, or if `digits.len() > out_bytes * 2` (would not fit).
pub fn encode_bcd(digits: &[u8], out_bytes: usize) -> Option<Vec<u8>> {
    let total_nibbles = out_bytes * 2;
    if digits.len() > total_nibbles {
        return None;
    }
    let pad = total_nibbles - digits.len();
    let mut nibbles: Vec<u8> = Vec::with_capacity(total_nibbles);
    nibbles.extend(std::iter::repeat_n(0u8, pad));
    for &b in digits {
        if !b.is_ascii_digit() {
            return None;
        }
        nibbles.push(b - b'0');
    }
    let mut out = Vec::with_capacity(out_bytes);
    let mut i = 0;
    while i < nibbles.len() {
        out.push((nibbles[i] << 4) | nibbles[i + 1]);
        i += 2;
    }
    Some(out)
}

/// Decode right-justified BCD bytes into exactly `digits` ASCII decimal
/// digit bytes.
///
/// Returns `None` if any nibble is `> 9`, or if `bytes.len() * 2 < digits`
/// (not enough source nibbles to satisfy the request). Any leading padding
/// nibbles (when `digits` is odd) MUST be zero — a non-zero pad nibble is
/// rejected as `None` to preserve the round-trip property `encode ∘ decode = id`.
pub fn decode_bcd(bytes: &[u8], digits: usize) -> Option<Vec<u8>> {
    let total_nibbles = bytes.len() * 2;
    if total_nibbles < digits {
        return None;
    }
    let pad = total_nibbles - digits;
    let mut all_nibbles: Vec<u8> = Vec::with_capacity(total_nibbles);
    for &b in bytes {
        let hi = b >> 4;
        let lo = b & 0x0f;
        if hi > 9 || lo > 9 {
            return None;
        }
        all_nibbles.push(hi);
        all_nibbles.push(lo);
    }
    if all_nibbles.iter().take(pad).any(|&n| n != 0) {
        return None;
    }
    Some(all_nibbles[pad..].iter().map(|&n| b'0' + n).collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn encode_even_digits_no_pad() {
        assert_eq!(encode_bcd(b"0800", 2).unwrap(), vec![0x08, 0x00]);
        assert_eq!(encode_bcd(b"16", 1).unwrap(), vec![0x16]);
        assert_eq!(encode_bcd(b"99", 1).unwrap(), vec![0x99]);
    }

    #[test]
    fn encode_odd_digits_left_pads_with_zero_nibble() {
        assert_eq!(encode_bcd(b"100", 2).unwrap(), vec![0x01, 0x00]);
        assert_eq!(encode_bcd(b"9", 1).unwrap(), vec![0x09]);
        assert_eq!(encode_bcd(b"123", 2).unwrap(), vec![0x01, 0x23]);
    }

    #[test]
    fn encode_rejects_non_digit() {
        assert!(encode_bcd(b"1X", 1).is_none());
        assert!(encode_bcd(b"AB", 1).is_none());
    }

    #[test]
    fn encode_rejects_overflow() {
        // 3 digits don't fit in 1 byte (2 nibbles).
        assert!(encode_bcd(b"100", 1).is_none());
    }

    #[test]
    fn decode_even_digits() {
        assert_eq!(decode_bcd(&[0x08, 0x00], 4).unwrap(), b"0800");
        assert_eq!(decode_bcd(&[0x16], 2).unwrap(), b"16");
    }

    #[test]
    fn decode_odd_digits_with_leading_zero_pad() {
        assert_eq!(decode_bcd(&[0x01, 0x00], 3).unwrap(), b"100");
        assert_eq!(decode_bcd(&[0x09], 1).unwrap(), b"9");
    }

    #[test]
    fn decode_rejects_high_nibble() {
        // 0xA in a nibble is invalid BCD.
        assert!(decode_bcd(&[0xA0], 2).is_none());
        assert!(decode_bcd(&[0x0F], 2).is_none());
    }

    #[test]
    fn decode_rejects_nonzero_pad() {
        // 3 digits requested but byte 0 high nibble is non-zero pad.
        assert!(decode_bcd(&[0x10, 0x00], 3).is_none());
    }

    #[test]
    fn decode_rejects_insufficient_bytes() {
        // 4 digits requested but only 1 byte (2 nibbles) supplied.
        assert!(decode_bcd(&[0x12], 4).is_none());
    }

    #[test]
    fn round_trip_random_digits() {
        for s in [
            b"0".as_slice(),
            b"12",
            b"123",
            b"1234",
            b"00000",
            b"9999999999",
        ] {
            let out_bytes = s.len().div_ceil(2);
            let encoded = encode_bcd(s, out_bytes).expect("encodes");
            let decoded = decode_bcd(&encoded, s.len()).expect("decodes");
            assert_eq!(
                decoded,
                s.to_vec(),
                "round-trip {:?}",
                std::str::from_utf8(s)
            );
        }
    }
}
