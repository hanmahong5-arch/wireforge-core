# PR draft: List `wf-mcp` in hermes-agent's MCP server registry

Target: `hermes-agent/hermes-agent` (or the actual registry repo;
verify location before submitting — the registry may live in a
separate `mcp-servers` repo per their convention).

## PR title

```
Add wireforge (wf-mcp) — ISO 8583 financial message codec
```

## PR body

```markdown
## Server

- **Name:** wireforge
- **Binary:** `wf-mcp`
- **Crate:** [`wf-mcp` on crates.io](https://crates.io/crates/wf-mcp)
- **Source:** https://github.com/wireforge/wireforge-core
- **License:** Apache-2.0
- **Transport:** stdio

## Tools exposed

| Name                  | Purpose                                                    |
|-----------------------|------------------------------------------------------------|
| `wf_parse_iso8583`    | Hex → structured field tree (MTI, bitmap, fields).         |
| `wf_build_iso8583`    | `{mti, fields}` → hex wire string.                         |
| `wf_validate_iso8583` | Structural validation (no PAN Luhn / MAC — see Limits).    |
| `wf_field_lookup`     | Field number (1..=128) → FieldDef (name, type, length).    |
| `wf_decode_mti`       | 4-digit MTI → version + class + function + origin.         |
| `wf_explain_message`  | Natural-language description (no LLM call — pure spec).    |
| `wf_roundtrip_check`  | parse → build → byte-compare for canonicality.             |

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
[`docs/mcp-integration.md`](https://github.com/wireforge/wireforge-core/blob/main/docs/mcp-integration.md).

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

- Workspace `cargo test` is green (≥ 39 tests across `wf-bitmap`,
  `wf-codec`, `wf-mcp`).
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

## Status

Pre-1.0. wf-mcp version `0.0.1` at PR time; SemVer breaks are still
possible. Will bump to `0.1.x` after the first integration
feedback. Real publish to crates.io happens after the dependency
chain (`wf-bitmap` → `wf-codec` → `wf-cli`/`wf-mcp`) is published in
order — coordinate with maintainers on timing.
