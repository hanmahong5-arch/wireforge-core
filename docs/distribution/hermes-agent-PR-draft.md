# PR draft: List `wf-mcp` in hermes-agent's MCP server registry

Target: `hermes-agent/hermes-agent` (or the actual registry repo;
verify location before submitting — the registry may live in a
separate `mcp-servers` repo per their convention).

## PR title

```
Add wireforge (wf-mcp) — ISO 8583 + SWIFT MT/MX financial message codec
```

## PR body

```markdown
## Server

- **Name:** wireforge
- **Registry name:** `io.github.hanmahong5-arch/wireforge` (see `server.json` at repo root)
- **Binary:** `wf-mcp`
- **Crate:** [`wf-mcp` on crates.io](https://crates.io/crates/wf-mcp)
- **Source:** https://github.com/hanmahong5-arch/wireforge-core
- **License:** Apache-2.0
- **Transport:** stdio

## Tools exposed

12 read-only tools:

| Name                       | Purpose                                                    |
|----------------------------|------------------------------------------------------------|
| `wf_parse_iso8583`         | Hex → structured field tree (MTI, bitmap, fields).         |
| `wf_build_iso8583`         | `{mti, fields}` → hex wire string.                         |
| `wf_validate_iso8583`      | Structural validation (no PAN Luhn / MAC — see Limits).    |
| `wf_field_lookup`          | Field number (1..=128) → FieldDef (name, type, length).    |
| `wf_decode_mti`            | 4-digit MTI → version + class + function + origin.         |
| `wf_explain_message`       | Natural-language description (no LLM call — pure spec).    |
| `wf_roundtrip_check`       | parse → build → byte-compare for canonicality.             |
| `wf_parse_swift_mt`        | SWIFT MT message text → parsed blocks and tagged fields.   |
| `wf_ebcdic_decode`         | EBCDIC-encoded hex bytes → decoded text.                   |
| `wf_sm3`                   | Bytes → SM3 (GB/T 32905) hash digest.                      |
| `wf_mt_mx_truncation_diff` | Detector for fields lost/truncated mapping MT→MX; reports differences only (no conversion / certification / equivalence claim). |
| `wf_mx_address_compliance` | Check a pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 or pain.001.001.09 debtor/creditor postal address for the CBPR+ SR2026 structured-address requirement (Town Name `TwnNm` + Country `Ctry` in dedicated fields, mandatory 2026-11-14). Auto-detects the message type. A structural presence check against that one rule — DETECTOR, not a full CBPR+ validation and not a certification. |

## Why this belongs in the registry

ISO 8583 is the dominant card-payment wire protocol. It is bitmapped,
length-prefixed, and dialect-heavy — bad input shapes today land in
agents as opaque hex blobs the agent can't reason about. Wiring a
deterministic codec as a tool lets the agent describe, validate, and
construct ISO 8583 messages without leaving its loop.

## What `wf_validate_iso8583` does and does NOT check

The validator confirms wire structure (MTI ASCII digits, bitmap
consistency, field length envelopes). It does NOT verify:

- PAN Luhn checksum
- Numeric / alpha field charset conformance
- ISO 4217 currency code membership
- MAC / PIN-block integrity

The response includes an explicit `limitations` array; agents should
surface it to the user. This is documented in
[`docs/mcp-integration.md`](https://github.com/hanmahong5-arch/wireforge-core/blob/main/docs/mcp-integration.md).

## Installation

```bash
cargo install wf-mcp
```

Then in `~/.hermes/mcp.json` (or wherever hermes-agent scans):

```json
{
  "servers": [
    { "name": "wireforge", "command": "wf-mcp" }
  ]
}
```

## Testing

- Workspace `cargo test` is green across the codec crates
  (`wf-bitmap`, `wf-codec`, `wf-swift`, `wf-xform`, `wf-sm`,
  `wf-mcp`, ...).
- Manual integration verified on Claude Code and Cursor with stdio
  transport.
- The server is read-only (no FS writes, no network, no state
  between calls).
```

## Submission steps

1. Check whether hermes-agent's registry lives in the main repo or
   a separate `mcp-servers` repo. Fork the right one.
2. Locate the registry format — typically `servers.yaml`,
   `registry.json`, or per-server YAML files. Match local
   convention.
3. Add the entry; confirm the entry passes any registry-validation
   CI.
4. Open the PR with title + body above.

## Open questions for maintainers

- Does hermes-agent want a server-side feature flag or capability
  manifest beyond the standard MCP `tools/list` discovery?
- Is there a preferred way to advertise tool limitations (the
  validate-only-structure caveat) in the registry vs in tool
  descriptions?

## Registry metadata

A `server.json` conforming to the official MCP registry schema lives at
the repo root. Registry name: `io.github.hanmahong5-arch/wireforge`.
Note: crates.io / cargo is not yet a `registryType` in the registry
schema, so the `server.json` carries the source `repository` plus the
`cargo install wf-mcp` instruction and the 12-tool list under `_meta`.

## Status

Pre-1.0. wf-mcp version `0.1.0` at PR time; SemVer breaks are still
possible. Real publish to crates.io happens after the internal
dependency chain is published in order:
`wf-bitmap, wf-format, wf-wal, wf-sm, wf-mx → wf-codec → wf-swift →
wf-xform → wf-cli, wf-mcp` — coordinate with maintainers on timing.
This document is a draft and has not been submitted.
