//! Tag 20 — Sender's Reference.
//!
//! Spec: `16x` (up to 16 chars in the SWIFT X charset). Used in MT103
//! and most other customer-credit messages as the sender's unique handle
//! for the transaction.

use super::{is_swift_x, DecodeError, FieldSemantic, MtFieldDecoder};

const TAG: &str = "20";
const MAX_LEN: usize = 16;

/// Zero-sized decoder for tag 20.
#[derive(Debug, Clone, Copy, Default)]
pub struct Field20;

impl MtFieldDecoder for Field20 {
    fn tag(&self) -> &'static str {
        TAG
    }

    fn decode(&self, raw: &str) -> Result<FieldSemantic, DecodeError> {
        if raw.is_empty() {
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: 0,
                max: MAX_LEN,
            });
        }
        if raw.len() > MAX_LEN {
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: raw.len(),
                max: MAX_LEN,
            });
        }
        if !raw.bytes().all(is_swift_x) {
            return Err(DecodeError::InvalidCharset {
                tag: TAG,
                value: raw.to_string(),
            });
        }
        Ok(FieldSemantic::Reference(raw.to_string()))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn accepts_typical_reference() {
        let out = Field20.decode("REFERENCE001").unwrap();
        assert_eq!(out, FieldSemantic::Reference("REFERENCE001".to_string()));
    }

    #[test]
    fn accepts_max_length_with_punctuation() {
        let out = Field20.decode("REF/2026-05-21AB").unwrap();
        assert_eq!(
            out,
            FieldSemantic::Reference("REF/2026-05-21AB".to_string())
        );
    }

    #[test]
    fn rejects_too_long() {
        let err = Field20.decode("THIS_IS_DEFINITELY_TOO_LONG").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidLength { tag: "20", .. }));
    }

    #[test]
    fn rejects_lowercase() {
        // SWIFT X is uppercase-only; lowercase is reserved for the Y set.
        let err = Field20.decode("ref001").unwrap_err();
        assert!(matches!(err, DecodeError::InvalidCharset { tag: "20", .. }));
    }

    #[test]
    fn rejects_empty_value() {
        let err = Field20.decode("").unwrap_err();
        assert!(matches!(
            err,
            DecodeError::InvalidLength {
                tag: "20",
                got: 0,
                ..
            }
        ));
    }
}
