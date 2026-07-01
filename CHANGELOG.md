# Changelog

All notable changes to wireforge-core are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — unreleased

First public release: a pure-Rust toolkit for financial wire messages, exposed
to humans (CLI), AI agents (MCP), and a desktop GUI.

### Honesty & scope (read first)

- Correctness is currently grounded on **synthetic and standard/specification
  test vectors only** — no real production message samples have been ingested
  yet. Any accuracy statement is `⏳ pending` until validated against real
  samples. Synthetic fixtures are labelled `SYNTHETIC` in-source.
- The MT/MX feature is a **field-truncation DETECTOR, not a converter**: it
  compares two messages a caller already holds and reports per-role loss
  against cited maximum lengths. It makes **no** conversion, certification,
  conformance, or equivalence claim.
- The China GM/T crypto (SM2/SM3/SM4) is **functional only** — it carries **no**
  GB/T 39786 密评 / OSCCA certification claim, and SM2 rests on an unaudited
  upstream implementation.

### Added

- **ISO 8583 codec** (`wf-codec`, `wf-bitmap`): primary + secondary bitmaps;
  three on-wire dialects (HybridAscii, FullAscii, FullBinary) with BCD packing
  for numeric fields; a 128-field builtin spec; and an optional runtime-loadable
  `FieldSpec` (TOML, `spec-load` feature) so the core stays zero-dependency.
- **SWIFT MT facade** (`wf-swift`): "parse as strongly as possible, never fail
  to no structure" — a vetted typed parser with a lossless structural fallback
  for unsupported message types.
- **ISO 20022 (MX) facade** (`wf-mx`): typed inbound parse over a vetted CBPR+
  library (pacs / pain / camt / admi), with Wireforge-owned error isolation.
- **MT/MX truncation detector** (`wf-xform`): for a corresponding MT103 and
  pacs.008.001.08, classifies five core roles (debtor name, creditor name,
  remittance info, settlement amount, settlement currency) as
  equal / reformatted / truncated / dropped / added / mismatch / absent-both,
  with every truncation verdict grounded in a cited standard max-length.
- **EBCDIC** (`wf-codec`): CP037 / CP500 single-byte decode (total) and encode.
- **China GM/T crypto** (`wf-sm`): SM3 hash (streaming), SM4 (CBC/ECB), and
  SM2 sign/verify — functional, vector-checked against GB/T standards.
- **`.wf` flat-file format** (`wf-format`): a git-friendly DSL for capturing
  ISO 8583 / SWIFT MT / ISO 20022 message specs under version control, with a
  parser and an AST-idempotent serializer. A single `.wf` file can hold a
  matched `swift-mt` + `mx` pair for the truncation detector, or a
  role-tagged `req`/`legacy`/`migrated` ISO 8583 triple plus an `oracle-spec`
  block for the conformance engine (`extract_oracle_triple`).
- **MCP server** (`wf-mcp`): 12 read-only tools (`wf_parse_iso8583`,
  `wf_build_iso8583`, `wf_validate_iso8583`, `wf_field_lookup`, `wf_decode_mti`,
  `wf_explain_message`, `wf_roundtrip_check`, `wf_parse_swift_mt`,
  `wf_ebcdic_decode`, `wf_sm3`, `wf_mt_mx_truncation_diff`,
  `wf_mx_address_compliance`) over MCP stdio. The address-compliance tool is a
  CBPR+ SR2026 structured-address (`TwnNm` + `Ctry`) presence DETECTOR that
  auto-detects pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 and
  pain.001.001.09 — not a full CBPR+ validation and not a certification.
- **CLI** (`wf`): `parse` / `build` (ISO 8583), `swift parse`, `ebcdic
  decode/encode`, `sm3`, `xform diff` (MT/MX truncation, two files or a
  single `.wf` pair), `xform address-check`, and `oracle check` (ISO 8583
  regression-conformance EVIDENCE: four inputs + `--spec` TOML, or a single
  `--wf` triple file; diff-style exit code).
- **SR2026 address-compliance batch gate** (`wf xform address-check`): accepts
  one-or-more MX files, a directory (one level, `*.xml`, sorted), or `-` for a
  single stdin envelope, and exits with a **diff-style code** so it gates CI —
  `0` all compliant, `1` ran cleanly but found non-compliance, `2` an input
  could not be checked. One unreadable file is reported, not fatal to the
  batch. Same structural CBPR+ SR2026 `TwnNm` + `Ctry` presence **DETECTOR**
  (auto-detecting pacs.008.001.08 / pacs.004.001.09 / pacs.003.001.08 /
  pain.001.001.09) — not a full CBPR+ validation, not a certification; fixtures
  are SYNTHETIC. The MCP `wf_mx_address_compliance` tool stays single-envelope
  (tool count unchanged at 12); recursive scan and `--format json` are deferred.
- **ISO 8583 conformance EVIDENCE engine** (`wf-oracle`): a deterministic
  **Mode-A replay** engine that compares a captured **legacy** response against
  a **migrated** response field-by-field under an operator-approved mask
  (STABLE / VOLATILE / CRYPTO / INTENDED-DELTA — unconsidered fields default to
  STABLE and **fail closed**), emitting a coverage-metered
  **regression-conformance EVIDENCE** report and a diff-style gate (`0`
  conformant / `1` UNEXPLAINED drift / `2` uncheckable). Coverage counts only
  value-bearing baseline fields (Stable/IntendedDelta present on the legacy
  side); Volatile/Crypto are **excluded** so the number cannot be inflated, and
  `0/0` renders `0%` not a misleading 100%. It makes **no** proof,
  certification, or equivalence claim (those words appear only in the negative
  disclaimer). The engine is format-agnostic behind a `WireMessage` trait — ISO
  8583 is the first implementation; MX / fixed-length plug in later. A
  hard rule compares **full** field slices (a dedicated `LengthDiff` verdict),
  never truncating to a common length. Exposed as `wf oracle check`; the MCP
  `wf_oracle_check` tool is intentionally **deferred** to hold the surface at
  12 tools. Fixtures are SYNTHETIC.
- **Fixed-length record views + layout structural check** (`wf-oracle::fixed`,
  `wf layout check`): a `FixedLayout` (ordered named fields, fixed byte
  lengths, optional variable tail) **tiles** a captured frame — the field
  lengths must account for every byte, no truncation, no remainder — and the
  parsed `FixedView` feeds the same masked-diff EVIDENCE engine as ISO 8583
  (keyed by field ordinal). `wf layout check --layout <toml>` verifies a
  recovered field-table draft against real bytes **before** anyone trusts it:
  `--trace` extracts every frame from a `bcl_dump`-style trace (incomplete
  dumps are counted and reported, never silently swallowed) and groups results
  by frame length; `--frame` checks one raw frame. **Structural check only** —
  field values and semantics are NOT validated, and a variable-tail layout is
  a deliberately weaker claim (it admits any frame at least as long as its
  fixed prefix). Diff-style exit code: 0 = explains ≥1 frame, 1 = explains
  none, 2 = uncheckable.
- **Write-ahead log** (`wf-wal`): append-only, CRC32-guarded records with
  crash-safe tail recovery.
- **Observability** (`wf-obs`): a local-first logging layer over the `tracing`
  facade — leveled logging, bounded raw-buffer hex dumps (`dump_buffer`), and
  stderr subscriber setup. The `wf` CLI gains a repeatable `-v` flag
  (`-v`/`-vv`/`-vvv` → info/debug/trace) and per-subcommand spans; the MCP
  server runs every tool inside a `tool` span with outcome logging. stdout is
  never touched (results / JSON-RPC framing stay clean), and there is **no**
  telemetry or remote export — logs go to local stderr only.

### Engineering

- Library (non-test) code is free of `unwrap` / `expect` / `panic` / `unsafe`
  (workspace lints; `wf-mcp` relaxes the unwrap/panic lints only for
  third-party proc-macro-generated code).
- Hardened across three adversarial audit passes plus a property-based
  round-trip fuzz suite (`proptest`) for the `.wf`, ISO 8583, and SWIFT
  parsers; non-ASCII (incl. CJK) message text round-trips intact.

[0.1.0]: https://github.com/hanmahong5-arch/wireforge-core/releases/tag/v0.1.0
