//! `wf_build_iso8583` — `{mti, fields}` → hex wire string.
//!
//! Field value convention: a plain string is interpreted as ASCII bytes.
//! Prefix `"hex:"` to send raw binary bytes (e.g. PIN block, MAC). Both
//! forms support fields of any wf-codec-defined length.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use wf_codec::iso8583::{build, Iso8583Message};

use crate::hex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Four ASCII digits, e.g. "0200".
    pub mti: String,
    /// Field number (as JSON string key, "1".."128") → payload.
    /// Plain string = ASCII; prefix "hex:" = raw binary bytes.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

pub fn handle(req: Request) -> Result<Value, String> {
    if req.mti.len() != 4 || !req.mti.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "mti must be exactly 4 ASCII digits, got {:?}",
            req.mti
        ));
    }
    let mut mti = [0u8; 4];
    mti.copy_from_slice(req.mti.as_bytes());
    let mut fields = BTreeMap::new();
    for (key, value) in req.fields {
        let n: u8 = key
            .parse()
            .map_err(|_| format!("field key {key:?} is not a valid u8 (1..=128)"))?;
        let bytes = if let Some(rest) = value.strip_prefix("hex:") {
            hex::decode(rest).map_err(|e| format!("field {n} hex: {e}"))?
        } else {
            value.into_bytes()
        };
        fields.insert(n, bytes);
    }
    let msg = Iso8583Message { mti, fields };
    let wire = build(&msg).map_err(|e| format!("build: {e}"))?;
    Ok(serde_json::json!({ "hex": hex::encode(&wire) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(json: serde_json::Value) -> Request {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn builds_field_3() {
        let v = handle(req(serde_json::json!({
            "mti": "0200",
            "fields": { "3": "000000" }
        })))
        .unwrap();
        let h = v["hex"].as_str().unwrap();
        assert!(h.starts_with("30323030")); // ASCII "0200"
        assert_eq!(h.len(), 36); // 4 + 8 + 6 bytes = 18 bytes = 36 hex
    }

    #[test]
    fn rejects_short_mti() {
        let r = Request {
            mti: "020".to_string(),
            fields: BTreeMap::new(),
        };
        assert!(handle(r).is_err());
    }

    #[test]
    fn accepts_hex_binary_field() {
        let v = handle(req(serde_json::json!({
            "mti": "0200",
            "fields": { "52": "hex:0102030405060708" }
        })))
        .unwrap();
        assert!(v["hex"].as_str().unwrap().starts_with("30323030"));
    }

    #[test]
    fn rejects_non_numeric_field_key() {
        let r = Request {
            mti: "0200".to_string(),
            fields: {
                let mut m = BTreeMap::new();
                m.insert("foo".to_string(), "bar".to_string());
                m
            },
        };
        assert!(handle(r).is_err());
    }
}
