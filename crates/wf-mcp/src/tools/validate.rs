//! `wf_validate_iso8583` — structural validation only.
//!
//! LIMITATION (callers must surface this): this tool only checks WIRE
//! STRUCTURE. It does NOT validate semantics such as PAN Luhn checksum,
//! amount-field digit-only contents, currency-code ISO 4217 membership,
//! or MAC integrity. Pass-through here does not imply the message is
//! semantically correct.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use wf_codec::iso8583::parse;

use crate::hex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Hex-encoded ISO 8583 wire bytes.
    pub hex: String,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let bytes = match hex::decode(&hex::strip_whitespace(&req.hex)) {
        Ok(b) => b,
        Err(e) => {
            return Ok(serde_json::json!({
                "valid": false,
                "errors": [format!("hex decode: {e}")],
                "checks": ["wire_structure"],
                "limitations": LIMITATIONS,
            }));
        }
    };
    match parse(&bytes) {
        Ok(_) => Ok(serde_json::json!({
            "valid": true,
            "errors": [],
            "checks": ["wire_structure"],
            "limitations": LIMITATIONS,
        })),
        Err(e) => Ok(serde_json::json!({
            "valid": false,
            "errors": [format!("{e}")],
            "checks": ["wire_structure"],
            "limitations": LIMITATIONS,
        })),
    }
}

const LIMITATIONS: &[&str] = &[
    "PAN Luhn checksum NOT verified",
    "Numeric/Alpha field charset NOT enforced",
    "Currency ISO 4217 membership NOT checked",
    "MAC / PIN block integrity NOT verified",
];

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "303230302000000000000000303030303030";

    fn req(hex: &str) -> Request {
        Request {
            hex: hex.to_string(),
        }
    }

    #[test]
    fn valid_message() {
        let v = handle(req(VALID)).unwrap();
        assert_eq!(v["valid"], true);
        assert!(v["limitations"].as_array().unwrap().len() >= 3);
    }

    #[test]
    fn invalid_message_returns_ok_with_errors() {
        let v = handle(req("deadbeef")).unwrap();
        assert_eq!(v["valid"], false);
        assert!(!v["errors"].as_array().unwrap().is_empty());
    }

    #[test]
    fn bad_hex_returns_ok_with_errors() {
        let v = handle(req("zz")).unwrap();
        assert_eq!(v["valid"], false);
    }
}
