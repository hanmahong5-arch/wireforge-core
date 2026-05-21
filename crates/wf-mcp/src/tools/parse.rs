//! `wf_parse_iso8583` — hex string → structured field tree.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wf_bitmap::Bitmap8583;
use wf_codec::iso8583::{
    field::{field_def, DataType, FieldDef, LengthSpec},
    parse, Iso8583Message,
};

use crate::hex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Hex-encoded ISO 8583 wire bytes. Whitespace is stripped.
    pub hex: String,
}

#[derive(Debug, Serialize)]
struct MessageView {
    mti: String,
    bitmap_hex: String,
    has_secondary: bool,
    fields: Vec<FieldView>,
}

#[derive(Debug, Serialize)]
struct FieldView {
    number: u8,
    name: &'static str,
    type_desc: String,
    length: usize,
    value_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    value_ascii: Option<String>,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let bytes = hex::decode(&hex::strip_whitespace(&req.hex))?;
    let msg = parse(&bytes).map_err(|e| format!("parse: {e}"))?;
    let view = build_view(&msg)?;
    serde_json::to_value(&view).map_err(|e| format!("serialize: {e}"))
}

fn build_view(msg: &Iso8583Message) -> Result<MessageView, String> {
    let bitmap_hex = encode_bitmap(msg)?;
    let has_secondary = msg.fields.keys().any(|n| *n > 64);
    let mut fields = Vec::with_capacity(msg.fields.len());
    for (number, data) in &msg.fields {
        fields.push(field_view(*number, data));
    }
    Ok(MessageView {
        mti: String::from_utf8_lossy(&msg.mti).into_owned(),
        bitmap_hex,
        has_secondary,
        fields,
    })
}

fn field_view(number: u8, data: &[u8]) -> FieldView {
    let def = field_def(number);
    let name = def.map_or("Unknown", |d| d.name);
    let type_desc = def.map_or_else(|| "?".to_string(), describe);
    let value_ascii = if data.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        Some(String::from_utf8_lossy(data).into_owned())
    } else {
        None
    };
    FieldView {
        number,
        name,
        type_desc,
        length: data.len(),
        value_hex: hex::encode(data),
        value_ascii,
    }
}

fn describe(def: &FieldDef) -> String {
    let dt = data_type_short(def.data_type);
    match def.length {
        LengthSpec::Fixed(n) => format!("{dt}{n} fixed"),
        LengthSpec::LLVAR { max } => format!("LLVAR {dt}..{max}"),
        LengthSpec::LLLVAR { max } => format!("LLLVAR {dt}..{max}"),
    }
}

fn data_type_short(t: DataType) -> &'static str {
    match t {
        DataType::Numeric => "n",
        DataType::Alpha => "a",
        DataType::Special => "s",
        DataType::AlphaNumeric => "an",
        DataType::AlphaSpecial => "as",
        DataType::NumericSpecial => "ns",
        DataType::AlphaNumericSpecial => "ans",
        DataType::Binary => "b",
        DataType::Track => "z",
    }
}

fn encode_bitmap(msg: &Iso8583Message) -> Result<String, String> {
    let mut bm = Bitmap8583::new();
    for n in msg.fields.keys() {
        bm.set(u16::from(*n))
            .map_err(|e| format!("bitmap set field {n}: {e:?}"))?;
    }
    Ok(hex::encode(&bm.encode()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // MTI "0200" (8 hex) + primary bitmap field 3 set (16 hex) + field 3 ASCII
    // "000000" (12 hex) = 36 hex / 18 bytes. bitmap byte 0 = 0b0010_0000 = 0x20.
    const SAMPLE: &str = "303230302000000000000000303030303030";

    fn req(hex: &str) -> Request {
        Request {
            hex: hex.to_string(),
        }
    }

    #[test]
    fn parses_minimal() {
        let v = handle(req(SAMPLE)).unwrap();
        assert_eq!(v["mti"], "0200");
        assert_eq!(v["has_secondary"], false);
        assert_eq!(v["fields"][0]["number"], 3);
        assert_eq!(v["fields"][0]["name"], "Processing Code");
    }

    #[test]
    fn rejects_garbage() {
        assert!(handle(req("zz")).is_err());
    }

    #[test]
    fn tolerates_whitespace() {
        let spaced: String = SAMPLE
            .chars()
            .enumerate()
            .flat_map(|(i, c)| {
                if i.is_multiple_of(2) && i > 0 {
                    vec![' ', c]
                } else {
                    vec![c]
                }
            })
            .collect();
        assert!(handle(req(&spaced)).is_ok());
    }
}
