//! `wf_decode_mti` — 4-digit MTI → {version, class, function, origin}.
//!
//! Per ISO 8583-1987/1993/2003 the MTI is a 4-digit code where each
//! position carries a semantic label. The tables below are taken from
//! the published spec; we list all standard codes and mark reserved /
//! future-use slots explicitly.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Four ASCII digits, e.g. "0200".
    pub mti: String,
}

pub fn handle(req: Request) -> Result<Value, String> {
    decode(&req.mti)
}

pub fn decode(mti: &str) -> Result<Value, String> {
    if mti.len() != 4 || !mti.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("mti must be 4 ASCII digits, got {mti:?}"));
    }
    let b = mti.as_bytes();
    let v = label(VERSIONS, b[0]);
    let c = label(CLASSES, b[1]);
    let f = label(FUNCTIONS, b[2]);
    let o = label(ORIGINS, b[3]);
    Ok(serde_json::json!({
        "mti": mti,
        "version": { "digit": (b[0] - b'0'), "label": v },
        "class":   { "digit": (b[1] - b'0'), "label": c },
        "function":{ "digit": (b[2] - b'0'), "label": f },
        "origin":  { "digit": (b[3] - b'0'), "label": o },
    }))
}

fn label(table: &[(u8, &'static str)], digit: u8) -> &'static str {
    table
        .iter()
        .find(|(d, _)| *d == digit)
        .map_or("Reserved", |(_, l)| *l)
}

const VERSIONS: &[(u8, &str)] = &[
    (b'0', "ISO 8583-1:1987"),
    (b'1', "ISO 8583-2:1993"),
    (b'2', "ISO 8583-3:2003"),
    (b'8', "National use"),
    (b'9', "Private use"),
];

const CLASSES: &[(u8, &str)] = &[
    (b'1', "Authorization"),
    (b'2', "Financial"),
    (b'3', "File actions"),
    (b'4', "Reversal / Chargeback"),
    (b'5', "Reconciliation"),
    (b'6', "Administrative"),
    (b'7', "Fee collection"),
    (b'8', "Network management"),
];

const FUNCTIONS: &[(u8, &str)] = &[
    (b'0', "Request"),
    (b'1', "Request response"),
    (b'2', "Advice"),
    (b'3', "Advice response"),
    (b'4', "Notification"),
    (b'5', "Notification acknowledgement"),
    (b'6', "Instruction"),
    (b'7', "Instruction acknowledgement"),
];

const ORIGINS: &[(u8, &str)] = &[
    (b'0', "Acquirer"),
    (b'1', "Acquirer repeat"),
    (b'2', "Issuer"),
    (b'3', "Issuer repeat"),
    (b'4', "Other"),
    (b'5', "Other repeat"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_0200() {
        let v = decode("0200").unwrap();
        assert_eq!(v["version"]["label"], "ISO 8583-1:1987");
        assert_eq!(v["class"]["label"], "Financial");
        assert_eq!(v["function"]["label"], "Request");
        assert_eq!(v["origin"]["label"], "Acquirer");
    }

    #[test]
    fn decodes_1110() {
        let v = decode("1110").unwrap();
        assert_eq!(v["version"]["label"], "ISO 8583-2:1993");
        assert_eq!(v["class"]["label"], "Authorization");
        assert_eq!(v["function"]["label"], "Request response");
    }

    #[test]
    fn rejects_short_mti() {
        assert!(decode("020").is_err());
    }

    #[test]
    fn marks_unknown_reserved() {
        // class 9 not in table
        let v = decode("0900").unwrap();
        assert_eq!(v["class"]["label"], "Reserved");
    }
}
