# wireforge-core

Apache-2.0 Rust crates for parsing, building, and reasoning about
financial wire messages. Currently focused on **ISO 8583**, with
SWIFT MT and EBCDIC adapters in flight.

## Crates

| Crate       | Purpose                                                    |
|-------------|------------------------------------------------------------|
| `wf-bitmap` | ISO 8583 primary / secondary bitmap encode + decode.       |
| `wf-codec`  | ISO 8583 parser + builder + field-table; SWIFT, EBCDIC.    |
| `wf-cli`    | `wf` CLI binary — parse / build from the shell.            |
| `wf-mcp`    | Model Context Protocol server — expose codec to AI agents. |

## Quick start (CLI)

```bash
cargo install --path crates/wf-cli
echo "303230302000000000000000303030303030" | wf parse -
```

Output: a tree showing MTI, bitmap, and decoded fields.

## Quick start (MCP, for AI agents)

```bash
cargo install --path crates/wf-mcp
```

Then wire the binary into your MCP-aware client. For Claude Code, add
to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "wireforge": { "command": "wf-mcp" }
  }
}
```

The agent can now call seven tools:

- `wf_parse_iso8583` — hex → structured field tree
- `wf_build_iso8583` — `{mti, fields}` → hex
- `wf_validate_iso8583` — structural validation
- `wf_field_lookup` — field number → FieldDef
- `wf_decode_mti` — 4-digit MTI → semantic parts
- `wf_explain_message` — natural-language description (no LLM call)
- `wf_roundtrip_check` — parse → build → byte-compare

See [`docs/mcp-integration.md`](docs/mcp-integration.md) for client
setup details, the field-payload convention, and validator
limitations.

## Status

Pre-1.0. The parser/builder are exact inverses for ISO 8583-1987 ASCII
messages; field 1..=104 plus 128 have concrete spec definitions, and
fields 105..=127 are treated as opaque binary envelopes. SWIFT MT and
EBCDIC modules are scaffolding only.

## Building

```bash
cargo build --workspace
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

The workspace pins clippy `unwrap_used` / `expect_used` / `panic` to
`deny`. The `wf-mcp` crate relaxes those three lints locally because
`rmcp`'s procedural macro generates code we don't control;
hand-written handlers in that crate still funnel errors through
`Result<_, _>`.

## License

Apache-2.0. See `LICENSE`.
