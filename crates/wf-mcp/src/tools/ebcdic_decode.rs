//! `wf_ebcdic_decode` tool: decode an EBCDIC hex dump to Unicode text.
//!
//! The natural agent tool for inspecting a mainframe dump: paste the hex
//! bytes, get back the readable text plus which code page was applied.

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};
use wf_codec::ebcdic::{self, CodePage};

use crate::hex;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Hex-encoded EBCDIC bytes (whitespace tolerated). Example: "C1C2C3".
    pub hex: String,
    /// EBCDIC code page to decode with: "cp037" (default) or "cp500".
    #[serde(default)]
    pub code_page: Option<String>,
}

fn parse_code_page(raw: Option<&str>) -> Result<CodePage, String> {
    match raw {
        None => Ok(CodePage::Cp037),
        Some(s) => match s.trim().to_ascii_lowercase().as_str() {
            "cp037" | "037" => Ok(CodePage::Cp037),
            "cp500" | "500" => Ok(CodePage::Cp500),
            other => Err(format!(
                "unknown code_page {other:?} — expected \"cp037\" or \"cp500\""
            )),
        },
    }
}

fn code_page_label(cp: CodePage) -> &'static str {
    match cp {
        CodePage::Cp037 => "cp037",
        CodePage::Cp500 => "cp500",
    }
}

pub fn handle(req: Request) -> Result<Value, String> {
    let cp = parse_code_page(req.code_page.as_deref())?;
    let cleaned = hex::strip_whitespace(&req.hex);
    let bytes = hex::decode(&cleaned)?;
    let text = ebcdic::decode(&bytes, cp);
    Ok(json!({
        "text": text,
        "code_page": code_page_label(cp),
        "byte_count": bytes.len(),
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn decodes_c1c2c3_to_abc() {
        // External fact: EBCDIC 0xC1/0xC2/0xC3 are 'A'/'B'/'C' in cp037.
        let req = Request {
            hex: "C1C2C3".to_string(),
            code_page: None,
        };
        let v = handle(req).unwrap();
        assert_eq!(v["text"], "ABC");
        assert_eq!(v["code_page"], "cp037");
        assert_eq!(v["byte_count"], 3);
    }

    #[test]
    fn tolerates_whitespace_and_explicit_cp500() {
        let req = Request {
            hex: "C1 C2 C3".to_string(),
            code_page: Some("cp500".to_string()),
        };
        let v = handle(req).unwrap();
        assert_eq!(v["text"], "ABC");
        assert_eq!(v["code_page"], "cp500");
        assert_eq!(v["byte_count"], 3);
    }

    #[test]
    fn rejects_unknown_code_page() {
        let req = Request {
            hex: "C1".to_string(),
            code_page: Some("ascii".to_string()),
        };
        assert!(handle(req).is_err());
    }

    #[test]
    fn rejects_bad_hex() {
        let req = Request {
            hex: "C1C".to_string(),
            code_page: None,
        };
        assert!(handle(req).is_err());
    }
}
