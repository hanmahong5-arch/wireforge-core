# wireforge-core

Apache-2.0 Rust crates for parsing, building, and reasoning about
financial wire messages. Today covers **ISO 8583** (three on-the-wire
dialects), **SWIFT MT** (structural + tag-level semantic decoders),
**SM3** (China GM/T cryptographic hash), and the **`.wf` flat-file
format** for capturing message specs under Git.

## Crates

| Crate         | Purpose                                                            |
|---------------|--------------------------------------------------------------------|
| `wf-bitmap`   | ISO 8583 primary / secondary bitmap encode + decode.               |
| `wf-codec`    | ISO 8583 parser + builder + 128-field type table (HybridAscii /   |
|               | FullAscii / FullBinary BCD dialects); SWIFT MT structural parser  |
|               | + semantic field decoders for tags 20 / 32A / 50K; EBCDIC stub.   |
| `wf-cli`      | `wf` CLI binary — parse / build / validate from the shell.         |
| `wf-format`   | Parser for the `.wf` Bruno-inspired flat-file DSL.                 |
| `wf-mcp`      | Model Context Protocol server — expose codec to AI agents.         |
| `wf-sm`       | China GM/T cryptography — SM3 today, SM2 / SM4 extension points.   |
| `wf-wal`      | Append-only write-ahead log with CRC-32 + `truncate_to` recovery.  |

`tools/sample-sanitize/` is a standalone (out-of-workspace) binary
that redacts PAN / track data from real ISO 8583 hex samples; see
[`docs/sample-policy.md`](docs/sample-policy.md) for the redaction
contract.

## Quick start (CLI)

```bash
cargo install --path crates/wf-cli
echo "303230302000000000000000303030303030" | wf parse -
```

Output: a tree showing MTI, bitmap, and decoded fields. ISO 8583
auto-sniffs across HybridAscii / FullAscii / FullBinary dialects; pass
`--dialect <name>` to force one.

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

The agent can now call eight tools:

- `wf_parse_iso8583` — hex → structured field tree
- `wf_build_iso8583` — `{mti, fields}` → hex
- `wf_validate_iso8583` — structural validation
- `wf_field_lookup` — field number → FieldDef
- `wf_decode_mti` — 4-digit MTI → semantic parts
- `wf_explain_message` — natural-language description (no LLM call)
- `wf_roundtrip_check` — parse → build → byte-compare
- `wf_parse_swift_mt` — SWIFT MT raw → structural block tree

See [`docs/mcp-integration.md`](docs/mcp-integration.md) for client
setup details, the field-payload convention, and validator
limitations.

## Quick start (`.wf` flat-file)

```bash
cat crates/wf-format/examples/iso8583-auth.wf
```

```text
meta {
  name: Auth Request 0200
  type: iso8583
}
iso8583 {
  mti: 0200
  field 2: 4242424242424242
  field 4: 000000010000
}
```

Parse it with the `wf-format` crate's `parse(&str) -> WfFile`. The
grammar is line-oriented + brace-grouped + additive: unrecognised
blocks land in `Body::Raw` so newer files never break older parsers.

## Quick start (SM3)

```rust
use wf_sm::{sm3, sm3_hex};
let digest = sm3(b"abc");           // [u8; 32]
let hex    = sm3_hex(b"abc");        // "66c7f0f4…8f4ba8e0"
```

Backed by `smcrypto` 0.3; throughput ~60-100 MB/s single-thread on a
laptop. Algorithm-selection rationale + measured numbers live in
[`docs/sm-crypto-research-2026-05.md`](docs/sm-crypto-research-2026-05.md).

## Status

Pre-1.0. ISO 8583-1987 parser / builder are exact inverses for every
supported dialect; field 1..=104 plus 128 have concrete spec
definitions, and fields 105..=127 are treated as opaque binary
envelopes. SWIFT MT structural layer is complete; semantic field
decoders cover three MT103 anchor tags (20 / 32A / 50K) with the
`MtFieldDecoder` trait as the extension point. EBCDIC module is
scaffolding only. `wf-sm` exposes SM3; SM2 / SM4 are reserved
extension-point modules.

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
