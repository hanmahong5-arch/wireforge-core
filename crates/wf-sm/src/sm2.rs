//! SM2 (GM/T 0003-2012, GB/T 32918) elliptic-curve digital signature.
//!
//! SM2 is the China GM/T public-key signature scheme over a 256-bit
//! prime-field curve. Signing prepends an identity hash `ZA` (computed
//! from a *distinguishing identifier* and the public key, hashed with
//! SM3) to the message before the EC operation; this wrapper delegates
//! that `ZA` handling to the upstream crate.
//!
//! This module is a thin, fixed-shape wrapper around RustCrypto's
//! [`sm2`](https://docs.rs/sm2) crate (the `dsa` feature). The wrapper
//! exists to:
//!
//! - Present a single [`Sm2KeyPair`] type with byte-oriented
//!   `from_*` / `to_*` accessors instead of exposing the upstream's
//!   generic `SecretKey` / `SigningKey` surface to every call site.
//! - Surface **both** signature encodings consumers need: fixed 64-byte
//!   raw `r || s` and variable-length DER.
//! - Default the distinguishing identifier to the GM/T conventional
//!   `"1234567812345678"` while letting callers override it.
//!
//! # Not a compliance boundary
//!
//! RustCrypto's `sm2` crate is **unaudited**. This module makes **no**
//! 密评 / GB/T 39786 / OSCCA compliance claim of any kind — it provides
//! *functional* SM2 signing/verification only. A compliance-grade SM2
//! path (certified module, 密评 evidence) is a separate Tongsuo C-FFI
//! route documented in the strategy memo, not this code.
//!
//! # Test scope
//!
//! Tests cover sign→verify round-trips, tamper detection, cross-key
//! rejection, and DER ↔ raw `(r,s)` consistency — all *functional
//! self-consistency*. SM2's standard example signatures are produced
//! with a fixed random nonce `k`; the upstream API draws `k` from an
//! RNG and does not expose nonce injection, so a published `(r,s)`
//! vector cannot be reproduced deterministically here. No independent
//! `(msg, pubkey, signature)` vector is embedded — see the honesty note
//! in the test module; this is disclosed rather than fabricated.

use ::sm2::dsa::signature::{Signer, Verifier};
use ::sm2::dsa::{Signature, SigningKey, VerifyingKey};
use ::sm2::{FieldBytes, SecretKey};
use core::fmt;

/// Length of a raw SM2 signature in bytes (`r || s`, 32 bytes each).
pub const SM2_SIGNATURE_RAW_LEN: usize = 64;

/// Length of an SM2 private key scalar in bytes.
pub const SM2_PRIVATE_KEY_LEN: usize = 32;

/// The GM/T conventional default distinguishing identifier (`1234...5678`).
pub const SM2_DEFAULT_ID: &str = "1234567812345678";

/// Errors returned by the SM2 wrapper.
///
/// Each variant's [`fmt::Display`] follows this crate's three-element
/// error convention: what happened, what was expected, what the caller
/// can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sm2Error {
    /// The supplied private-key bytes were not a valid SM2 scalar.
    InvalidPrivateKey,
    /// The supplied public-key bytes were not a valid SM2 point/encoding.
    InvalidPublicKey,
    /// The distinguishing identifier was rejected (e.g. too long for the
    /// 16-bit length prefix the `ZA` construction uses).
    InvalidDistinguishingId,
    /// A raw signature buffer was the wrong length or not a valid `(r,s)`.
    InvalidSignatureEncoding,
}

impl fmt::Display for Sm2Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Sm2Error::InvalidPrivateKey => write!(
                f,
                "SM2 private-key bytes are not a valid curve scalar; \
                 expected {SM2_PRIVATE_KEY_LEN} big-endian bytes in the \
                 group order range; regenerate the key or verify the \
                 source encoding"
            ),
            Sm2Error::InvalidPublicKey => write!(
                f,
                "SM2 public-key bytes are not a valid curve point; \
                 expected a SEC1-encoded point on the SM2 curve; verify \
                 the key was exported from a compatible SM2 implementation"
            ),
            Sm2Error::InvalidDistinguishingId => write!(
                f,
                "SM2 distinguishing identifier was rejected; expected an \
                 identifier short enough for the ZA length prefix (well \
                 under 8192 bytes); shorten the identifier"
            ),
            Sm2Error::InvalidSignatureEncoding => write!(
                f,
                "SM2 signature bytes could not be parsed; expected either \
                 a {SM2_SIGNATURE_RAW_LEN}-byte raw r||s buffer or valid \
                 DER; verify the signature was produced by a compatible \
                 SM2 implementation and not truncated"
            ),
        }
    }
}

impl std::error::Error for Sm2Error {}

/// An SM2 signature, wrapping the upstream signature type so callers
/// depend on this crate's surface rather than the upstream's.
#[derive(Clone)]
pub struct Sm2Signature {
    inner: Signature,
}

impl Sm2Signature {
    /// Encode as the fixed 64-byte raw form `r || s` (32 bytes each).
    pub fn to_raw_bytes(&self) -> [u8; SM2_SIGNATURE_RAW_LEN] {
        let bytes = self.inner.to_bytes();
        let mut out = [0u8; SM2_SIGNATURE_RAW_LEN];
        // `to_bytes` yields exactly 64 bytes for SM2; copy defensively
        // by length so a future width change cannot overrun.
        let n = bytes.len().min(SM2_SIGNATURE_RAW_LEN);
        out[..n].copy_from_slice(&bytes[..n]);
        out
    }

    /// Encode as variable-length ASN.1 DER (`SEQUENCE { INTEGER r,
    /// INTEGER s }`), the interop form used by most SM2 toolchains.
    ///
    /// The upstream `dsa::Signature` only exposes the fixed 64-byte raw
    /// `r || s` form under the features we enable, so we build the DER
    /// SEQUENCE here from the two 32-byte scalars. This pairs with
    /// [`from_der`](Self::from_der) for a faithful round-trip.
    pub fn to_der(&self) -> Vec<u8> {
        let raw = self.inner.to_bytes();
        // raw is exactly 64 bytes: r = raw[..32], s = raw[32..].
        let (r, s) = raw.split_at(SM2_SCALAR_LEN);
        encode_der_ecdsa_sig(r, s)
    }

    /// Parse a 64-byte raw `r || s` signature.
    pub fn from_raw_bytes(bytes: &[u8; SM2_SIGNATURE_RAW_LEN]) -> Result<Self, Sm2Error> {
        Signature::from_slice(bytes)
            .map(|inner| Self { inner })
            .map_err(|_| Sm2Error::InvalidSignatureEncoding)
    }

    /// Parse a DER-encoded signature (`SEQUENCE { INTEGER r, INTEGER s }`).
    ///
    /// The upstream signature type does not expose a DER parser under the
    /// features we enable, so we decode the two `INTEGER`s with a small
    /// local DER reader and rebuild via `from_scalars`. This keeps the
    /// crate free of an extra `der` dependency while round-tripping the
    /// output of [`to_der`](Self::to_der).
    pub fn from_der(der: &[u8]) -> Result<Self, Sm2Error> {
        let (r, s) = parse_der_ecdsa_sig(der).ok_or(Sm2Error::InvalidSignatureEncoding)?;
        let r_fb = scalar_to_field_bytes(&r).ok_or(Sm2Error::InvalidSignatureEncoding)?;
        let s_fb = scalar_to_field_bytes(&s).ok_or(Sm2Error::InvalidSignatureEncoding)?;
        Signature::from_scalars(r_fb, s_fb)
            .map(|inner| Self { inner })
            .map_err(|_| Sm2Error::InvalidSignatureEncoding)
    }
}

impl fmt::Debug for Sm2Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Signatures are not secret, but keep Debug compact and stable.
        f.debug_struct("Sm2Signature").finish_non_exhaustive()
    }
}

/// Length of an SM2 scalar (`r` or `s`) in bytes.
const SM2_SCALAR_LEN: usize = 32;

/// Encode a single non-negative big-endian integer as a DER `INTEGER`
/// TLV. A leading `0x00` is prepended when the high bit of the first
/// magnitude byte is set, so the value is unambiguously positive.
fn encode_der_integer(magnitude: &[u8]) -> Vec<u8> {
    const INTEGER: u8 = 0x02;
    // Strip leading zero bytes to a minimal magnitude (keep at least one).
    let mut start = 0usize;
    while start + 1 < magnitude.len() && magnitude[start] == 0x00 {
        start += 1;
    }
    let mag = &magnitude[start..];
    let need_pad = mag.first().is_some_and(|&b| b & 0x80 != 0);

    let content_len = mag.len() + usize::from(need_pad);
    let mut out = Vec::with_capacity(2 + content_len);
    out.push(INTEGER);
    // content_len for a 32-byte scalar (+1 pad) is <= 33, fits short form.
    out.push(content_len as u8);
    if need_pad {
        out.push(0x00);
    }
    out.extend_from_slice(mag);
    out
}

/// Encode an ECDSA/SM2 signature as DER `SEQUENCE { INTEGER r,
/// INTEGER s }` from two big-endian scalar magnitudes.
fn encode_der_ecdsa_sig(r: &[u8], s: &[u8]) -> Vec<u8> {
    const SEQUENCE: u8 = 0x30;
    let r_der = encode_der_integer(r);
    let s_der = encode_der_integer(s);
    let body_len = r_der.len() + s_der.len();
    let mut out = Vec::with_capacity(2 + body_len);
    out.push(SEQUENCE);
    // body_len for two <=33-byte integers is <= 70, fits short form.
    out.push(body_len as u8);
    out.extend_from_slice(&r_der);
    out.extend_from_slice(&s_der);
    out
}

/// Right-align a big-endian, minimal-length scalar into a fixed
/// [`FieldBytes`] (32 bytes). Returns `None` if the scalar is longer
/// than 32 bytes (not a valid SM2 component).
fn scalar_to_field_bytes(scalar: &[u8]) -> Option<FieldBytes> {
    if scalar.len() > SM2_SCALAR_LEN {
        return None;
    }
    let mut out = [0u8; SM2_SCALAR_LEN];
    out[SM2_SCALAR_LEN - scalar.len()..].copy_from_slice(scalar);
    Some(FieldBytes::from(out))
}

/// Minimal DER reader for an ECDSA/SM2 signature
/// (`SEQUENCE { INTEGER r, INTEGER s }`). Returns the two integer
/// contents as big-endian byte slices (with any DER sign-padding `0x00`
/// leading byte stripped), or `None` on any structural error.
///
/// This is intentionally tiny: it accepts only the exact two-INTEGER
/// SEQUENCE shape an SM2 signature uses and rejects anything else
/// (trailing bytes, wrong tags, long-form lengths beyond what a 32-byte
/// integer needs).
fn parse_der_ecdsa_sig(der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut pos = 0usize;

    // Read a single DER TLV with a short-form (single-byte) length.
    // Returns (tag, contents, new_pos). SM2 r/s integers are <= 33
    // bytes, so short-form length is always sufficient here.
    fn read_tlv(buf: &[u8], pos: usize) -> Option<(u8, &[u8], usize)> {
        let tag = *buf.get(pos)?;
        let len = *buf.get(pos + 1)? as usize;
        // Reject long-form lengths (high bit set): not needed for SM2.
        if len & 0x80 != 0 {
            return None;
        }
        let start = pos + 2;
        let end = start.checked_add(len)?;
        if end > buf.len() {
            return None;
        }
        Some((tag, &buf[start..end], end))
    }

    const SEQUENCE: u8 = 0x30;
    const INTEGER: u8 = 0x02;

    let (tag, seq_body, end) = read_tlv(der, pos)?;
    if tag != SEQUENCE || end != der.len() {
        return None; // not a SEQUENCE, or trailing bytes after it
    }

    // Parse the two INTEGERs inside the SEQUENCE body.
    pos = 0;
    let (r_tag, r_bytes, r_end) = read_tlv(seq_body, pos)?;
    if r_tag != INTEGER {
        return None;
    }
    pos = r_end;
    let (s_tag, s_bytes, s_end) = read_tlv(seq_body, pos)?;
    if s_tag != INTEGER || s_end != seq_body.len() {
        return None; // not exactly two integers
    }

    Some((strip_der_int(r_bytes)?, strip_der_int(s_bytes)?))
}

/// Strip a single DER sign-padding `0x00` byte if present, returning the
/// minimal big-endian magnitude. Rejects empty integers.
fn strip_der_int(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.is_empty() {
        return None;
    }
    // A leading 0x00 is DER sign padding when the next byte's high bit
    // is set; in either harmless case stripping a single leading zero is
    // safe for a positive magnitude.
    let trimmed = if bytes.len() > 1 && bytes[0] == 0x00 {
        &bytes[1..]
    } else {
        bytes
    };
    Some(trimmed.to_vec())
}

/// An SM2 key pair plus the distinguishing identifier used to derive the
/// `ZA` identity hash during signing/verification.
///
/// The private key is held in [`SecretKey`] and is **never** printed by
/// the [`fmt::Debug`] impl.
#[derive(Clone)]
pub struct Sm2KeyPair {
    secret: SecretKey,
    distinguishing_id: String,
}

impl Sm2KeyPair {
    /// Generate a fresh random key pair using the default distinguishing
    /// identifier [`SM2_DEFAULT_ID`].
    pub fn generate() -> Self {
        // Use the OS CSPRNG re-exported through the sm2 crate's
        // elliptic-curve dependency so we don't pin a separate
        // rand_core version that could drift from the upstream's.
        let secret = SecretKey::random(&mut ::sm2::elliptic_curve::rand_core::OsRng);
        Self {
            secret,
            distinguishing_id: SM2_DEFAULT_ID.to_string(),
        }
    }

    /// Override the distinguishing identifier used for `ZA`. Both signer
    /// and verifier must agree on this value.
    pub fn with_distinguishing_id(mut self, id: &str) -> Self {
        self.distinguishing_id = id.to_string();
        self
    }

    /// Reconstruct a key pair from a 32-byte big-endian private scalar,
    /// using the default distinguishing identifier.
    pub fn from_private_key_bytes(bytes: &[u8; SM2_PRIVATE_KEY_LEN]) -> Result<Self, Sm2Error> {
        let secret = SecretKey::from_slice(bytes).map_err(|_| Sm2Error::InvalidPrivateKey)?;
        Ok(Self {
            secret,
            distinguishing_id: SM2_DEFAULT_ID.to_string(),
        })
    }

    /// Export the 32-byte big-endian private scalar.
    ///
    /// Handle the result as secret material; it is not redacted here
    /// because the caller explicitly asked for it.
    pub fn to_private_key_bytes(&self) -> [u8; SM2_PRIVATE_KEY_LEN] {
        let field = self.secret.to_bytes();
        let mut out = [0u8; SM2_PRIVATE_KEY_LEN];
        let n = field.len().min(SM2_PRIVATE_KEY_LEN);
        out[..n].copy_from_slice(&field[..n]);
        out
    }

    /// Export the public key in SEC1 point encoding (compressed = false).
    pub fn public_key_sec1_bytes(&self) -> Vec<u8> {
        use ::sm2::elliptic_curve::sec1::ToEncodedPoint;
        self.secret
            .public_key()
            .to_encoded_point(false)
            .as_bytes()
            .to_vec()
    }

    /// The distinguishing identifier this key pair signs/verifies with.
    pub fn distinguishing_id(&self) -> &str {
        &self.distinguishing_id
    }

    /// Build the upstream signing key (which binds the distinguishing id).
    fn signing_key(&self) -> Result<SigningKey, Sm2Error> {
        SigningKey::new(&self.distinguishing_id, &self.secret)
            .map_err(|_| Sm2Error::InvalidDistinguishingId)
    }

    /// Sign `message`. The upstream computes `ZA || message` (with SM3)
    /// internally; callers pass the raw message, not a pre-hash.
    pub fn sign(&self, message: &[u8]) -> Result<Sm2Signature, Sm2Error> {
        let key = self.signing_key()?;
        let sig: Signature = key
            .try_sign(message)
            .map_err(|_| Sm2Error::InvalidSignatureEncoding)?;
        Ok(Sm2Signature { inner: sig })
    }

    /// Verify `signature` over `message` against this key pair's own
    /// public key, using its distinguishing identifier.
    pub fn verify(&self, message: &[u8], signature: &Sm2Signature) -> Result<bool, Sm2Error> {
        let key = self.signing_key()?;
        Ok(key
            .verifying_key()
            .verify(message, &signature.inner)
            .is_ok())
    }
}

impl fmt::Debug for Sm2KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SECURITY: never expose the private scalar. Show only the
        // public, non-sensitive distinguishing id and a redaction
        // marker for the key material.
        f.debug_struct("Sm2KeyPair")
            .field("private_key", &"<redacted>")
            .field("distinguishing_id", &self.distinguishing_id)
            .finish_non_exhaustive()
    }
}

/// Verify `signature` over `message` against an externally supplied
/// SEC1 public key and distinguishing identifier. Use this when the
/// verifier holds only the peer's public key, not a full key pair.
///
/// Returns `Ok(true)` on a valid signature, `Ok(false)` on a
/// well-formed but non-matching signature, and `Err` if the inputs
/// could not be parsed.
pub fn sm2_verify(
    public_key_sec1: &[u8],
    distinguishing_id: &str,
    message: &[u8],
    signature: &Sm2Signature,
) -> Result<bool, Sm2Error> {
    let public = ::sm2::PublicKey::from_sec1_bytes(public_key_sec1)
        .map_err(|_| Sm2Error::InvalidPublicKey)?;
    let verifying = VerifyingKey::new(distinguishing_id, public)
        .map_err(|_| Sm2Error::InvalidDistinguishingId)?;
    Ok(verifying.verify(message, &signature.inner).is_ok())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    // HONESTY NOTE (per the project's test-independence policy): all SM2 tests below are FUNCTIONAL
    // SELF-CONSISTENCY, not a standards measurement. SM2's published
    // example signatures fix the random nonce k; the RustCrypto sm2 API
    // draws k from an RNG and exposes no nonce-injection hook, so a
    // canonical (r,s) vector cannot be reproduced deterministically. No
    // independent (msg, pubkey, signature) vector is embedded — this is
    // disclosed, not fabricated. Cross-key rejection below provides an
    // independence check (a signature must not verify under a different,
    // independently generated key).

    #[test]
    fn sign_then_verify_round_trips() {
        let kp = Sm2KeyPair::generate();
        let msg = b"wireforge SM2 functional round-trip";
        let sig = kp.sign(msg).unwrap();
        assert!(kp.verify(msg, &sig).unwrap());
    }

    #[test]
    fn tampered_message_fails_verification() {
        let kp = Sm2KeyPair::generate();
        let msg = b"original message bytes";
        let sig = kp.sign(msg).unwrap();

        let mut tampered = msg.to_vec();
        tampered[0] ^= 0x01;
        assert!(!kp.verify(&tampered, &sig).unwrap());
    }

    #[test]
    fn signature_does_not_verify_under_a_different_key() {
        // Independence check: a signature from key A must not verify
        // against an independently generated key B.
        let a = Sm2KeyPair::generate();
        let b = Sm2KeyPair::generate();
        let msg = b"cross-key rejection";
        let sig = a.sign(msg).unwrap();
        assert!(a.verify(msg, &sig).unwrap());
        assert!(!b.verify(msg, &sig).unwrap());
    }

    #[test]
    fn der_round_trip_is_consistent() {
        let kp = Sm2KeyPair::generate();
        let msg = b"DER encode/decode consistency";
        let sig = kp.sign(msg).unwrap();

        let der = sig.to_der();
        let reparsed = Sm2Signature::from_der(&der).unwrap();
        // Re-encoded DER must be byte-identical.
        assert_eq!(reparsed.to_der(), der);
        // And it must still verify.
        assert!(kp.verify(msg, &reparsed).unwrap());
    }

    #[test]
    fn raw_round_trip_is_consistent() {
        let kp = Sm2KeyPair::generate();
        let msg = b"raw r||s encode/decode consistency";
        let sig = kp.sign(msg).unwrap();

        let raw = sig.to_raw_bytes();
        assert_eq!(raw.len(), SM2_SIGNATURE_RAW_LEN);
        let reparsed = Sm2Signature::from_raw_bytes(&raw).unwrap();
        assert_eq!(reparsed.to_raw_bytes(), raw);
        assert!(kp.verify(msg, &reparsed).unwrap());
    }

    #[test]
    fn der_and_raw_describe_the_same_signature() {
        let kp = Sm2KeyPair::generate();
        let msg = b"DER and raw must agree";
        let sig = kp.sign(msg).unwrap();

        let from_raw = Sm2Signature::from_raw_bytes(&sig.to_raw_bytes()).unwrap();
        let from_der = Sm2Signature::from_der(&sig.to_der()).unwrap();
        // Both reconstructions must verify the same message.
        assert!(kp.verify(msg, &from_raw).unwrap());
        assert!(kp.verify(msg, &from_der).unwrap());
        // And their raw encodings must match each other.
        assert_eq!(from_raw.to_raw_bytes(), from_der.to_raw_bytes());
    }

    #[test]
    fn private_key_bytes_round_trip() {
        let kp = Sm2KeyPair::generate();
        let priv_bytes = kp.to_private_key_bytes();
        let restored = Sm2KeyPair::from_private_key_bytes(&priv_bytes).unwrap();
        assert_eq!(restored.to_private_key_bytes(), priv_bytes);

        // A signature from the restored key verifies against itself.
        let msg = b"reconstructed key still works";
        let sig = restored.sign(msg).unwrap();
        assert!(restored.verify(msg, &sig).unwrap());
    }

    #[test]
    fn external_public_key_verification() {
        // Exercise the public-key-only verification path against the
        // SEC1 export, matching the on-the-wire verifier shape.
        let kp = Sm2KeyPair::generate();
        let msg = b"public-key-only verification path";
        let sig = kp.sign(msg).unwrap();

        let pub_sec1 = kp.public_key_sec1_bytes();
        assert!(sm2_verify(&pub_sec1, kp.distinguishing_id(), msg, &sig).unwrap());

        // Wrong message must fail.
        assert!(!sm2_verify(&pub_sec1, kp.distinguishing_id(), b"other", &sig).unwrap());
    }

    #[test]
    fn distinguishing_id_must_match() {
        // ZA binds the distinguishing id, so a verifier using a
        // different id must reject an otherwise-valid signature.
        let kp = Sm2KeyPair::generate().with_distinguishing_id("ALICE@wireforge");
        let msg = b"id binding via ZA";
        let sig = kp.sign(msg).unwrap();
        let pub_sec1 = kp.public_key_sec1_bytes();

        assert!(sm2_verify(&pub_sec1, "ALICE@wireforge", msg, &sig).unwrap());
        assert!(!sm2_verify(&pub_sec1, "BOB@wireforge", msg, &sig).unwrap());
    }

    #[test]
    fn bad_signature_encodings_rejected() {
        // All-zero raw bytes are not a valid (r,s) pair.
        let zeros = [0u8; SM2_SIGNATURE_RAW_LEN];
        assert_eq!(
            Sm2Signature::from_raw_bytes(&zeros).unwrap_err(),
            Sm2Error::InvalidSignatureEncoding
        );
        // Garbage DER is rejected.
        assert_eq!(
            Sm2Signature::from_der(&[0xFF, 0x00, 0x01]).unwrap_err(),
            Sm2Error::InvalidSignatureEncoding
        );
    }

    #[test]
    fn invalid_private_key_rejected() {
        // All-zero scalar is not a valid SM2 private key.
        let zeros = [0u8; SM2_PRIVATE_KEY_LEN];
        assert_eq!(
            Sm2KeyPair::from_private_key_bytes(&zeros).unwrap_err(),
            Sm2Error::InvalidPrivateKey
        );
    }

    #[test]
    fn debug_redacts_private_key() {
        let kp = Sm2KeyPair::generate();
        let priv_hex: String = kp
            .to_private_key_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let dbg = format!("{kp:?}");
        assert!(dbg.contains("<redacted>"));
        assert!(dbg.contains("Sm2KeyPair"));
        // The actual private scalar hex must not appear.
        assert!(!dbg.contains(&priv_hex));
    }

    #[test]
    fn signature_debug_is_compact() {
        let kp = Sm2KeyPair::generate();
        let sig = kp.sign(b"x").unwrap();
        assert!(format!("{sig:?}").contains("Sm2Signature"));
    }
}
