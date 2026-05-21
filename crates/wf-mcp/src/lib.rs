//! Wireforge Model Context Protocol server.
//!
//! Exposes ISO 8583 + SWIFT MT tools over MCP stdio transport so AI agents
//! (Claude Code, Cursor, hermes-agent, etc.) can parse, build, and
//! validate financial messages without leaving the agent loop.
//!
//! Tools:
//! - `wf_parse_iso8583`    — hex → structured field tree
//! - `wf_build_iso8583`    — `{mti, fields}` → hex
//! - `wf_validate_iso8583` — structural validation
//! - `wf_field_lookup`     — field number → FieldDef
//! - `wf_decode_mti`       — 4-digit MTI → semantic parts
//! - `wf_explain_message`  — natural-language description (no LLM)
//! - `wf_roundtrip_check`  — parse → build → byte-compare
//! - `wf_parse_swift_mt`   — SWIFT MT wire text → structured block tree
//!
//! ## Why this crate relaxes three workspace lints
//!
//! `rmcp` 0.16's `#[tool_router]` procedural macro generates code that
//! contains `unwrap` / `panic` paths we don't control. The workspace
//! `unwrap_used = "deny"` / `expect_used = "deny"` / `panic = "deny"`
//! lints would block compilation. We relax them in `Cargo.toml` so the
//! macro can expand. All hand-written handler code in this crate still
//! goes through `Result<_, String>`; errors travel out as the MCP
//! `isError: true` CallToolResult branch via rmcp's `IntoCallToolResult`
//! impl for `Result<T: IntoContents, E: IntoContents>`.

pub mod hex;
pub mod tools;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ServerHandler,
};

/// JSON-string success, JSON-string error. rmcp turns this into the MCP
/// `CallToolResult` envelope (success vs `isError: true`) automatically.
type ToolOut = Result<String, String>;

#[derive(Clone, Debug)]
pub struct WireforgeServer {
    tool_router: ToolRouter<Self>,
}

impl Default for WireforgeServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl WireforgeServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Parse a hex-encoded ISO 8583 message into a structured field tree. \
        Whitespace in the hex is tolerated. Returns MTI, bitmap, and an array of \
        decoded fields with their spec name and type."
    )]
    fn wf_parse_iso8583(&self, Parameters(req): Parameters<tools::parse::Request>) -> ToolOut {
        json_string(tools::parse::handle(req))
    }

    #[tool(
        description = "Build an ISO 8583 wire message (returned as a hex string) from \
        an MTI plus a map of field-number-to-payload. Plain string payloads are treated \
        as ASCII; prefix a payload with \"hex:\" to send raw binary bytes (e.g. PIN \
        block, MAC)."
    )]
    fn wf_build_iso8583(&self, Parameters(req): Parameters<tools::build::Request>) -> ToolOut {
        json_string(tools::build::handle(req))
    }

    #[tool(
        description = "Structural validation of a hex-encoded ISO 8583 message. \
        NOTE: only checks wire structure (MTI digits, bitmap consistency, field \
        length envelopes). Does NOT verify PAN Luhn checksum, numeric/alpha field \
        charsets, currency code membership, or MAC integrity. The response includes \
        an explicit \"limitations\" list — surface it to the user."
    )]
    fn wf_validate_iso8583(
        &self,
        Parameters(req): Parameters<tools::validate::Request>,
    ) -> ToolOut {
        json_string(tools::validate::handle(req))
    }

    #[tool(
        description = "Look up an ISO 8583 field's spec metadata by field number \
        (1..=128). Returns name, data type, length kind (fixed/llvar/lllvar), and \
        length value."
    )]
    fn wf_field_lookup(&self, Parameters(req): Parameters<tools::field::Request>) -> ToolOut {
        json_string(tools::field::handle(req))
    }

    #[tool(
        description = "Decode a 4-digit ISO 8583 MTI (e.g. \"0200\") into its four \
        semantic positions: version (1987/1993/2003/national/private), class \
        (authorization/financial/...), function (request/response/advice/...), and \
        origin (acquirer/issuer/...)."
    )]
    fn wf_decode_mti(&self, Parameters(req): Parameters<tools::mti::Request>) -> ToolOut {
        json_string(tools::mti::handle(req))
    }

    #[tool(description = "Produce a natural-language description of an ISO 8583 \
        message: a one-line summary plus a per-field explanation built from the \
        official field-name table. This tool performs NO LLM call; it returns \
        structured facts for the calling agent to reason over.")]
    fn wf_explain_message(&self, Parameters(req): Parameters<tools::explain::Request>) -> ToolOut {
        json_string(tools::explain::handle(req))
    }

    #[tool(
        description = "Parse a hex-encoded ISO 8583 message and re-build it, then \
        byte-compare the result against the original. Useful to confirm a message is \
        canonical (parser and builder are exact inverses). Reports byte-level \
        differences on mismatch."
    )]
    fn wf_roundtrip_check(&self, Parameters(req): Parameters<tools::diff::Request>) -> ToolOut {
        json_string(tools::diff::handle(req))
    }

    #[tool(
        description = "Parse a SWIFT MT wire message (the `{1:…}{2:…}{3:…}{4:…\\r\\n-}{5:…}` \
        FIN format used for MT103/MT202/etc.) into a structured block tree. Block 4 \
        is decomposed into ordered :tag:value fields; blocks 3 and 5 are decomposed \
        into {tag:value} sub-blocks. Structural only — semantic field decoding \
        (e.g. :32A: date+currency+amount) is not done at this layer."
    )]
    fn wf_parse_swift_mt(
        &self,
        Parameters(req): Parameters<tools::swift_parse::Request>,
    ) -> ToolOut {
        json_string(tools::swift_parse::handle(req))
    }
}

#[tool_handler]
impl ServerHandler for WireforgeServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Wireforge MCP server for ISO 8583 + SWIFT MT financial messages. Use \
                 these tools to parse, build, validate, and explain wire messages. The \
                 server is read-only; no state is persisted between calls."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

fn json_string(out: Result<serde_json::Value, String>) -> ToolOut {
    match out {
        Ok(v) => serde_json::to_string_pretty(&v).map_err(|e| format!("serialize: {e}")),
        Err(e) => Err(e),
    }
}
