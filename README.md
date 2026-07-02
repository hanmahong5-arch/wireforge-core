# wireforge-core

> ### SR2026 address check in 30 seconds
>
> CBPR+ SR2026 makes structured debtor/creditor postal addresses (`TwnNm` +
> `Ctry` in dedicated fields) **mandatory on 2026-11-14**. Batch-scan your
> outbound pacs.008 / pacs.004 / pacs.003 / pain.001 store before the deadline:
>
> ```bash
> cargo install --git https://github.com/hanmahong5-arch/wireforge-core wf-cli
> wf xform address-check outbox/     # exit 0 = compliant, 1 = gaps, 2 = unreadable
> ```
>
> Runs entirely on your machine — no message ever leaves it. CI-ready exit
> codes. This is a structural presence **DETECTOR** for the cited SR2026 rule,
> not a full CBPR+ validation and not a certification; details
> [below](#sr2026-address-compliance-gate-cli).

## What is this?

Wireforge is a **local-first toolkit for financial wire messages** — a CLI
(`wf`), an AI-agent server (`wf-mcp`, 12 read-only MCP tools), and the Rust
crates underneath. It exists because four problems keep landing on payment
teams' desks:

| Pain | What Wireforge does about it |
|---|---|
| **SR2026 deadline (2026-11-14):** CBPR+ makes structured debtor/creditor addresses mandatory; free-text `AdrLine` messages face rejection and repair queues | `wf xform address-check` batch-scans your outbound pacs.008 / pacs.004 / pacs.003 / pain.001 store and exits with a CI-ready 0/1/2 verdict per run |
| **Silent MT↔MX data loss:** coexistence-era conversion truncates 140-char MX names/remittance into 4×35 MT blocks — an AML/screening risk nobody reports | `wf xform diff` compares a matched MT103 + pacs.008 pair and names each truncated/dropped role, including the exact lost characters |
| **Messages are too sensitive for online tools:** you cannot paste production payment data into a web validator | Everything runs offline on your machine — no network calls, no telemetry, stdout clean for pipes |
| **Legacy migration "trust us" gap:** a replaced ISO 8583 endpoint claims byte-compatibility; nobody can prove it | `wf oracle check` replays captured legacy-vs-migrated responses under an operator-approved mask spec and emits field-level regression-conformance EVIDENCE; `wf layout check` verifies recovered fixed-length specs against real captures |

**Who it's for:** payment/integration engineers wiring compliance gates into
CI, compliance teams sizing their SR2026 backlog, migration teams needing
regression evidence, and AI-agent users who want a safe read-only tool for
message forensics.

**Start here → [User Guide](docs/user-guide.md)** — task-oriented manual for
every command, CI recipes included. No Rust knowledge required.

## For developers: the crates

Apache-2.0 Rust crates for parsing, building, and reasoning about
financial wire messages. Today covers **ISO 8583** (three on-the-wire
dialects + runtime-loadable field specs), **SWIFT MT** (structural +
tag-level semantic decoders, plus a typed facade), **ISO 20022 / MX**
(inbound parse via a typed facade), **MT↔MX truncation diffing**,
**EBCDIC** (CP037 / CP500), **China GM/T crypto** (SM3 / SM4 / SM2,
functional — not 密评-certified), and the **`.wf` flat-file format**
for capturing message specs under Git.

| Crate         | Purpose                                                            |
|---------------|--------------------------------------------------------------------|
| `wf-bitmap`   | ISO 8583 primary / secondary bitmap encode + decode.               |
| `wf-codec`    | ISO 8583 parser + builder + 128-field table (HybridAscii /        |
|               | FullAscii / FullBinary BCD dialects) + runtime-loadable FieldSpec; |
|               | SWIFT MT structural parser + semantic decoders (20 / 32A / 50K);  |
|               | EBCDIC CP037 / CP500 single-byte codec.                          |
| `wf-cli`      | `wf` CLI binary — parse / build / validate from the shell.         |
| `wf-format`   | Parser for the `.wf` Bruno-inspired flat-file DSL.                 |
| `wf-mcp`      | Model Context Protocol server — expose codec to AI agents.         |
| `wf-sm`       | China GM/T cryptography — SM3 hash, SM4 cipher (ECB/CBC +          |
|               | bounded streaming), SM2 signature (functional, not 密评-certified).|
| `wf-swift`    | Typed SWIFT MT facade over an external parser, with lossless       |
|               | fallback to `wf-codec`'s structural parser.                        |
| `wf-mx`       | ISO 20022 / MX inbound facade (pacs / pain / camt / admi).         |
| `wf-xform`    | pacs.008.001.08 ↔ MT103 truncation/loss **detector** across five   |
|               | roles — DETECTOR not converter; no certification claim.            |
| `wf-wal`      | Append-only write-ahead log with CRC-32 + `truncate_to` recovery.  |
| `wf-obs`      | Local-first observability: leveled `tracing` logs, bounded raw-    |
|               | buffer hex dumps, stderr subscriber setup (no telemetry).          |

**Scope & honesty (`wf-xform`):** it compares pacs.008.001.08 against
MT103 over five core roles and is SYNTHETIC-validated only (no real
production samples yet); it is a DETECTOR, not a converter, and makes
no certification, conformance, or equivalence claim.

`tools/sample-sanitize/` is a standalone (out-of-workspace) binary
that redacts PAN / track data from real ISO 8583 hex samples; see
[`docs/sample-policy.md`](docs/sample-policy.md) for the redaction
contract.

## 5-minute try

No clone needed — install `wf` straight from the repo and parse one
sample ISO 8583 frame:

```bash
cargo install --git https://github.com/hanmahong5-arch/wireforge-core wf-cli
echo "303230302000000000000000303030303030" | wf parse -
```

You should see a tree with `MTI = 0200`, the bitmap, and field 3
("Processing Code") decoded. If something looks off — wrong field, a
crash, anything — please file an issue using the **feedback** template.
Honest reports beat polished stars.

Prebuilt binaries for Linux / macOS / Windows are attached to each
[GitHub release](https://github.com/hanmahong5-arch/wireforge-core/releases).

## Quick start (CLI from source)

```bash
cargo install --path crates/wf-cli
echo "303230302000000000000000303030303030" | wf parse -
```

Output: a tree showing MTI, bitmap, and decoded fields. ISO 8583
auto-sniffs across HybridAscii / FullAscii / FullBinary dialects; pass
`--dialect <name>` to force one.

Add `-v` / `-vv` / `-vvv` for info / debug / trace logging on **stderr**
(stdout stays machine-clean); at trace, the raw input buffer is hex-dumped.
`RUST_LOG` overrides the level. Logs are local-only — no telemetry.

## SR2026 address-compliance gate (CLI)

CBPR+ SR2026 makes a structured debtor/creditor postal address (`TwnNm` +
`Ctry` in dedicated `PstlAdr` fields) mandatory on **2026-11-14**. Scan your
outbound message store before the deadline:

```bash
wf xform address-check outbox/             # scan a directory of *.xml
wf xform address-check a.xml b.xml c.xml   # one or more explicit files
cat msg.xml | wf xform address-check -     # one envelope from stdin
```

The message type (pacs.008.001.08 / pacs.004.001.09 / pacs.003.001.08 /
pain.001.001.09) is auto-detected per file, and the process exits with a
diff-style code so the check drops straight into CI:

- `0` — every input is compliant
- `1` — ran cleanly, but at least one input is non-compliant
- `2` — at least one input could not be checked (unreadable / unparseable /
  unsupported message type)

One unreadable file does not abort the batch — it is reported and folded into
the exit code. A directory scan is **one level, `*.xml` only, sorted**
(recursive scan and a `--format json` machine output are not yet built). This
is a **structural presence DETECTOR** for the one cited SR2026 rule — **not** a
full CBPR+ validation and **not** a certification; all fixtures are SYNTHETIC.

## Quick start (MCP, for AI agents)

**Claude Desktop**: download
[`wireforge.mcpb`](https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/wireforge.mcpb)
and open it (Settings → Extensions) — the bundle carries macOS (Apple
Silicon) and Windows binaries. The server is on the MCP Registry as
`io.github.hanmahong5-arch/wireforge`.

**Other stdio clients**: install the binary —

```bash
cargo install --path crates/wf-mcp
```

— then wire it into your MCP-aware client. For Claude Code, add
to `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "wireforge": { "command": "wf-mcp" }
  }
}
```

The agent can now call 12 tools:

- `wf_parse_iso8583` — hex → structured field tree
- `wf_build_iso8583` — `{mti, fields}` → hex
- `wf_validate_iso8583` — structural validation
- `wf_field_lookup` — field number → FieldDef
- `wf_decode_mti` — 4-digit MTI → semantic parts
- `wf_explain_message` — natural-language description (no LLM call)
- `wf_roundtrip_check` — parse → build → byte-compare
- `wf_parse_swift_mt` — SWIFT MT raw → structural block tree
- `wf_ebcdic_decode` — EBCDIC-encoded hex bytes → decoded text
- `wf_sm3` — bytes → SM3 (GB/T 32905) hash digest
- `wf_mt_mx_truncation_diff` — pacs.008.001.08 ↔ MT103 field truncation/loss
  **detector** across five roles (DETECTOR not converter; no
  certification / conformance / equivalence claim)
- `wf_mx_address_compliance` — check a pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 or
  pain.001.001.09 debtor/creditor postal address for the CBPR+ SR2026 structured-address
  requirement (`TwnNm` + `Ctry` in dedicated fields, mandatory 2026-11-14); auto-detects the
  message type. DETECTOR, not a full CBPR+ validation and not a certification

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

Backed by RustCrypto [`sm3`](https://docs.rs/sm3) (the `Digest` trait);
the streaming `Sm3` hasher keeps real 64-byte block state, so hashing a
large WAL is O(1) memory. Algorithm-selection rationale, the 2026-05-29
swap from `smcrypto`, and the GB/T 32905-2016 test vectors live in
[`docs/sm-crypto-research-2026-05.md`](docs/sm-crypto-research-2026-05.md).

## Status

Pre-1.0. ISO 8583-1987 parser / builder are exact inverses for every
supported dialect; field 1..=104 plus 128 have concrete spec
definitions, and fields 105..=127 are treated as opaque binary
envelopes. A runtime-loadable `FieldSpec` (TOML, `spec-load` feature)
overrides the built-in table for national / private dialects without a
recompile. SWIFT MT structural layer is complete; semantic field
decoders cover three MT103 anchor tags (20 / 32A / 50K) with the
`MtFieldDecoder` trait as the extension point. EBCDIC CP037 / CP500
single-byte codec is implemented (tables vendored under the Unicode
License, see `NOTICE`); DBCS host pages are deferred. `wf-sm` exposes
SM3 hash, SM4 (ECB / CBC + bounded streaming), and SM2 signature —
functional only, with **no** 密评 / GB/T 39786 compliance claim.

## Scope & honesty

Read this before trusting any number.

- **No real production samples yet.** Correctness is grounded on synthetic and
  standard/specification test vectors only (labelled `SYNTHETIC` in-source) plus
  a property-based round-trip fuzz suite. Any accuracy statement is `⏳ pending`
  real-sample validation. The Phase 0 exit gate (≥ 5 real ISO 8583 hex samples)
  is **unmet** — contributions welcome via `tools/sample-sanitize/`.
- **`wf-xform` is a truncation DETECTOR, not a converter.** It compares an MT103
  and a pacs.008.001.08 a caller already holds and reports per-role field loss
  against cited maximum lengths. It performs **no** conversion and makes **no**
  certification, conformance, or equivalence claim.
- **`wf-sm` (国密) is functional only** — no 密评 / GB/T 39786 / OSCCA
  certification claim; SM2 rests on an unaudited upstream. Suitable for
  development against CN rails, not as a certified cryptographic product.
- **`wf-mx` wraps a third-party upstream** (`mx-message` 3.1.4) that is currently
  frozen. The facade isolates the dependency; owning MX parsing is a future option.

A grounded, source-cited go-to-market and next-steps plan lives in
[`docs/strategy/next-steps-2026-06.md`](docs/strategy/next-steps-2026-06.md).

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
