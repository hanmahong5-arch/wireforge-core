//! Sanitize an ISO 8583 message: parse → redact PII per policy → rebuild →
//! verify length-preserved and round-trip-identical → emit redacted bytes plus
//! an audit record (which fields touched, anonymity-set size for the PAN).
//!
//! Policy reference: `docs/sample-policy.md` — redaction table, lines 30-44.
//! Honesty notes:
//! - This is an *anonymity* tool, not an *irreversibility* tool. The
//!   anonymity-set size we report is the count of Luhn-valid PAN
//!   completions consistent with the masked digits — it defends against a
//!   casual reader, not an adversary who also holds acquirer-side
//!   transaction logs that can re-identify by amount/timestamp/STAN.
//! - The redacted PAN intentionally contains non-numeric `X` bytes even
//!   though field 2's spec type is Numeric. `wf-codec`'s ASCII parser
//!   only validates the length prefix, not the payload character class,
//!   so the structurally-invalid-but-shape-correct PAN round-trips fine.
//!   This is deliberate: an `X`-masked PAN is unambiguously test data.

use wf_codec::iso8583::{build_with, parse_any, parse_with, BuildError, Dialect, ParseError};

/// Fields covered by the auto-redaction rules below. Anything outside this
/// list passes through verbatim — operators relying on extra fields (e.g.
/// custom field 60 sub-elements) must extend the rules and bump
/// [`crate::meta::SANITIZE_VERSION`].
pub const REDACTABLE_FIELDS: &[u8] = &[2, 35, 41, 42, 43, 45, 48, 102, 103];

/// Outcome of a successful sanitize. The caller writes `redacted_bytes` to
/// `<source>-<idx>.hex` and records the rest in `<source>-<idx>.meta.toml`.
#[derive(Debug, Clone)]
pub struct Sanitized {
    /// Final wire bytes after redact + rebuild. Same byte length as input.
    pub redacted_bytes: Vec<u8>,
    /// Field numbers that were actually touched (not just present-in-message).
    pub fields_redacted: Vec<u8>,
    /// Approximate count of Luhn-valid full-PAN completions consistent with
    /// the masked digits in field 2. Zero if field 2 was absent.
    pub anonymity_set_size: u128,
    /// Wire dialect detected from the input. Reported into `meta.toml` so the
    /// final report's coverage matrix can split samples by dialect.
    pub dialect: Dialect,
}

/// All failure modes the sanitizer surfaces. Reject loudly — never silently
/// drop a field or pass through unredacted bytes.
#[derive(Debug)]
pub enum SanitizeError {
    /// `wf-codec` could not parse the candidate. Often this means the input
    /// was a tutorial-grade blob with a hand-broken bitmap; we explicitly
    /// don't try to "repair" it — that would defeat the realism check.
    Parse(ParseError),
    /// `wf-codec` could not rebuild the message after redaction. Indicates
    /// a sanitizer bug (we should have produced length-spec-compatible bytes).
    Build(BuildError),
    /// Re-parsing the rebuilt bytes did not yield the same message. Indicates
    /// a `wf-codec` build/parse asymmetry bug. We refuse to emit.
    RoundTripMismatch,
    /// Output byte length differs from input. The policy (`sample-policy.md`
    /// line 47) requires byte-identical framing post-redaction.
    LengthChanged { before: usize, after: usize },
    /// Input dialect requires a redaction strategy this build does not
    /// implement yet. Today this only fires on `FullBinary` + field 2
    /// (PAN) — masking with ASCII `'X'` bytes would violate the BCD
    /// nibble invariant (every nibble must be `0..=9`). A future sprint
    /// adds a BCD-aware redactor (decode to ASCII digits → mask middle
    /// 6 → re-encode as zero digits); until then we reject loudly rather
    /// than silently produce a different field byte length.
    UnsupportedDialect {
        dialect: Dialect,
        field: u8,
        hint: &'static str,
    },
}

impl core::fmt::Display for SanitizeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SanitizeError::Parse(e) => write!(f, "parse failed: {e}"),
            SanitizeError::Build(e) => write!(f, "rebuild failed: {e}"),
            SanitizeError::RoundTripMismatch => write!(f, "round-trip mismatch after rebuild"),
            SanitizeError::LengthChanged { before, after } => {
                write!(f, "byte length changed: {before} -> {after}")
            }
            SanitizeError::UnsupportedDialect {
                dialect,
                field,
                hint,
            } => write!(
                f,
                "dialect {dialect:?} + field {field} redaction not implemented: {hint}"
            ),
        }
    }
}

impl std::error::Error for SanitizeError {}

impl From<ParseError> for SanitizeError {
    fn from(e: ParseError) -> Self {
        SanitizeError::Parse(e)
    }
}

impl From<BuildError> for SanitizeError {
    fn from(e: BuildError) -> Self {
        SanitizeError::Build(e)
    }
}

/// Run the full sanitize pipeline on one ISO 8583 message's wire bytes.
///
/// Auto-detects the source dialect via [`parse_any`] and rebuilds in the
/// SAME dialect so byte length and framing are preserved exactly. A
/// HybridAscii input that has its 8-byte binary bitmap re-rendered as 16
/// ASCII hex chars would no longer round-trip; the dialect carry-through
/// closes that hole.
pub fn sanitize(input: &[u8]) -> Result<Sanitized, SanitizeError> {
    let (mut msg, dialect) = parse_any(input)?;

    // FullBinary + field 2 PAN: the redactor emits ASCII 'X' bytes which
    // are not BCD-valid (high nibble 5, low nibble 8 — both ≤ 9 but they
    // decode to digits "58", not 'X'). Rebuilding via `build_with` would
    // also fail since `bcd::encode_bcd` only accepts ASCII digit bytes.
    // Pre-emptive reject gives a clearer message than the eventual build
    // error and avoids leaving partial state behind.
    if dialect == Dialect::FullBinary && msg.fields.contains_key(&2) {
        return Err(SanitizeError::UnsupportedDialect {
            dialect,
            field: 2,
            hint: "BCD redaction not implemented; decode to ASCII first \
                   or use a BCD-aware masking strategy in a future revision",
        });
    }

    let mut fields_redacted = Vec::new();
    let mut anonymity_set_size: u128 = 0;

    for &field in REDACTABLE_FIELDS {
        if let Some(payload) = msg.fields.get(&field).cloned() {
            let (new_payload, anon_delta) = redact_field(field, &payload);
            if new_payload != payload {
                msg.fields.insert(field, new_payload);
                fields_redacted.push(field);
                if field == 2 {
                    anonymity_set_size = anon_delta;
                }
            }
        }
    }

    let rebuilt = build_with(&msg, dialect)?;
    if rebuilt.len() != input.len() {
        return Err(SanitizeError::LengthChanged {
            before: input.len(),
            after: rebuilt.len(),
        });
    }
    if parse_with(&rebuilt, dialect)? != msg {
        return Err(SanitizeError::RoundTripMismatch);
    }

    Ok(Sanitized {
        redacted_bytes: rebuilt,
        fields_redacted,
        anonymity_set_size,
        dialect,
    })
}

/// Pure per-field redaction. Returns `(new_payload, anonymity_set_size)`.
/// `anonymity_set_size` is only meaningful for field 2; zero otherwise.
fn redact_field(field: u8, payload: &[u8]) -> (Vec<u8>, u128) {
    match field {
        2 => redact_pan(payload),
        35 | 45 => (mask_all(payload), 0),
        41 => (replace_truncated(payload, b"WIREFORGE_TEST_"), 0),
        42 => (replace_truncated(payload, b"WIREFORGE_TEST_"), 0),
        43 => (
            replace_truncated(payload, b"JOHN DOE / WIREFORGE TEST       "),
            0,
        ),
        48 => (mask_all(payload), 0),
        102 | 103 => (zeros(payload.len()), 0),
        _ => (payload.to_vec(), 0),
    }
}

/// PAN redaction per policy: keep first 6 + last 4, replace middle with `X`.
/// Returns the redacted bytes plus the Luhn-constrained anonymity-set size.
///
/// Anonymity-set math: for `k` masked digit positions and a Luhn constraint
/// (one linear equation mod 10 over digits 0-9), exactly `10^(k-1)` PAN
/// completions are Luhn-valid given the unmasked digits. If `k == 0`, no
/// redaction was applied and the anonymity set is 1 (PAN fully revealed —
/// shouldn't happen for a real PAN since min PAN length is 13 and we keep
/// only 10 digits).
fn redact_pan(payload: &[u8]) -> (Vec<u8>, u128) {
    let len = payload.len();
    if len <= 10 {
        // PAN shorter than 10 chars — unusual; keep only first byte, mask rest.
        let mut out = Vec::with_capacity(len);
        if len > 0 {
            out.push(payload[0]);
        }
        out.extend(std::iter::repeat_n(b'X', len.saturating_sub(1)));
        let masked = len.saturating_sub(1) as u32;
        let anon = pow10_minus_one(masked);
        return (out, anon);
    }
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&payload[..6]);
    let mid_len = len - 10;
    out.extend(std::iter::repeat_n(b'X', mid_len));
    out.extend_from_slice(&payload[len - 4..]);
    let anon = pow10_minus_one(mid_len as u32);
    (out, anon)
}

/// `10^(k-1)`; saturates at u128::MAX for k beyond representable range. For
/// `k == 0` returns 1 (a single, fully-determined PAN).
fn pow10_minus_one(k: u32) -> u128 {
    if k == 0 {
        return 1;
    }
    let exp = k - 1;
    if exp >= 38 {
        // 10^38 > u128::MAX (~3.4 * 10^38); saturate.
        return u128::MAX;
    }
    10u128.saturating_pow(exp)
}

fn mask_all(payload: &[u8]) -> Vec<u8> {
    vec![b'X'; payload.len()]
}

fn zeros(len: usize) -> Vec<u8> {
    vec![b'0'; len]
}

/// Replace `payload`'s contents with `replacement`, truncating or padding
/// (with spaces) to preserve the original byte length exactly.
fn replace_truncated(payload: &[u8], replacement: &[u8]) -> Vec<u8> {
    let len = payload.len();
    let mut out = Vec::with_capacity(len);
    let take = replacement.len().min(len);
    out.extend_from_slice(&replacement[..take]);
    while out.len() < len {
        out.push(b' ');
    }
    out
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use wf_codec::iso8583::Iso8583Message;

    /// Build a synthetic 0200 message carrying a PAN (field 2), processing
    /// code (field 3), amount (field 4), Track 2 (field 35), and a card
    /// acceptor name (field 43). All values are spec-shape so `build` and
    /// `parse` round-trip.
    ///
    /// Per the anti-tautology rule ("测量与被测不可同源"): this synthetic input tests
    /// the *sanitizer code path*, NOT the parse-accuracy baseline. No tautology — the
    /// sanitizer's job is to redact + round-trip-verify, and that contract
    /// can (and should) be validated against deterministic inputs.
    fn synthetic_message() -> Iso8583Message {
        let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
        // Field 2 PAN: 16 digits — Visa-shape test number, NOT a real PAN.
        fields.insert(2, b"4111111111111111".to_vec());
        // Field 3 Processing code: 6 digits, must not be redacted.
        fields.insert(3, b"000000".to_vec());
        // Field 4 Amount: 12 digits, must not be redacted.
        fields.insert(4, b"000000001000".to_vec());
        // Field 35 Track 2: digits + '=' separator; LLVAR.
        fields.insert(35, b"4111111111111111=25121011234567890".to_vec());
        // Field 43 Card Acceptor Name/Location: ANS fixed 40. Content is
        // arbitrary — what matters for this test is that the byte length is
        // exactly 40 (per ISO 8583 spec) and that the bytes differ from the
        // sanitizer's replacement string so the diff assertion is meaningful.
        fields.insert(43, vec![b'A'; 40]);
        Iso8583Message {
            mti: *b"0200",
            fields,
        }
    }

    #[test]
    fn pan_redaction_keeps_first_six_last_four() {
        let (out, _anon) = redact_pan(b"4111111111111111");
        assert_eq!(&out[..6], b"411111");
        assert_eq!(&out[6..12], b"XXXXXX");
        assert_eq!(&out[12..], b"1111");
    }

    #[test]
    fn pan_anonymity_set_is_ten_to_masked_minus_one() {
        // 16-digit PAN → 6 masked → anonymity = 10^5.
        let (_out, anon) = redact_pan(b"4111111111111111");
        assert_eq!(anon, 100_000);
        // 19-digit PAN → 9 masked → anonymity = 10^8.
        let (_out, anon) = redact_pan(b"4111111111111111234");
        assert_eq!(anon, 100_000_000);
    }

    #[test]
    fn sanitize_redacts_expected_fields_and_round_trips() {
        let msg = synthetic_message();
        let wire = build_with(&msg, Dialect::HybridAscii).expect("synthetic fixture must build");

        let out = sanitize(&wire).expect("synthetic fixture must sanitize cleanly");

        assert_eq!(out.redacted_bytes.len(), wire.len(), "length preserved");
        assert_eq!(out.fields_redacted, vec![2, 35, 43]);
        assert_eq!(out.anonymity_set_size, 100_000);
        assert_eq!(out.dialect, Dialect::HybridAscii);

        let reparsed =
            parse_with(&out.redacted_bytes, Dialect::HybridAscii).expect("redacted bytes re-parse");
        assert_ne!(reparsed.fields.get(&2), msg.fields.get(&2));
        assert_ne!(reparsed.fields.get(&35), msg.fields.get(&35));
        assert_ne!(reparsed.fields.get(&43), msg.fields.get(&43));
        assert_eq!(reparsed.fields.get(&3), msg.fields.get(&3));
        assert_eq!(reparsed.fields.get(&4), msg.fields.get(&4));
    }

    #[test]
    fn sanitize_detects_and_preserves_full_ascii_dialect() {
        // Same logical message, but built in FullAscii dialect — exercises the
        // parse_any → redact → build_with(detected) dialect carry-through that
        // is the whole reason dialect support exists.
        let msg = synthetic_message();
        let wire = build_with(&msg, Dialect::FullAscii).expect("FullAscii build must succeed");

        let out = sanitize(&wire).expect("FullAscii input must sanitize");
        assert_eq!(out.dialect, Dialect::FullAscii);
        assert_eq!(out.redacted_bytes.len(), wire.len(), "length preserved");
        assert_eq!(out.fields_redacted, vec![2, 35, 43]);

        // The redacted wire must parse back as FullAscii and reproduce the
        // redacted message exactly — i.e. byte-for-byte round-trip.
        let reparsed = parse_with(&out.redacted_bytes, Dialect::FullAscii)
            .expect("redacted FullAscii bytes re-parse");
        assert_ne!(reparsed.fields.get(&2), msg.fields.get(&2));
        assert_eq!(reparsed.fields.get(&3), msg.fields.get(&3));
    }

    #[test]
    fn sanitize_rejects_malformed_input() {
        // Truncated bitmap → parse error.
        let garbage = b"0200abcd";
        assert!(matches!(sanitize(garbage), Err(SanitizeError::Parse(_))));
    }

    #[test]
    fn sanitize_rejects_full_binary_pan_with_explicit_hint() {
        // Build the same logical PAN message in FullBinary — the redactor
        // can't emit 'X' nibbles into BCD without violating the spec, so the
        // sanitizer must reject loudly with a UnsupportedDialect error
        // rather than silently produce a corrupted PAN or unequal length.
        let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
        fields.insert(2, b"4111111111111111".to_vec());
        let msg = Iso8583Message {
            mti: *b"0200",
            fields,
        };
        let wire = build_with(&msg, Dialect::FullBinary).expect("FullBinary build must succeed");

        let err = sanitize(&wire).expect_err("FullBinary + field 2 must reject");
        match err {
            SanitizeError::UnsupportedDialect {
                dialect,
                field,
                hint,
            } => {
                assert_eq!(dialect, Dialect::FullBinary);
                assert_eq!(field, 2);
                assert!(!hint.is_empty(), "hint must not be empty");
            }
            other => panic!("expected UnsupportedDialect, got {other:?}"),
        }
    }

    #[test]
    fn sanitize_accepts_full_binary_without_field_2() {
        // FullBinary with a non-PAN-only field set is still serviceable —
        // only field 2 carries the BCD masking incompatibility today.
        let mut fields: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
        fields.insert(3, b"000000".to_vec());
        fields.insert(4, b"000000010000".to_vec());
        let msg = Iso8583Message {
            mti: *b"0200",
            fields,
        };
        let wire = build_with(&msg, Dialect::FullBinary).expect("FullBinary build must succeed");
        let out = sanitize(&wire).expect("FullBinary without field 2 must sanitize");
        assert_eq!(out.dialect, Dialect::FullBinary);
        assert!(
            out.fields_redacted.is_empty(),
            "nothing redactable was set; got {:?}",
            out.fields_redacted
        );
    }

    #[test]
    fn replace_truncated_pads_short_replacement_with_space() {
        let out = replace_truncated(&[b'A'; 10], b"HI");
        assert_eq!(out, b"HI        ");
    }

    #[test]
    fn replace_truncated_clips_long_replacement_to_field_length() {
        let out = replace_truncated(&[b'A'; 3], b"WIREFORGE_TEST_");
        assert_eq!(out, b"WIR");
    }
}
