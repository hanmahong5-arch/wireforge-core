//! `wf_parse_swift_mt` — SWIFT MT wire text → structured block tree.
//!
//! Companion to the ISO 8583 parse tool, exposing the structural layer of
//! `wf_codec::swift` via MCP so AI agents can introspect SWIFT MT messages
//! without leaving the chat loop.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wf_codec::swift::{parse, Block, MtMessage};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct Request {
    /// Raw on-the-wire SWIFT MT text, including the `{1:…}{2:…}…` block
    /// wrappers. CRLF and LF line endings are both accepted in block 4.
    pub wire: String,
}

#[derive(Debug, Serialize)]
struct MessageView {
    blocks: Vec<BlockView>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum BlockView {
    Raw { id: u8, value: String },
    Text { id: u8, fields: Vec<FieldView> },
    Tagged { id: u8, entries: Vec<EntryView> },
}

#[derive(Debug, Serialize)]
struct FieldView {
    tag: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct EntryView {
    tag: String,
    value: String,
}

pub fn handle(req: Request) -> Result<Value, String> {
    let msg = parse(&req.wire).map_err(|e| format!("parse: {e}"))?;
    let view = render(&msg);
    serde_json::to_value(&view).map_err(|e| format!("serialize: {e}"))
}

fn render(msg: &MtMessage) -> MessageView {
    let blocks = msg
        .blocks
        .iter()
        .map(|(id, block)| match block {
            Block::Raw(s) => BlockView::Raw {
                id: *id,
                value: s.clone(),
            },
            Block::Text(fields) => BlockView::Text {
                id: *id,
                fields: fields
                    .iter()
                    .map(|f| FieldView {
                        tag: f.tag.clone(),
                        value: f.value.clone(),
                    })
                    .collect(),
            },
            Block::Tagged(subs) => BlockView::Tagged {
                id: *id,
                entries: subs
                    .iter()
                    .map(|s| EntryView {
                        tag: s.tag.clone(),
                        value: s.value.clone(),
                    })
                    .collect(),
            },
        })
        .collect();
    MessageView { blocks }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MT103_SKELETON: &str =
        "{1:F01BANKBICAA1234567890}{2:I103RECVBIC22N}{4:\r\n:20:REF001\r\n:32A:240520USD1000,00\r\n-}";

    #[test]
    fn parses_mt103_skeleton() {
        let v = handle(Request {
            wire: MT103_SKELETON.to_string(),
        })
        .unwrap();
        let blocks = v["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3);
        // Block 1 = Raw
        assert_eq!(blocks[0]["kind"], "raw");
        assert_eq!(blocks[0]["id"], 1);
        // Block 4 = Text with two fields
        assert_eq!(blocks[2]["kind"], "text");
        assert_eq!(blocks[2]["id"], 4);
        assert_eq!(blocks[2]["fields"][0]["tag"], "20");
        assert_eq!(blocks[2]["fields"][0]["value"], "REF001");
        assert_eq!(blocks[2]["fields"][1]["tag"], "32A");
    }

    #[test]
    fn rejects_unbalanced_brace() {
        let err = handle(Request {
            wire: "{1:no-close".to_string(),
        })
        .unwrap_err();
        assert!(err.contains("unbalanced"), "got: {err}");
    }
}
