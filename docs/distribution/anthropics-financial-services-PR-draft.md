# PR draft: Add wireforge ISO 8583 connector

Target: `anthropics/financial-services` (the repo listing MCP
connectors maintained or endorsed for financial workloads;
**verify the actual repo path before submitting** — the name may
differ).

## PR title

```
Add wireforge connector — ISO 8583 message codec via MCP
```

## PR body

```markdown
## Connector summary

- **Name:** wireforge
- **Domain:** Card-payment processing — ISO 8583-1987 ASCII wire codec
- **Protocol:** Model Context Protocol (stdio transport)
- **Source:** https://github.com/wireforge/wireforge-core
- **License:** Apache-2.0
- **Binary:** `wf-mcp` (Rust)
- **Install:** `cargo install wf-mcp`

## Capabilities

Seven tools focused on parsing, building, validating, and explaining
ISO 8583 wire messages:

- `wf_parse_iso8583` — hex → structured field tree
- `wf_build_iso8583` — `{mti, fields}` → hex
- `wf_validate_iso8583` — structural validation (limited; see below)
- `wf_field_lookup` — field number → spec entry
- `wf_decode_mti` — 4-digit MTI → version/class/function/origin
- `wf_explain_message` — natural-language description (NO LLM call)
- `wf_roundtrip_check` — parse → build → byte-compare

## Why it fits the financial-services connector list

Card-payment infrastructure runs on ISO 8583. Anyone debugging an
authorization flow, building a card-network adapter, or reconciling
a settlement file ends up staring at hex. A first-class MCP
connector lets agents:

- Diagnose declined / malformed messages at the byte level
- Construct test messages for sandbox flows
- Translate field numbers and MTI codes without web searches
- Verify a message is canonical (`wf_roundtrip_check`) before
  emitting it to a downstream gateway

## Compliance / safety properties

- **Read-only by design.** No tool writes to a filesystem, network,
  or database. Every call is self-contained; nothing persists
  between calls.
- **No LLM dependency inside the tool.** `wf_explain_message`
  generates its description from the ISO 8583-1987 spec field
  table — there is no model call inside the server.
- **`unsafe_code = "forbid"`** at the workspace level. The MCP
  server itself uses zero `unsafe`. Three clippy lints
  (`unwrap_used`, `expect_used`, `panic`) are relaxed only because
  the `rmcp` procedural macro generates code we don't control;
  hand-written handler code still goes through `Result<_, _>`.
- **No PII surface.** The codec does not log payloads. Stdout is
  reserved for JSON-RPC frames; stderr carries structured tracing
  only.
- **Explicit limitations.** `wf_validate_iso8583` returns a
  `limitations` array in its response so the calling agent
  surfaces to the user what was NOT checked (PAN Luhn, charset,
  ISO 4217, MAC).

## Spec compliance

- Targets ISO 8583-1987 ASCII first (the de-facto dialect used in
  most acquirer / processor / scheme integrations).
- Field table covers slots 1..=104 plus 128 with concrete spec
  definitions; 105..=127 are treated as opaque binary envelopes
  per the 1987 spec's "reserved for ISO/national/private use"
  guidance. (Source citations in
  [`crates/wf-codec/src/iso8583/field.rs`](https://github.com/wireforge/wireforge-core/blob/main/crates/wf-codec/src/iso8583/field.rs).)
- Parser and builder are exact inverses for any structurally-valid
  message; round-trip tests in the workspace guard this property.

## Pre-PR checklist

- [x] Apache-2.0 license file in repo root
- [x] Crate metadata complete (description, license, repository,
      authors)
- [x] `cargo test --workspace` green (≥ 39 tests across the crates)
- [x] `cargo clippy --workspace --all-targets -- -D warnings` green
- [x] `cargo fmt --all -- --check` green
- [x] `docs/mcp-integration.md` covers client setup for Claude Code,
      Cursor, hermes-agent
- [x] README contains MCP section with one-line install + example
- [ ] Real-world sample tests (open — sourcing sanitized ISO 8583
      hex from external channels; see
      [TigerBeetle community blog post](../blog/debugging-iso8583-tigerbeetle.md)
      for the outreach)

## Open questions for reviewers

- Is there a preferred connector manifest format
  (`manifest.yaml` / `connector.json`)? I'll match whatever
  convention already exists in the repo.
- How should I advertise the structural-only validator limitation
  in the manifest — a `safety_notes` field, a tool-level
  capability flag, or just inline in the description?
- Are there compliance attestations (SOC2, PCI scope) you want
  before listing? Wireforge runs entirely in-process on the user's
  machine and never sees a real PAN; happy to add documentation if
  that helps.
```

## Submission notes

- This PR carries the highest distribution value of the four
  channels (Warp / hermes / TigerBeetle / anthropics) AND the
  highest uncertainty. Acceptance may stall on compliance review
  for weeks; that's expected and documented in the risk register.
- Even if the PR is rejected, the underlying `wf-mcp` crate and
  the issue / listing request remain valuable as a public artifact
  that future agent platforms can reference.
- Coordinate the actual submission with: (a) wf-mcp published to
  crates.io, (b) at least one real-world sample test merged into
  the workspace, (c) the TigerBeetle blog post reaching a few
  community readers (proof of demand).
