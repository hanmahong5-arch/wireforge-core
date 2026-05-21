//! Tag 50K — Ordering Customer (Party Identifier, K-option).
//!
//! Spec: `[/34x]4*35x`
//!
//! - `[/34x]` — optional first line starting with `/` followed by up to
//!   34 chars (account number / IBAN). The leading `/` is structural;
//!   the `account` field in [`FieldSemantic::Party`] holds the substring
//!   after the slash.
//! - `4*35x` — 1 to 4 name & address lines, each up to 35 chars in the
//!   SWIFT X charset.
//!
//! Lines are CRLF-separated in the wire format; the structural parser
//! preserves the embedded CRLF inside [`super::super::MtField::value`],
//! so this decoder splits on `\r\n` (with a `\n` fallback for inputs
//! that were normalised upstream).

use super::{is_swift_x, DecodeError, FieldSemantic, MtFieldDecoder};

const TAG: &str = "50K";
const ACCOUNT_MAX_LEN: usize = 34;
const NAME_LINE_MAX_LEN: usize = 35;
const MAX_NAME_LINES: usize = 4;

/// Zero-sized decoder for tag 50K.
#[derive(Debug, Clone, Copy, Default)]
pub struct Field50K;

impl MtFieldDecoder for Field50K {
    fn tag(&self) -> &'static str {
        TAG
    }

    fn decode(&self, raw: &str) -> Result<FieldSemantic, DecodeError> {
        if raw.is_empty() {
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: 0,
                max: ACCOUNT_MAX_LEN + MAX_NAME_LINES * NAME_LINE_MAX_LEN,
            });
        }
        let lines: Vec<&str> = split_lines(raw);
        if lines.is_empty() {
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: 0,
                max: ACCOUNT_MAX_LEN + MAX_NAME_LINES * NAME_LINE_MAX_LEN,
            });
        }
        let (account, name_lines) =
            if let Some(first) = lines.first().and_then(|l| l.strip_prefix('/')) {
                if first.len() > ACCOUNT_MAX_LEN {
                    return Err(DecodeError::InvalidLength {
                        tag: TAG,
                        got: first.len(),
                        max: ACCOUNT_MAX_LEN,
                    });
                }
                (Some(first.to_string()), &lines[1..])
            } else {
                (None, &lines[..])
            };
        if name_lines.len() > MAX_NAME_LINES {
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: name_lines.len(),
                max: MAX_NAME_LINES,
            });
        }
        for line in name_lines {
            if line.len() > NAME_LINE_MAX_LEN {
                return Err(DecodeError::InvalidLength {
                    tag: TAG,
                    got: line.len(),
                    max: NAME_LINE_MAX_LEN,
                });
            }
            if !line.bytes().all(is_swift_x) {
                return Err(DecodeError::InvalidCharset {
                    tag: TAG,
                    value: (*line).to_string(),
                });
            }
        }
        if account.is_none() && name_lines.is_empty() {
            // No account, no name — would mean the field is just blank
            // after stripping line breaks. Reject as a length error so
            // the caller sees an unambiguous "empty payload" signal.
            return Err(DecodeError::InvalidLength {
                tag: TAG,
                got: 0,
                max: ACCOUNT_MAX_LEN + MAX_NAME_LINES * NAME_LINE_MAX_LEN,
            });
        }
        Ok(FieldSemantic::Party {
            account,
            lines: name_lines.iter().map(|s| (*s).to_string()).collect(),
        })
    }
}

/// Split on `\r\n` (canonical SWIFT line break) with a `\n` fallback for
/// inputs whose line endings were normalised upstream. Empty trailing
/// segments are discarded so a trailing CRLF before the block terminator
/// doesn't manifest as a phantom blank line.
fn split_lines(raw: &str) -> Vec<&str> {
    let mut out: Vec<&str> = if raw.contains('\r') {
        raw.split("\r\n").collect()
    } else {
        raw.split('\n').collect()
    };
    while out.last() == Some(&"") {
        out.pop();
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn accepts_account_plus_two_name_lines() {
        let raw = "/12345678\r\nACME CORP\r\n123 MAIN ST";
        let out = Field50K.decode(raw).unwrap();
        match out {
            FieldSemantic::Party { account, lines } => {
                assert_eq!(account.as_deref(), Some("12345678"));
                assert_eq!(
                    lines,
                    vec!["ACME CORP".to_string(), "123 MAIN ST".to_string()]
                );
            }
            other => panic!("expected Party, got {other:?}"),
        }
    }

    #[test]
    fn accepts_lf_only_line_breaks() {
        let raw = "/12345678\nACME CORP";
        let out = Field50K.decode(raw).unwrap();
        assert!(matches!(out, FieldSemantic::Party { .. }));
    }

    #[test]
    fn accepts_name_only_no_account() {
        let raw = "ACME CORP\r\n123 MAIN ST";
        let out = Field50K.decode(raw).unwrap();
        match out {
            FieldSemantic::Party { account, lines } => {
                assert!(account.is_none());
                assert_eq!(lines.len(), 2);
            }
            other => panic!("expected Party, got {other:?}"),
        }
    }

    #[test]
    fn rejects_more_than_four_name_lines() {
        let raw = "ACME\r\nLINE1\r\nLINE2\r\nLINE3\r\nLINE4";
        let err = Field50K.decode(raw).unwrap_err();
        assert!(matches!(err, DecodeError::InvalidLength { tag: "50K", .. }));
    }
}
