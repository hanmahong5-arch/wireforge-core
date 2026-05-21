//! `wf_field_lookup` — field number → FieldDef (name + spec).

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use wf_codec::iso8583::field::{field_def, DataType, LengthSpec};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Field number, 1..=128.
    pub number: u8,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let def = field_def(req.number)
        .ok_or_else(|| format!("no definition for field {} (valid: 1..=128)", req.number))?;
    let (length_kind, length_value) = match def.length {
        LengthSpec::Fixed(v) => ("fixed", v),
        LengthSpec::LLVAR { max } => ("llvar", max),
        LengthSpec::LLLVAR { max } => ("lllvar", max),
    };
    Ok(serde_json::json!({
        "number": def.number,
        "name": def.name,
        "data_type": data_type_name(def.data_type),
        "length_kind": length_kind,
        "length_value": length_value,
    }))
}

fn data_type_name(t: DataType) -> &'static str {
    match t {
        DataType::Numeric => "numeric",
        DataType::Alpha => "alpha",
        DataType::Special => "special",
        DataType::AlphaNumeric => "alpha_numeric",
        DataType::AlphaSpecial => "alpha_special",
        DataType::NumericSpecial => "numeric_special",
        DataType::AlphaNumericSpecial => "alpha_numeric_special",
        DataType::Binary => "binary",
        DataType::Track => "track",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_pan() {
        let v = handle(Request { number: 2 }).unwrap();
        assert_eq!(v["name"], "Primary Account Number");
        assert_eq!(v["length_kind"], "llvar");
        assert_eq!(v["length_value"], 19);
    }

    #[test]
    fn rejects_field_0() {
        assert!(handle(Request { number: 0 }).is_err());
    }

    #[test]
    fn handles_reserved_field() {
        let v = handle(Request { number: 110 }).unwrap();
        assert_eq!(v["name"], "Reserved");
    }
}
