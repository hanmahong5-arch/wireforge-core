# MCP Integration

`wf-mcp` is the Model Context Protocol server that exposes Wireforge's
ISO 8583 and SWIFT MT/MX codecs as tools an AI agent can call. It speaks
**stdio** transport — the agent (Claude Code, Cursor, hermes-agent, ...)
spawns `wf-mcp` as a child process and exchanges JSON-RPC over
stdin/stdout.

The server is **read-only and stateless**: every tool call is
self-contained, no session data is persisted between calls, and the
process can be killed at any time.

## Tools exposed

The server exposes **13 read-only tools**:

| Name                      | Purpose                                                          |
|---------------------------|------------------------------------------------------------------|
| `wf_parse_iso8583`        | Hex → structured field tree (MTI, bitmap, decoded fields).       |
| `wf_build_iso8583`        | `{mti, fields}` → hex wire string.                               |
| `wf_validate_iso8583`     | Structural validation (see Limitations below).                   |
| `wf_field_lookup`         | Field number (1..=128) → FieldDef (name, type, length spec).     |
| `wf_decode_mti`           | 4-digit MTI → version + class + function + origin.               |
| `wf_explain_message`      | Natural-language description, no LLM call.                       |
| `wf_roundtrip_check`      | parse → build → byte-compare for canonicality.                   |
| `wf_parse_swift_mt`       | SWIFT MT message text → parsed blocks and tagged fields.         |
| `wf_ebcdic_decode`        | EBCDIC-encoded hex bytes → decoded text.                         |
| `wf_sm3`                  | Bytes → SM3 (GB/T 32905) hash digest.                            |
| `wf_mt_mx_truncation_diff`| Detector: flags fields that would be lost or truncated mapping an MT message toward MX. Reports differences only — it does NOT convert, certify, or assert MT↔MX equivalence/conformance. |
| `wf_mx_address_compliance` | Check a pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 or pain.001.001.09 debtor/creditor postal address for the CBPR+ SR2026 structured-address requirement (Town Name `TwnNm` + Country `Ctry` in dedicated fields, mandatory 2026-11-14). Auto-detects the message type. A structural presence check against that one rule — DETECTOR, not a full CBPR+ validation and not a certification. |
| `wf_mx_address_scan`      | Batch variant of `wf_mx_address_compliance`: runs the same SR2026 structured-address presence check over one-or-more MX envelopes and returns a diff-style gate/exit_code summarizing the whole batch. DETECTOR, not a full CBPR+ validation and not a certification. |

### Limitations of `wf_validate_iso8583`

The validator only checks **wire structure** (MTI digits, bitmap
consistency, field length envelopes). It does **NOT** verify:

- PAN Luhn checksum
- Numeric / alpha field charset (a "numeric" field may pass with
  non-digit bytes)
- Currency code ISO 4217 membership
- MAC / PIN-block integrity

The validator's response always carries an explicit `limitations` list;
surface it to the user so they know what was and wasn't checked.

### `wf_build_iso8583` field payload convention

`fields` is a map from field-number-as-string (`"3"`, `"52"`, ...) to a
payload string. Two payload encodings:

- Plain string → ASCII bytes. Example: `"3": "000000"`.
- `"hex:..."` prefix → raw binary bytes. Example: `"52": "hex:0102030405060708"`.

The same payload is sized against the field's spec on build; oversize,
undersize, or otherwise-invalid payloads come back as a tool error.

## Installation

### From source (recommended while pre-1.0)

```bash
cargo install --path crates/wf-mcp \
  --manifest-path /absolute/path/to/wireforge-core/Cargo.toml
```

This installs the `wf-mcp` binary into `$CARGO_HOME/bin` (typically
`~/.cargo/bin/wf-mcp` on Unix, `%USERPROFILE%\.cargo\bin\wf-mcp.exe` on
Windows). Ensure that directory is on `PATH`, or use the absolute path
in the client configs below.

### From crates.io (once published)

```bash
cargo install wf-mcp
```

## Client setup

### Claude Code

Edit `~/.claude/settings.json` (or the project-local equivalent) and add
an entry under `mcpServers`:

```json
{
  "mcpServers": {
    "wireforge": {
      "command": "wf-mcp"
    }
  }
}
```

Restart Claude Code. In a new session, ask:

> Parse this ISO 8583 hex: `303230302000000000000000303030303030`

Claude should call `wf_parse_iso8583` automatically and return a
structured tree.

### Cursor

Cursor's MCP integration reads the same shape under
`Settings → MCP Servers`:

```json
{
  "wireforge": {
    "command": "wf-mcp"
  }
}
```

### hermes-agent

Add to the agent's tool registry (consult hermes-agent's own docs for
the exact YAML / JSON location — at the time of writing, it scans
`~/.hermes/mcp.json`):

```json
{
  "servers": [
    {
      "name": "wireforge",
      "command": "wf-mcp"
    }
  ]
}
```

## Logging

`wf-mcp` writes structured logs to **stderr** (stdout is reserved for
the JSON-RPC frame). Control verbosity with `RUST_LOG`:

```bash
RUST_LOG=debug wf-mcp
```

The Claude Code / Cursor UIs typically show stderr in the MCP server
panel; check there first if the agent can't reach the server.

## Why no SSE / HTTP transport yet

stdio covers every client we care about for the initial release
(Claude Code, Cursor, hermes-agent). SSE / streamable-HTTP add network
attack surface, auth concerns, and deployment overhead. They are
tracked for a later sprint and will land behind a `--transport http`
flag.

## Reporting issues

File bugs at the wireforge-core repo issue tracker. Include:

- The hex you fed in (sanitize PAN to `400000xxxxxxxx0002` style)
- The tool you called
- The JSON response you got
- Your `wf-mcp --version` and rustc version
