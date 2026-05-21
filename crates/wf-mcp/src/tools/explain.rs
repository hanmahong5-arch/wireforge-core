//! `wf_explain_message` — natural-language description of a message,
//! assembled purely from field-table metadata. Calls NO external LLM —
//! the calling agent does its own reasoning on top of these facts.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use wf_codec::iso8583::{
    field::{field_def, DataType, LengthSpec},
    parse,
};

use crate::hex;
use crate::tools::mti;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Hex-encoded ISO 8583 wire bytes.
    pub hex: String,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let bytes = hex::decode(&hex::strip_whitespace(&req.hex))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {e}"))?;
    let mti_str = String::from_utf8_lossy(&msg.mti).into_owned();
    let mti_decoded = mti::decode(&mti_str).unwrap_or(Value::Null);

    let summary = format!(
        "{}-byte ISO 8583 message, MTI {} ({} / {}), {} field(s) set",
        bytes.len(),
        mti_str,
        mti_decoded["class"]["label"].as_str().unwrap_or("?"),
        mti_decoded["function"]["label"].as_str().unwrap_or("?"),
        msg.fields.len(),
    );

    let mut fields_explained = Vec::with_capacity(msg.fields.len());
    for (number, data) in &msg.fields {
        fields_explained.push(explain_field(*number, data));
    }

    Ok(serde_json::json!({
        "summary": summary,
        "mti_decoded": mti_decoded,
        "bytes_total": bytes.len(),
        "fields_explained": fields_explained,
    }))
}

fn explain_field(number: u8, data: &[u8]) -> Value {
    let def = field_def(number);
    let name = def.map_or("Unknown", |d| d.name);
    let kind = def.map_or("?", |d| match d.data_type {
        DataType::Numeric => "numeric",
        DataType::Alpha => "alpha",
        DataType::Special => "special",
        DataType::AlphaNumeric => "alpha-numeric",
        DataType::AlphaSpecial => "alpha-special",
        DataType::NumericSpecial => "numeric-special",
        DataType::AlphaNumericSpecial => "alpha-numeric-special",
        DataType::Binary => "binary",
        DataType::Track => "track-data",
    });
    let length_note = def.map_or_else(String::new, |d| match d.length {
        LengthSpec::Fixed(n) => format!("fixed {n}"),
        LengthSpec::LLVAR { max } => format!("LLVAR up to {max}"),
        LengthSpec::LLLVAR { max } => format!("LLLVAR up to {max}"),
    });
    let printable = data.iter().all(|b| b.is_ascii_graphic() || *b == b' ');
    let preview = if printable {
        String::from_utf8_lossy(data).into_owned()
    } else {
        format!("hex:{}", hex::encode(data))
    };
    let sentence = format!(
        "Field {number} ({name}) is a {kind} value, {length_note}, {} byte(s): {preview}",
        data.len()
    );
    serde_json::json!({
        "number": number,
        "name": name,
        "sentence": sentence,
    })
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
    fn explains_minimal() {
        let v = handle(req(SAMPLE)).unwrap();
        assert!(v["summary"].as_str().unwrap().contains("MTI 0200"));
        assert!(v["summary"].as_str().unwrap().contains("Financial"));
        let f0 = &v["fields_explained"][0];
        assert_eq!(f0["number"], 3);
        assert!(f0["sentence"].as_str().unwrap().contains("Processing Code"));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(handle(req("zz")).is_err());
    }
}
