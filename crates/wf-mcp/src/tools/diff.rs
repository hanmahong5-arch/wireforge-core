//! `wf_roundtrip_check` — parse → build → compare original bytes.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use wf_codec::iso8583::{build, parse};

use crate::hex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Hex-encoded ISO 8583 wire bytes.
    pub hex: String,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let bytes = hex::decode(&hex::strip_whitespace(&req.hex))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {e}"))?;
    let rebuilt = build(&msg).map_err(|e| format!("build: {e}"))?;
    if rebuilt == bytes {
        Ok(serde_json::json!({
            "ok": true,
            "input_bytes": bytes.len(),
            "diff": Value::Null,
        }))
    } else {
        let diffs = first_diffs(&bytes, &rebuilt, 8);
        Ok(serde_json::json!({
            "ok": false,
            "input_bytes": bytes.len(),
            "rebuilt_bytes": rebuilt.len(),
            "diff": {
                "first_differences": diffs,
                "input_hex":  hex::encode(&bytes),
                "rebuilt_hex": hex::encode(&rebuilt),
            },
        }))
    }
}

fn first_diffs(a: &[u8], b: &[u8], limit: usize) -> Vec<Value> {
    let mut out = Vec::new();
    let common = a.len().min(b.len());
    for i in 0..common {
        if a[i] != b[i] {
            out.push(serde_json::json!({
                "offset": i,
                "input":   format!("{:02x}", a[i]),
                "rebuilt": format!("{:02x}", b[i]),
            }));
            if out.len() >= limit {
                return out;
            }
        }
    }
    if a.len() != b.len() {
        out.push(serde_json::json!({
            "length_mismatch": {
                "input":   a.len(),
                "rebuilt": b.len(),
            }
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "303230302000000000000000303030303030";

    fn req(hex: &str) -> Request {
        Request {
            hex: hex.to_string(),
        }
    }

    #[test]
    fn round_trips_clean() {
        let v = handle(req(SAMPLE)).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["input_bytes"], 18);
    }

    #[test]
    fn rejects_unparseable() {
        assert!(handle(req("deadbeef")).is_err());
    }
}
