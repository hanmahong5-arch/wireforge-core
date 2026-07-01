//! Wireforge Model Context Protocol server.
//!
//! Exposes ISO 8583 + SWIFT MT tools over MCP stdio transport so MCP-aware
//! AI agents can parse, build, and validate financial messages without
//! leaving the agent loop.
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
//! - `wf_mt_mx_truncation_diff` — MT103 vs pacs.008 field truncation/loss
//!   DETECTOR (no conversion, no conformance claim)
//! - `wf_ebcdic_decode`    — EBCDIC hex → Unicode text
//! - `wf_sm3`              — GM/T 0004-2012 SM3 hash digest
//! - `wf_mx_address_compliance` — CBPR+ SR2026 structural address-presence
//!   check (TwnNm + Ctry in pacs.008.001.08 / pacs.004.001.09 / pacs.003.001.08
//!   / pain.001.001.09; auto-detects the message type; NOT a full CBPR+
//!   validation)
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
        run("wf_parse_iso8583", move || tools::parse::handle(req))
    }

    #[tool(
        description = "Build an ISO 8583 wire message (returned as a hex string) from \
        an MTI plus a map of field-number-to-payload. Plain string payloads are treated \
        as ASCII; prefix a payload with \"hex:\" to send raw binary bytes (e.g. PIN \
        block, MAC)."
    )]
    fn wf_build_iso8583(&self, Parameters(req): Parameters<tools::build::Request>) -> ToolOut {
        run("wf_build_iso8583", move || tools::build::handle(req))
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
        run("wf_validate_iso8583", move || tools::validate::handle(req))
    }

    #[tool(
        description = "Look up an ISO 8583 field's spec metadata by field number \
        (1..=128). Returns name, data type, length kind (fixed/llvar/lllvar), and \
        length value."
    )]
    fn wf_field_lookup(&self, Parameters(req): Parameters<tools::field::Request>) -> ToolOut {
        run("wf_field_lookup", move || tools::field::handle(req))
    }

    #[tool(
        description = "Decode a 4-digit ISO 8583 MTI (e.g. \"0200\") into its four \
        semantic positions: version (1987/1993/2003/national/private), class \
        (authorization/financial/...), function (request/response/advice/...), and \
        origin (acquirer/issuer/...)."
    )]
    fn wf_decode_mti(&self, Parameters(req): Parameters<tools::mti::Request>) -> ToolOut {
        run("wf_decode_mti", move || tools::mti::handle(req))
    }

    #[tool(description = "Produce a natural-language description of an ISO 8583 \
        message: a one-line summary plus a per-field explanation built from the \
        official field-name table. This tool performs NO LLM call; it returns \
        structured facts for the calling agent to reason over.")]
    fn wf_explain_message(&self, Parameters(req): Parameters<tools::explain::Request>) -> ToolOut {
        run("wf_explain_message", move || tools::explain::handle(req))
    }

    #[tool(
        description = "Parse a hex-encoded ISO 8583 message and re-build it, then \
        byte-compare the result against the original. Useful to confirm a message is \
        canonical (parser and builder are exact inverses). Reports byte-level \
        differences on mismatch."
    )]
    fn wf_roundtrip_check(&self, Parameters(req): Parameters<tools::diff::Request>) -> ToolOut {
        run("wf_roundtrip_check", move || tools::diff::handle(req))
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
        run("wf_parse_swift_mt", move || tools::swift_parse::handle(req))
    }

    #[tool(
        description = "Decode an EBCDIC hex dump into Unicode text — the natural way \
        to inspect a mainframe dump. Supply the hex bytes (whitespace tolerated) and \
        optionally a code page (\"cp037\" default, or \"cp500\"). Returns the decoded \
        text, the code page applied, and the byte count."
    )]
    fn wf_ebcdic_decode(
        &self,
        Parameters(req): Parameters<tools::ebcdic_decode::Request>,
    ) -> ToolOut {
        run("wf_ebcdic_decode", move || {
            tools::ebcdic_decode::handle(req)
        })
    }

    #[tool(
        description = "Compute the GM/T 0004-2012 SM3 hash (functional) of an input. \
        Provide exactly one of `hex` (hex-encoded bytes, whitespace tolerated) or \
        `text` (a UTF-8 string). Returns the lowercase 64-char hex digest, which input \
        kind was hashed, and the byte length hashed."
    )]
    fn wf_sm3(&self, Parameters(req): Parameters<tools::sm3::Request>) -> ToolOut {
        run("wf_sm3", move || tools::sm3::handle(req))
    }

    #[tool(
        description = "DETECT field truncation and loss between a SWIFT MT103 (ISO 15022) and an \
        ISO 20022 pacs.008.001.08 the caller already holds. This is a DETECTOR, not a converter: \
        it does NOT convert MT to MX or MX to MT, and makes NO certification, conformance, or \
        equivalence claim. Provide the pair one of two ways: either `mt` (raw SWIFT MT103 wire \
        text) AND `mx` (raw ISO 20022 XML, a full <AppHdr>+<Document> envelope), OR `wf` alone (a \
        single `.wf` source string holding a matched swift-mt + mx pair). Coverage is limited to \
        pacs.008.001.08 vs MT103 across five roles only: debtor name, creditor name, remittance \
        info, settlement amount, settlement currency. Each role gets a verdict (equal/reformatted/\
        truncated/dropped/added/mismatch); a `note` field restates this scope."
    )]
    fn wf_mt_mx_truncation_diff(
        &self,
        Parameters(req): Parameters<tools::mt_mx_diff::Request>,
    ) -> ToolOut {
        run("wf_mt_mx_truncation_diff", move || {
            tools::mt_mx_diff::handle(req)
        })
    }

    #[tool(
        description = "Structural CBPR+ SR2026 address-compliance check: verifies that a \
        pacs.008.001.08 (FIToFICstmrCdtTrf), pacs.004.001.09 (PmtRtr), pacs.003.001.08 \
        (FIToFICstmrDrctDbt) OR pain.001.001.09 (CstmrCdtTrfInitn) debtor/creditor postal \
        address carries Town Name (TwnNm) and Country (Ctry) in dedicated structured fields, as \
        required from 2026-11-14. The message type is auto-detected. Provide `mx` as the raw \
        ISO 20022 XML envelope. Returns a `note` stating the scope, a `message_type` naming the \
        detected spec, and a `rows` array (one entry per party) with `party`, `verdict` \
        (compliant/missing_structured/no_address), `town_name`, `country`, and \
        `unstructured_lines`. This is a presence check against that one SR2026 rule, NOT a full \
        CBPR+ validation and NOT a certification."
    )]
    fn wf_mx_address_compliance(
        &self,
        Parameters(req): Parameters<tools::address_compliance::Request>,
    ) -> ToolOut {
        run("wf_mx_address_compliance", move || {
            tools::address_compliance::handle(req)
        })
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

/// Run one tool handler inside an observability span and serialize its result.
///
/// Opens a `tool` span carrying the tool name (the call-site "component"
/// slice, à la Starring's `bcl_log` component arg) so every event the handler
/// emits — and the outcome logged here — is filterable by tool. The handler's
/// raw input bytes are dumped at `TRACE` upstream (e.g. in [`hex::decode`]);
/// here we log only the outcome (ok size / error) at `DEBUG`/`WARN`, never the
/// payload, to keep default-level logs quiet and free of message contents.
fn run<F>(tool: &'static str, f: F) -> ToolOut
where
    F: FnOnce() -> Result<serde_json::Value, String>,
{
    let _span = tracing::info_span!("tool", tool).entered();
    tracing::debug!("invoked");
    match f() {
        Ok(v) => match serde_json::to_string_pretty(&v) {
            Ok(s) => {
                tracing::debug!(bytes = s.len(), "ok");
                Ok(s)
            }
            Err(e) => {
                let msg = format!("serialize: {e}");
                tracing::error!(error = %msg, "serialize failed");
                Err(msg)
            }
        },
        Err(e) => {
            tracing::warn!(error = %e, "tool error");
            Err(e)
        }
    }
}
