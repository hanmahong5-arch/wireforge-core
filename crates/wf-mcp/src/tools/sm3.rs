//! `wf_sm3` tool: GM/T 0004-2012 SM3 hash (functional) of bytes or text.
//!
//! Hashes either hex-decoded bytes or a UTF-8 text string. Exactly one of
//! `hex` / `text` must be supplied. This is a functional hash only; it makes
//! no compliance / certification claim.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use wf_sm::sm3::sm3_hex;

use crate::hex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Hex-encoded input bytes (whitespace tolerated). Mutually exclusive with `text`.
    #[serde(default)]
    pub hex: Option<String>,
    /// UTF-8 text input. Mutually exclusive with `hex`.
    #[serde(default)]
    pub text: Option<String>,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let (bytes, kind): (Vec<u8>, &str) = match (req.hex, req.text) {
        (Some(_), Some(_)) => {
            return Err("provide exactly one of `hex` or `text`, not both".to_string())
        }
        (None, None) => return Err("provide exactly one of `hex` or `text`".to_string()),
        (Some(h), None) => {
            let cleaned = hex::strip_whitespace(&h);
            (hex::decode(&cleaned)?, "hex")
        }
        (None, Some(t)) => (t.into_bytes(), "text"),
    };

    Ok(json!({
        "sm3_hex": sm3_hex(&bytes),
        "input_kind": kind,
        "input_len": bytes.len(),
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// External standards anchor: the SM3 of ASCII "abc" is the published
    /// GM/T 0004-2012 test vector.
    const SM3_ABC: &str = "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0";

    #[test]
    fn text_abc_matches_known_vector() {
        let req = Request {
            hex: None,
            text: Some("abc".to_string()),
        };
        let v = handle(req).unwrap();
        assert_eq!(v["sm3_hex"], SM3_ABC);
        assert_eq!(v["input_kind"], "text");
        assert_eq!(v["input_len"], 3);
    }

    #[test]
    fn hex_of_abc_bytes_matches_same_vector() {
        // 0x61 0x62 0x63 == "abc"; hex and text paths must agree.
        let req = Request {
            hex: Some("616263".to_string()),
            text: None,
        };
        let v = handle(req).unwrap();
        assert_eq!(v["sm3_hex"], SM3_ABC);
        assert_eq!(v["input_kind"], "hex");
        assert_eq!(v["input_len"], 3);
    }

    #[test]
    fn rejects_both_inputs() {
        let req = Request {
            hex: Some("616263".to_string()),
            text: Some("abc".to_string()),
        };
        assert!(handle(req).is_err());
    }

    #[test]
    fn rejects_neither_input() {
        let req = Request {
            hex: None,
            text: None,
        };
        assert!(handle(req).is_err());
    }
}
