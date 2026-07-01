//! SM3 (GM/T 0004-2012, GB/T 32905-2016) cryptographic hash.
//!
//! SM3 is a 256-bit Merkle-Damgård hash with a 64-byte block size,
//! designed for use within the China GM/T cryptographic suite. Output
//! width and structural properties closely mirror SHA-256, but the
//! compression function uses GM-specific constants and rotations.
//!
//! This module is a thin, fixed-shape wrapper around RustCrypto's
//! [`sm3`](https://docs.rs/sm3) implementation (the `Digest` trait). The
//! wrapper exists to:
//!
//! - Pin a 32-byte fixed-size return type (`[u8; 32]`) so callers can
//!   put SM3 digests into `Copy` types and stack-allocate without a
//!   `Vec` heap hop.
//! - Offer a streaming [`Sm3`] hasher that holds the upstream's
//!   incremental block state, so large inputs (file scans, WAL records)
//!   never need the whole payload buffered in memory — satisfying the
//!   "有界一切" constraint that the previous `smcrypto`-backed,
//!   full-buffer wrapper violated.
//! - Provide a `sm3_hex` convenience for log lines / debug output where
//!   the canonical lowercase-hex form is the expected representation.
//!
//! # Standard test vectors
//!
//! Tests in this module are sourced from GB/T 32905-2016 § A.1 and
//! § A.2 (the two canonical vectors every SM3 implementation publishes),
//! plus the 64-byte (16×"abcd") single-block-boundary case. Per the
//! project's test-independence policy, the expected digests are written
//! longhand and not regenerated from our own [`sm3`] output. They are also the safety net for the
//! 2026-05-29 backend swap from `smcrypto` to RustCrypto `sm3`: an
//! incorrect migration would diverge from these spec-defined values.

use ::sm3::{Digest, Sm3 as Sm3Backend};
use core::fmt;

/// Length of the SM3 digest in bytes.
pub const SM3_DIGEST_LEN: usize = 32;

/// Lowercase-hex alphabet for [`to_hex`].
const HEX_LOWER: &[u8; 16] = b"0123456789abcdef";

/// One-shot SM3 hash. Returns the 32-byte digest of `input`.
pub fn sm3(input: &[u8]) -> [u8; SM3_DIGEST_LEN] {
    let mut hasher = Sm3Backend::new();
    hasher.update(input);
    finalize_to_array(hasher)
}

/// Convenience: SM3 hash as a 64-char lowercase hex string. Useful for
/// logging or comparing digests in tests without an extra `format!`
/// dance at the call site.
pub fn sm3_hex(input: &[u8]) -> String {
    to_hex(&sm3(input))
}

/// Streaming SM3 hasher. Construct with [`Sm3::new`], feed data via
/// [`Sm3::update`] in arbitrary chunk sizes, then call [`Sm3::finalize`]
/// once to obtain the 32-byte digest. Equivalent to calling [`sm3`] on
/// the concatenated input.
///
/// Unlike the previous implementation, this holds the upstream's
/// incremental hasher state (a 64-byte working block plus the chaining
/// value) — **not** a growable buffer of the entire input. Hashing a
/// gigabyte-scale WAL therefore costs O(1) memory.
#[derive(Clone)]
pub struct Sm3 {
    inner: Sm3Backend,
}

impl Sm3 {
    /// Start a fresh SM3 hasher.
    pub fn new() -> Self {
        Self {
            inner: Sm3Backend::new(),
        }
    }

    /// Feed `data` into the running hash. May be called any number of
    /// times with arbitrary chunk sizes.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Consume the hasher and return the 32-byte digest.
    pub fn finalize(self) -> [u8; SM3_DIGEST_LEN] {
        finalize_to_array(self.inner)
    }
}

impl Default for Sm3 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Sm3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Don't expose the internal chaining state; it carries no useful
        // information for callers and could leak partial-hash bytes.
        f.debug_struct("Sm3").finish_non_exhaustive()
    }
}

/// Drive the backend hasher to completion and copy its digest into a
/// fixed `[u8; 32]`. Works across `digest` major versions: the finalize
/// output always derefs to `[u8]`, and SM3's output width is fixed at
/// [`SM3_DIGEST_LEN`].
fn finalize_to_array(hasher: Sm3Backend) -> [u8; SM3_DIGEST_LEN] {
    let digest = hasher.finalize();
    let mut out = [0u8; SM3_DIGEST_LEN];
    out.copy_from_slice(&digest);
    out
}

/// Encode bytes as a lowercase hex string. Kept local so wf-sm stays
/// free of a `hex` dependency.
fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX_LOWER[(b >> 4) as usize] as char);
        out.push(HEX_LOWER[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// GB/T 32905-2016 § A.1: SM3("abc") = `66c7f0f4...8f4ba8e0`.
    ///
    /// The expected digest is reproduced longhand from the standard
    /// document so a regression in either the wrapper or the upstream
    /// crate would diverge from the spec-defined value, not from a
    /// value we generated ourselves.
    const VECTOR_ABC_HEX: &str = "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0";

    /// GB/T 32905-2016 § A.2: SM3 of 16 repeats of `"abcd"` (64 bytes,
    /// exactly one full block).
    const VECTOR_ABCD_X16_HEX: &str =
        "debe9ff92275b8a138604889c18e5a4d6fdb70e5387e5765293dcba39c0c5732";

    /// Decode a 64-character lowercase hex string into the 32-byte
    /// digest representation. Test-only helper used to compare the
    /// byte-array form against the longhand spec vectors.
    fn decode_hex_32(s: &str) -> [u8; SM3_DIGEST_LEN] {
        assert_eq!(s.len(), SM3_DIGEST_LEN * 2, "vector hex length");
        let mut out = [0u8; SM3_DIGEST_LEN];
        let bytes = s.as_bytes();
        for (i, slot) in out.iter_mut().enumerate() {
            let hi = hex_nibble(bytes[i * 2]);
            let lo = hex_nibble(bytes[i * 2 + 1]);
            *slot = (hi << 4) | lo;
        }
        out
    }

    fn hex_nibble(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("non-hex byte {b:#x} in test vector"),
        }
    }

    #[test]
    fn vector_abc() {
        let digest = sm3(b"abc");
        let hex = sm3_hex(b"abc");
        assert_eq!(hex, VECTOR_ABC_HEX);
        assert_eq!(digest.len(), SM3_DIGEST_LEN);
        // Cross-check: hex form must decode to the same bytes.
        assert_eq!(digest, decode_hex_32(VECTOR_ABC_HEX));
    }

    #[test]
    fn vector_abcd_x16() {
        let input: Vec<u8> = (0..16).flat_map(|_| b"abcd".iter().copied()).collect();
        assert_eq!(input.len(), 64);
        let digest = sm3(&input);
        assert_eq!(digest, decode_hex_32(VECTOR_ABCD_X16_HEX));
    }

    #[test]
    fn empty_input_is_well_defined() {
        // SM3 of the empty string is a fixed published value; we verify
        // the wrapper does not panic on empty input and emits a 32-byte
        // digest matching the well-known SM3("") constant.
        const VECTOR_EMPTY_HEX: &str =
            "1ab21d8355cfa17f8e61194831e81a8f22bec8c728fefb747ed035eb5082aa2b";
        let digest = sm3(b"");
        assert_eq!(digest.len(), SM3_DIGEST_LEN);
        assert_eq!(digest, decode_hex_32(VECTOR_EMPTY_HEX));
    }

    #[test]
    fn streaming_matches_oneshot() {
        let mut s = Sm3::new();
        s.update(b"ab");
        s.update(b"c");
        let streamed = s.finalize();
        assert_eq!(streamed, sm3(b"abc"));
    }

    #[test]
    fn streaming_handles_many_small_chunks() {
        let input: Vec<u8> = (0..16).flat_map(|_| b"abcd".iter().copied()).collect();
        let mut s = Sm3::new();
        for byte in &input {
            s.update(std::slice::from_ref(byte));
        }
        assert_eq!(s.finalize(), decode_hex_32(VECTOR_ABCD_X16_HEX));
    }

    #[test]
    fn streaming_across_block_boundary() {
        // Feed 200 bytes split at an awkward offset to exercise the
        // incremental block state (>3 full 64-byte blocks plus a tail).
        let input: Vec<u8> = (0..200u32).map(|i| (i % 251) as u8).collect();
        let oneshot = sm3(&input);
        let mut s = Sm3::new();
        s.update(&input[..37]);
        s.update(&input[37..128]);
        s.update(&input[128..]);
        assert_eq!(s.finalize(), oneshot);
    }

    #[test]
    fn sm3_hex_is_lowercase_64_chars() {
        let h = sm3_hex(b"abc");
        assert_eq!(h.len(), SM3_DIGEST_LEN * 2);
        assert!(h
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()));
    }
}
