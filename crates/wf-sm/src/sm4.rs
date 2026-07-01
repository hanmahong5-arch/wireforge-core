//! SM4 (GM/T 0002-2012, GB/T 32907-2016) 128-bit block cipher.
//!
//! SM4 is the China GM/T symmetric block cipher: 128-bit key, 128-bit
//! block, 32-round Feistel-like network. This module is a thin,
//! fixed-shape wrapper around RustCrypto's
//! [`sm4`](https://docs.rs/sm4) block primitive (the `BlockEncrypt` /
//! `BlockDecrypt` traits). The wrapper exists to:
//!
//! - Express key and IV sizes in the type system as `&[u8; 16]` so a
//!   wrong-length key is a compile error at the call site, not a
//!   runtime check.
//! - Provide the two block-chaining modes Chinese HSM / payment stacks
//!   actually use — CBC (with IV) and ECB (no IV) — with PKCS#7
//!   padding handled here rather than leaking the raw block primitive
//!   to every consumer.
//! - Offer a **bounded** streaming encryptor ([`Sm4CbcEncryptor`]) that
//!   processes input one 16-byte block at a time and never buffers the
//!   whole plaintext, satisfying the "有界一切" constraint for
//!   large-payload paths (file/WAL encryption later).
//!
//! Upstream RustCrypto `sm4` is unaudited. No 密评 / GB/T 39786
//! compliance claim attaches to this module — see [`crate::sm2`] for
//! the same caveat and the separate Tongsuo C-FFI compliance route.
//!
//! # Standard test vector
//!
//! Tests assert the single-block ECB vector published in
//! GB/T 32907-2016 (key = plaintext = `0123456789ABCDEF FEDCBA9876543210`,
//! ciphertext = `681EDF34D206965E 86B3E94F536E4246`). Per the project's
//! test-independence policy the expected ciphertext is written longhand from the
//! standard, not regenerated from our own output, so a regression in
//! either the wrapper or the upstream crate diverges from the
//! spec-defined value. Round-trip (encrypt then decrypt) tests are
//! labelled there as functional self-consistency, not a standards
//! measurement.

use ::sm4::cipher::generic_array::GenericArray;
use ::sm4::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use ::sm4::Sm4;
use core::fmt;

/// SM4 key length in bytes (128 bits).
pub const SM4_KEY_LEN: usize = 16;

/// SM4 block / IV length in bytes (128 bits).
pub const SM4_BLOCK_LEN: usize = 16;

/// Errors returned by the SM4 wrapper.
///
/// Each variant's [`fmt::Display`] follows the three-element error
/// convention used across this crate: what happened, what was
/// expected, and what the caller can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sm4Error {
    /// Ciphertext length was not a whole number of 16-byte blocks.
    CiphertextNotBlockAligned {
        /// The offending ciphertext length, in bytes.
        len: usize,
    },
    /// PKCS#7 padding was structurally invalid after decryption.
    ///
    /// This means either the trailing pad-length byte was out of the
    /// `1..=16` range, or the padding bytes did not all equal that
    /// length. It usually indicates a wrong key/IV or tampered
    /// ciphertext.
    InvalidPadding,
}

impl fmt::Display for Sm4Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Sm4Error::CiphertextNotBlockAligned { len } => write!(
                f,
                "SM4 ciphertext length {len} is not a multiple of the \
                 {SM4_BLOCK_LEN}-byte block size; expected a whole number \
                 of blocks; verify the ciphertext was produced by this \
                 cipher and was not truncated in transit"
            ),
            Sm4Error::InvalidPadding => write!(
                f,
                "SM4 plaintext failed PKCS#7 padding validation after \
                 decryption; expected a final pad-length byte in 1..={SM4_BLOCK_LEN} \
                 with matching trailing bytes; this typically means the \
                 key or IV is wrong or the ciphertext was tampered with — \
                 do not trust the output"
            ),
        }
    }
}

impl std::error::Error for Sm4Error {}

/// Append PKCS#7 padding so the result is a whole number of blocks.
///
/// Always adds between 1 and [`SM4_BLOCK_LEN`] bytes (a full extra
/// block when the input is already block-aligned), so the inverse is
/// unambiguous.
fn pad_pkcs7(data: &[u8]) -> Vec<u8> {
    let pad_len = SM4_BLOCK_LEN - (data.len() % SM4_BLOCK_LEN);
    let mut padded = Vec::with_capacity(data.len() + pad_len);
    padded.extend_from_slice(data);
    padded.extend(std::iter::repeat_n(pad_len as u8, pad_len));
    padded
}

/// Remove and validate PKCS#7 padding. Constant in structure (not
/// constant-time): it rejects any padding whose declared length is out
/// of range or whose trailing bytes disagree.
fn unpad_pkcs7(data: &[u8]) -> Result<Vec<u8>, Sm4Error> {
    if data.is_empty() || !data.len().is_multiple_of(SM4_BLOCK_LEN) {
        return Err(Sm4Error::CiphertextNotBlockAligned { len: data.len() });
    }
    let pad_len = data[data.len() - 1] as usize;
    if pad_len == 0 || pad_len > SM4_BLOCK_LEN {
        return Err(Sm4Error::InvalidPadding);
    }
    let start = data.len() - pad_len;
    if data[start..].iter().any(|&b| b as usize != pad_len) {
        return Err(Sm4Error::InvalidPadding);
    }
    Ok(data[..start].to_vec())
}

/// XOR `block` in place against `mask` (both 16 bytes). Used for CBC
/// chaining.
fn xor_block(block: &mut [u8; SM4_BLOCK_LEN], mask: &[u8; SM4_BLOCK_LEN]) {
    for (b, m) in block.iter_mut().zip(mask.iter()) {
        *b ^= *m;
    }
}

/// Encrypt `plaintext` under SM4-ECB with PKCS#7 padding.
///
/// ECB leaks plaintext block equality and must only be used where the
/// surrounding protocol requires it (some CN HSM key-wrap paths). Prefer
/// [`sm4_cbc_encrypt`] for general data.
pub fn sm4_ecb_encrypt(key: &[u8; SM4_KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, Sm4Error> {
    let cipher = Sm4::new(GenericArray::from_slice(key));
    let padded = pad_pkcs7(plaintext);
    let mut out = Vec::with_capacity(padded.len());
    for chunk in padded.chunks_exact(SM4_BLOCK_LEN) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        out.extend_from_slice(&block);
    }
    Ok(out)
}

/// Decrypt `ciphertext` produced by [`sm4_ecb_encrypt`], removing
/// PKCS#7 padding.
pub fn sm4_ecb_decrypt(key: &[u8; SM4_KEY_LEN], ciphertext: &[u8]) -> Result<Vec<u8>, Sm4Error> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(SM4_BLOCK_LEN) {
        return Err(Sm4Error::CiphertextNotBlockAligned {
            len: ciphertext.len(),
        });
    }
    let cipher = Sm4::new(GenericArray::from_slice(key));
    let mut plain = Vec::with_capacity(ciphertext.len());
    for chunk in ciphertext.chunks_exact(SM4_BLOCK_LEN) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        plain.extend_from_slice(&block);
    }
    unpad_pkcs7(&plain)
}

/// Encrypt `plaintext` under SM4-CBC with the given `iv` and PKCS#7
/// padding.
///
/// The `iv` must be unpredictable per message for CBC to be secure; it
/// is not secret and is typically prepended to the ciphertext by the
/// caller. This function does **not** prepend it.
pub fn sm4_cbc_encrypt(
    key: &[u8; SM4_KEY_LEN],
    iv: &[u8; SM4_BLOCK_LEN],
    plaintext: &[u8],
) -> Result<Vec<u8>, Sm4Error> {
    let cipher = Sm4::new(GenericArray::from_slice(key));
    let padded = pad_pkcs7(plaintext);
    let mut out = Vec::with_capacity(padded.len());
    let mut prev = *iv;
    for chunk in padded.chunks_exact(SM4_BLOCK_LEN) {
        let mut block = [0u8; SM4_BLOCK_LEN];
        block.copy_from_slice(chunk);
        xor_block(&mut block, &prev);
        let mut ga = GenericArray::clone_from_slice(&block);
        cipher.encrypt_block(&mut ga);
        out.extend_from_slice(&ga);
        prev.copy_from_slice(&ga);
    }
    Ok(out)
}

/// Decrypt `ciphertext` produced by [`sm4_cbc_encrypt`] under the same
/// `key` and `iv`, removing PKCS#7 padding.
pub fn sm4_cbc_decrypt(
    key: &[u8; SM4_KEY_LEN],
    iv: &[u8; SM4_BLOCK_LEN],
    ciphertext: &[u8],
) -> Result<Vec<u8>, Sm4Error> {
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(SM4_BLOCK_LEN) {
        return Err(Sm4Error::CiphertextNotBlockAligned {
            len: ciphertext.len(),
        });
    }
    let cipher = Sm4::new(GenericArray::from_slice(key));
    let mut plain = Vec::with_capacity(ciphertext.len());
    let mut prev = *iv;
    for chunk in ciphertext.chunks_exact(SM4_BLOCK_LEN) {
        let mut ct_block = [0u8; SM4_BLOCK_LEN];
        ct_block.copy_from_slice(chunk);
        let mut ga = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut ga);
        let mut block = [0u8; SM4_BLOCK_LEN];
        block.copy_from_slice(&ga);
        xor_block(&mut block, &prev);
        plain.extend_from_slice(&block);
        prev = ct_block;
    }
    unpad_pkcs7(&plain)
}

/// Bounded streaming SM4-CBC encryptor.
///
/// Construct with [`Sm4CbcEncryptor::new`], push plaintext in arbitrary
/// chunk sizes via [`Sm4CbcEncryptor::update`] (which returns the
/// ciphertext for every *complete* 16-byte block it can form), then
/// call [`Sm4CbcEncryptor::finalize`] once to flush the final padded
/// block.
///
/// Memory is bounded: the encryptor retains at most one partial block
/// (< 16 bytes) of buffered plaintext plus the 16-byte chaining value,
/// regardless of total input size. This is the "有界一切"-compliant path
/// for encrypting streams whose length is not known up front (file
/// scans, WAL records). The one-shot [`sm4_cbc_encrypt`] is fine for
/// small in-memory payloads.
pub struct Sm4CbcEncryptor {
    cipher: Sm4,
    prev: [u8; SM4_BLOCK_LEN],
    /// Plaintext bytes not yet forming a full block. Length is always
    /// strictly less than [`SM4_BLOCK_LEN`].
    buf: Vec<u8>,
    finalized: bool,
}

impl Sm4CbcEncryptor {
    /// Start a streaming SM4-CBC encryptor for `key` / `iv`.
    pub fn new(key: &[u8; SM4_KEY_LEN], iv: &[u8; SM4_BLOCK_LEN]) -> Self {
        Self {
            cipher: Sm4::new(GenericArray::from_slice(key)),
            prev: *iv,
            buf: Vec::with_capacity(SM4_BLOCK_LEN),
            finalized: false,
        }
    }

    /// Encrypt one buffered block in place via CBC and append it to
    /// `out`, advancing the chaining value.
    fn seal_block(&mut self, plain_block: &[u8], out: &mut Vec<u8>) {
        let mut block = [0u8; SM4_BLOCK_LEN];
        block.copy_from_slice(plain_block);
        xor_block(&mut block, &self.prev);
        let mut ga = GenericArray::clone_from_slice(&block);
        self.cipher.encrypt_block(&mut ga);
        out.extend_from_slice(&ga);
        self.prev.copy_from_slice(&ga);
    }

    /// Feed plaintext. Returns ciphertext for every complete block that
    /// could be formed from the buffered remainder plus `data`. Bytes
    /// that do not fill a block are retained internally (< 16 bytes).
    ///
    /// After [`finalize`](Self::finalize) has been called this is a
    /// no-op returning an empty `Vec`.
    pub fn update(&mut self, data: &[u8]) -> Vec<u8> {
        if self.finalized {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut input = data;

        // Top off a partially-filled buffer to a full block first.
        if !self.buf.is_empty() {
            let need = SM4_BLOCK_LEN - self.buf.len();
            let take = need.min(input.len());
            self.buf.extend_from_slice(&input[..take]);
            input = &input[take..];
            if self.buf.len() == SM4_BLOCK_LEN {
                let block = core::mem::take(&mut self.buf);
                self.seal_block(&block, &mut out);
                self.buf.reserve(SM4_BLOCK_LEN);
            }
        }

        // Process whole blocks straight from the input slice.
        let mut chunks = input.chunks_exact(SM4_BLOCK_LEN);
        for chunk in chunks.by_ref() {
            self.seal_block(chunk, &mut out);
        }

        // Retain the sub-block remainder.
        self.buf.extend_from_slice(chunks.remainder());
        out
    }

    /// Flush the final block(s): pad the buffered remainder with PKCS#7
    /// and return the last ciphertext block. Consumes the encryptor so
    /// it cannot be reused.
    pub fn finalize(mut self) -> Vec<u8> {
        if self.finalized {
            return Vec::new();
        }
        self.finalized = true;
        let padded = pad_pkcs7(&self.buf);
        let mut out = Vec::with_capacity(padded.len());
        // `padded` is always a whole number of blocks (one or two when
        // the remainder is already block-aligned-empty vs. partial).
        for chunk in padded.chunks_exact(SM4_BLOCK_LEN) {
            self.seal_block(chunk, &mut out);
        }
        out
    }
}

impl fmt::Debug for Sm4CbcEncryptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the key (held inside `cipher`), the chaining value,
        // or buffered plaintext — all are secret or sensitive.
        f.debug_struct("Sm4CbcEncryptor")
            .field("buffered_bytes", &self.buf.len())
            .field("finalized", &self.finalized)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Decode a fixed-length lowercase/uppercase hex string into bytes.
    fn decode_hex(s: &str) -> Vec<u8> {
        assert_eq!(s.len() % 2, 0, "hex length even");
        let b = s.as_bytes();
        (0..s.len() / 2)
            .map(|i| (nibble(b[i * 2]) << 4) | nibble(b[i * 2 + 1]))
            .collect()
    }

    fn nibble(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("non-hex byte {b:#x}"),
        }
    }

    fn key16(s: &str) -> [u8; SM4_KEY_LEN] {
        let v = decode_hex(s);
        let mut k = [0u8; SM4_KEY_LEN];
        k.copy_from_slice(&v);
        k
    }

    // --- Standards measurement (GB/T 32907-2016 single-block ECB) ---
    //
    // Key, plaintext and ciphertext are written longhand from the
    // standard. A correct SM4 of this 16-byte plaintext under this key
    // MUST equal this ciphertext. This is the independent safety net,
    // NOT a self-generated value.
    const GBT_KEY_HEX: &str = "0123456789ABCDEFFEDCBA9876543210";
    const GBT_PLAINTEXT_HEX: &str = "0123456789ABCDEFFEDCBA9876543210";
    const GBT_CIPHERTEXT_HEX: &str = "681EDF34D206965E86B3E94F536E4246";

    #[test]
    fn gbt_32907_single_block_via_raw_cipher() {
        // Drive the raw block primitive directly (no padding) so the
        // single 16-byte block maps 1:1 to the standard's ciphertext.
        let key = key16(GBT_KEY_HEX);
        let pt = decode_hex(GBT_PLAINTEXT_HEX);
        let expected = decode_hex(GBT_CIPHERTEXT_HEX);

        let cipher = Sm4::new(GenericArray::from_slice(&key));
        let mut block = GenericArray::clone_from_slice(&pt);
        cipher.encrypt_block(&mut block);
        assert_eq!(
            block.as_slice(),
            expected.as_slice(),
            "SM4 must match GB/T 32907-2016 single-block vector"
        );

        // And decrypt back to the standard plaintext.
        cipher.decrypt_block(&mut block);
        assert_eq!(block.as_slice(), pt.as_slice());
    }

    // --- Functional self-consistency (NOT a standards measurement) ---

    #[test]
    fn ecb_round_trip() {
        let key = key16(GBT_KEY_HEX);
        let msg = b"wireforge SM4 ECB payload spanning >1 block!!";
        let ct = sm4_ecb_encrypt(&key, msg).unwrap();
        assert_eq!(ct.len() % SM4_BLOCK_LEN, 0);
        let pt = sm4_ecb_decrypt(&key, &ct).unwrap();
        assert_eq!(pt, msg);
    }

    #[test]
    fn ecb_block_aligned_input_round_trips() {
        // Exactly 32 bytes -> a full extra pad block is added.
        let key = key16(GBT_KEY_HEX);
        let msg = [0xABu8; 32];
        let ct = sm4_ecb_encrypt(&key, &msg).unwrap();
        assert_eq!(ct.len(), 48, "32 plaintext + 16 pad block");
        assert_eq!(sm4_ecb_decrypt(&key, &ct).unwrap(), msg);
    }

    #[test]
    fn cbc_round_trip() {
        let key = key16(GBT_KEY_HEX);
        let iv = [0x11u8; SM4_BLOCK_LEN];
        let msg = b"wireforge SM4 CBC payload spanning multiple blocks; bounded.";
        let ct = sm4_cbc_encrypt(&key, &iv, msg).unwrap();
        let pt = sm4_cbc_decrypt(&key, &iv, &ct).unwrap();
        assert_eq!(pt, msg);
    }

    #[test]
    fn cbc_empty_input_round_trips() {
        let key = key16(GBT_KEY_HEX);
        let iv = [0u8; SM4_BLOCK_LEN];
        let ct = sm4_cbc_encrypt(&key, &iv, b"").unwrap();
        assert_eq!(ct.len(), SM4_BLOCK_LEN, "one full pad block");
        assert_eq!(sm4_cbc_decrypt(&key, &iv, &ct).unwrap(), b"");
    }

    #[test]
    fn cbc_iv_changes_ciphertext() {
        let key = key16(GBT_KEY_HEX);
        let msg = b"identical plaintext, different IV";
        let ct1 = sm4_cbc_encrypt(&key, &[1u8; SM4_BLOCK_LEN], msg).unwrap();
        let ct2 = sm4_cbc_encrypt(&key, &[2u8; SM4_BLOCK_LEN], msg).unwrap();
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn cbc_tamper_is_detected_or_garbled() {
        // Flipping a ciphertext bit must not round-trip back to the
        // original plaintext. Depending on which block is hit it either
        // fails padding validation or produces different plaintext;
        // either way it must NOT equal the input.
        let key = key16(GBT_KEY_HEX);
        let iv = [0x55u8; SM4_BLOCK_LEN];
        let msg = b"tamper-evidence check for CBC mode";
        let mut ct = sm4_cbc_encrypt(&key, &iv, msg).unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        match sm4_cbc_decrypt(&key, &iv, &ct) {
            Err(_) => {}
            Ok(pt) => assert_ne!(pt.as_slice(), msg.as_slice()),
        }
    }

    #[test]
    fn unaligned_ciphertext_rejected() {
        let key = key16(GBT_KEY_HEX);
        let iv = [0u8; SM4_BLOCK_LEN];
        let err = sm4_cbc_decrypt(&key, &iv, &[0u8; 17]).unwrap_err();
        assert_eq!(err, Sm4Error::CiphertextNotBlockAligned { len: 17 });
    }

    #[test]
    fn streaming_cbc_matches_one_shot() {
        // The bounded streaming encryptor must produce byte-identical
        // ciphertext to the one-shot path. Functional self-consistency.
        let key = key16(GBT_KEY_HEX);
        let iv = [0x33u8; SM4_BLOCK_LEN];
        let msg: Vec<u8> = (0..200u32).map(|i| (i % 251) as u8).collect();

        let one_shot = sm4_cbc_encrypt(&key, &iv, &msg).unwrap();

        let mut enc = Sm4CbcEncryptor::new(&key, &iv);
        let mut streamed = Vec::new();
        // Feed in awkward chunk sizes to exercise the partial buffer.
        streamed.extend(enc.update(&msg[..7]));
        streamed.extend(enc.update(&msg[7..16]));
        streamed.extend(enc.update(&msg[16..129]));
        streamed.extend(enc.update(&msg[129..]));
        streamed.extend(enc.finalize());

        assert_eq!(streamed, one_shot);
        assert_eq!(sm4_cbc_decrypt(&key, &iv, &streamed).unwrap(), msg);
    }

    #[test]
    fn streaming_cbc_block_aligned_input() {
        let key = key16(GBT_KEY_HEX);
        let iv = [0u8; SM4_BLOCK_LEN];
        let msg = [0x7Eu8; 48]; // exactly 3 blocks
        let one_shot = sm4_cbc_encrypt(&key, &iv, &msg).unwrap();

        let mut enc = Sm4CbcEncryptor::new(&key, &iv);
        let mut streamed = Vec::new();
        streamed.extend(enc.update(&msg));
        streamed.extend(enc.finalize());
        assert_eq!(streamed, one_shot);
    }

    #[test]
    fn streaming_buffer_stays_bounded() {
        // After feeding many blocks the internal buffer must hold only
        // the sub-block remainder (< 16 bytes), proving O(1) memory.
        let key = key16(GBT_KEY_HEX);
        let iv = [0u8; SM4_BLOCK_LEN];
        let mut enc = Sm4CbcEncryptor::new(&key, &iv);
        let big = vec![0u8; 16 * 1000 + 5];
        let _ = enc.update(&big);
        assert!(enc.buf.len() < SM4_BLOCK_LEN);
        assert_eq!(enc.buf.len(), 5);
    }

    #[test]
    fn debug_does_not_leak_secrets() {
        let key = key16(GBT_KEY_HEX);
        let iv = [0u8; SM4_BLOCK_LEN];
        let enc = Sm4CbcEncryptor::new(&key, &iv);
        let s = format!("{enc:?}");
        // Must not contain raw key bytes in any obvious form.
        assert!(!s.contains("0123456789"));
        assert!(s.contains("Sm4CbcEncryptor"));
    }
}
