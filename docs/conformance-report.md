# Wireforge v0.1.0 — Conformance & Capability Report

**Date**: 2026-06-02
**Measured against**: commit at time of v0.1.0 publish-readiness review; updated for the `wf_mx_address_compliance` rename + pacs.004.001.09 extension, then extended again to pacs.003.001.08 + pain.001.001.09, then the `wf xform address-check` CLI **batch gate** (multi-file / directory scan + 0/1/2 exit code) (2026-06-03; MCP tool count unchanged at 12 — the gate is CLI-only).

---

## Scope and honesty

Read this section before the capability table.

1. **Test vector provenance**: all CI vectors are synthetic — constructed from
   published standards (ISO 8583, SWIFT MT User Handbook, CBPR+ SR2026 spec,
   ISO 20022 schema catalogue) and property-fuzz. No real production SWIFT MT
   messages, no real ISO 8583 hex from a live network, and no real pacs.008
   from a CBPR+ member institution have been used.

2. **Phase 0 gate is UNMET**: the project defines `≥ 5 real ISO 8583 hex
   samples from a live acquirer or issuer network` as the Phase 0 exit gate for
   real-sample validation. As of this report, that gate is **not met**. Every
   accuracy and round-trip claim below is a claim about behavior on synthetic
   and spec-vector inputs only.

3. **MT/MX feature is a detector, not a converter**: `wf-xform` and the
   `wf_mt_mx_truncation_diff` MCP tool compare an existing MT103 and an existing
   pacs.008.001.08 for per-role field truncation or loss. They do NOT convert
   MT to MX or MX to MT, do NOT produce a conformant output message, and make NO
   certification or equivalence claim.

4. **国密 (SM2/SM3/SM4) is functional, not 密评-certified**: the SM primitives
   use RustCrypto's `sm3 0.5`, `sm4 0.5`, `sm2 0.13` (all MIT OR Apache-2.0,
   unaudited). No GB/T 39786 certification exists. These are functionally-correct
   primitives for integration work outside the mandatory-certification perimeter.
   A pure-software Rust library cannot pass 密评; that requires an HSM/hardware path.

5. **`wf-mx` upstream**: the MX facade pins `mx-message = "=3.1.4"`, frozen
   since October 2025 (upstream effort moved to Reframe). If that crate is
   eventually deprecated, the facade sits on an unmaintained dependency.

---

## Measured test results

Command run to capture totals:

```
"C:/Users/Anita/.cargo/bin/cargo.exe" test --workspace \
  --manifest-path E:/wireforge/wireforge-core/Cargo.toml 2>&1 | grep "test result"
```

**Aggregate: 434 passed / 0 failed / 6 ignored**

The 6 ignored tests are:
- 2 in `wf-codec/tests/parse_accuracy.rs` (`baseline_model_a`, `baseline_model_b`)
  — blocked on real samples + external API keys. Explicitly `#[ignore]`d by
  design until the Phase 0 gate clears.
- 4 in `wf-sm/tests/sm3_throughput.rs` — throughput benchmarks ignored in
  standard `cargo test` (require `--ignored` flag and take > 1 s each).

Clippy: `cargo clippy --workspace --all-targets -- -D warnings` exits 0 (no
warnings, no errors, as of 2026-06-03).

Per-binary breakdown (from `Running …` lines):

| Binary / test file | Passed | Ignored |
|---|---|---|
| `wf-bitmap` lib unit tests | 0 | 0 |
| `wf-bitmap/tests/iso8583_bitmap.rs` | 12 | 0 |
| `wf-cli` lib unit tests | 0 | 0 |
| `wf-cli` bin unit tests | 0 | 0 |
| `wf-cli/tests/address_check.rs` | 18 | 0 |
| `wf-cli/tests/cli_roundtrip.rs` | 9 | 0 |
| `wf-cli/tests/ebcdic_sm3.rs` | 14 | 0 |
| `wf-cli/tests/xform_diff.rs` | 8 | 0 |
| `wf-codec` lib unit tests | 51 | 0 |
| `wf-codec/tests/iso8583_dialect.rs` | 20 | 0 |
| `wf-codec/tests/iso8583_field.rs` | 16 | 0 |
| `wf-codec/tests/iso8583_message.rs` | 16 | 0 |
| `wf-codec/tests/iso8583_spec.rs` | 8 | 0 |
| `wf-codec/tests/parse_accuracy.rs` | 1 | 2 |
| `wf-codec/tests/roundtrip_proptest.rs` | 4 | 0 |
| `wf-codec/tests/swift_semantic.rs` | 7 | 0 |
| `wf-codec/tests/swift_structure.rs` | 27 | 0 |
| `wf-format` lib unit tests | 22 | 0 |
| `wf-format/tests/dsl_grammar.rs` | 19 | 0 |
| `wf-format/tests/roundtrip_proptest.rs` | 7 | 0 |
| `wf-mcp` lib unit tests | 58 | 0 |
| `wf-mcp` bin unit tests | 0 | 0 |
| `wf-mx` lib unit tests | 5 | 0 |
| `wf-obs` lib unit tests | 5 | 0 |
| `wf-sm` lib unit tests | 32 | 0 |
| `wf-sm/tests/sm3_throughput.rs` | 0 | 4 |
| `wf-swift` lib unit tests | 4 | 0 |
| `wf-wal` lib unit tests | 2 | 0 |
| `wf-wal/tests/wal_recovery.rs` | 13 | 0 |
| `wf-xform` lib unit tests | 12 | 0 |
| `wf-xform/tests/address_compliance.rs` | 30 | 0 |
| `wf-xform/tests/golden_corpus.rs` | 8 | 0 |
| `wf-xform/tests/maxlen_pin.rs` | 3 | 0 |

---

## Capability table

Each row: what the capability covers, which test files constitute the evidence,
test count as measured above, and the relevant honesty caveat.

### ISO 8583 codec — three dialects + BCD + round-trip property test

| | |
|---|---|
| **What is covered** | Parse and build ISO 8583 messages in three wire dialects: `HybridAscii` (ASCII MTI + raw binary bitmap + ASCII fields), `FullAscii` (ASCII MTI + 16/32 ASCII hex bitmap + ASCII fields; jPOS ISO87A/ISO93A style), `FullBinary` (BCD-packed MTI + raw binary bitmap + BCD-packed Numeric fields; jPOS ISO87BPackager / mainframe NDC). The `parse_any` path tries dialects in declaration order. |
| **Test files** | `crates/wf-codec/tests/iso8583_dialect.rs` (20 tests), `crates/wf-codec/tests/iso8583_message.rs` (16 tests), `crates/wf-codec/tests/iso8583_field.rs` (16 tests), `crates/wf-codec/tests/roundtrip_proptest.rs` (4 property tests) |
| **Test count** | 56 deterministic + 4 property-fuzz |
| **Caveat** | All vectors are synthetic or spec-derived. No real acquirer/issuer wire captures. The `parse_accuracy.rs` baseline tests (which would measure accuracy against real samples) are currently ignored. |

### Runtime FieldSpec (TOML)

| | |
|---|---|
| **What is covered** | `FieldSpec::from_toml_str` (gated behind the `spec-load` feature) loads a `[[field]]` TOML document into a live `FieldSpec` at runtime; the codec then uses it in place of the built-in table. |
| **Test files** | `crates/wf-codec/tests/iso8583_spec.rs` (8 tests) |
| **Test count** | 8 |
| **Caveat** | TOML parsing uses `toml 0.8` + `serde`; only the field-definition schema is tested, not arbitrary user TOML. |

### SWIFT MT — typed facade + structural fallback

| | |
|---|---|
| **What is covered** | `wf-swift` wraps `swift-mt-message = "=3.1.5"`. `WfMt::Typed` carries a fully-typed body for the ~30 MT types the upstream supports (MT103, MT202, MT940, etc.). `WfMt::Structural` is the lossless fallback for unsupported types or failed typed parses: it decomposes blocks 1–5, and block 4 into ordered `:tag:value` fields. |
| **Test files** | `crates/wf-codec/tests/swift_structure.rs` (27 tests), `crates/wf-codec/tests/swift_semantic.rs` (7 tests), `crates/wf-swift/src/lib.rs` in-module tests (4 tests) |
| **Test count** | 38 |
| **Caveat** | Semantic field decoding within block 4 tags (e.g. the date+currency+amount decomposition of `:32A:`) is not done at the facade layer. All test vectors are synthetic. |

### ISO 20022 MX — pacs.008 inbound

| | |
|---|---|
| **What is covered** | `wf-mx` wraps `mx-message = "=3.1.4"`. `WfMx::from_xml` accepts a full `<AppHdr>+<Document>` envelope (bare `<Document>` is rejected). The typed body is the `Document::Pacs008` variant of the upstream's ~25-type enum. `message_type()` returns the `MsgDefIdr` string (e.g. `"pacs.008.001.08"`). `to_xml()` and `to_json()` are provided. |
| **Test files** | `crates/wf-mx/src/lib.rs` in-module tests (5 tests) |
| **Test count** | 5 |
| **Caveat** | `mx-message 3.1.4` is pinned and frozen (upstream development moved to Reframe as of Oct 2025). If that crate is deprecated, this facade depends on an unmaintained library. Only inbound parsing is exercised in these tests; build/emit of arbitrary pacs.008 bodies is not a supported workflow. |

### MT/MX truncation detector — 5 roles, cited caps

| | |
|---|---|
| **What is covered** | `wf-xform` takes an existing MT103 and an existing pacs.008.001.08 and reports, per role, whether values are `equal`, `reformatted`, `truncated`, `dropped`, `added`, or `mismatch`. Roles covered: `debtor_name`, `creditor_name`, `remittance_info`, `settlement_amount`, `settlement_currency`. Every max-length threshold cites the standard it comes from (SWIFT MT103 field spec; CBPR+ pacs.008.001.08 validators). |
| **Test files** | `crates/wf-xform/tests/golden_corpus.rs` (8 tests), `crates/wf-xform/tests/maxlen_pin.rs` (3 tests), `crates/wf-xform/src/lib.rs` in-module tests (11 tests), `crates/wf-cli/tests/xform_diff.rs` (8 tests), `crates/wf-mcp/src/tools/mt_mx_diff.rs` in-module tests (9 tests) |
| **Test count** | 39 |
| **Caveat** | This is a **detector only**. It does not convert, does not produce a conformant output message, and makes no certification or equivalence claim. Coverage is limited to exactly 5 roles and exactly one message pair type (MT103 vs pacs.008.001.08). |

### MX SR2026 address compliance (pacs.008 + pacs.004 + pacs.003 + pain.001)

| | |
|---|---|
| **What is covered** | `wf-xform/src/address.rs` and the `wf_mx_address_compliance` MCP tool check whether a pacs.008.001.08, pacs.004.001.09, pacs.003.001.08 or pain.001.001.09 debtor/creditor postal address carries `TwnNm` and `Ctry` in dedicated structured XML elements, as required from 2026-11-14 per CBPR+ SR2026. The message type is auto-detected (`check_mx_address` dispatch). Returns `compliant`, `missing_structured`, or `no_address` per party. The `wf xform address-check` CLI adds a **batch gate**: it scans one-or-more files, a one-level `*.xml` directory (sorted), or stdin, and exits **0 / 1 / 2** (all-compliant / found-non-compliant / had-errors) so the check can fail a CI pipeline before the deadline. |
| **Test files** | `crates/wf-xform/tests/address_compliance.rs` (30 tests), `crates/wf-mcp/src/tools/address_compliance.rs` in-module tests (14 tests), `crates/wf-cli/tests/address_check.rs` (18 tests — single-file render + the pure `render_address_scan` gate / `select_xml` filter) |
| **Test count** | 62 |
| **Caveat** | This is a single-rule presence check, not a full CBPR+ validation. Scope is pacs.008.001.08 + pacs.004.001.09 + pacs.003.001.08 + pain.001.001.09 (all four tested here). The CLI gate is CLI-only — the MCP tool stays single-envelope, so the tool count is unchanged at 12; **recursive directory scan** and a **`--format json`** machine output are deferred. pain.008 (multi-debtor direct debit) and the pacs.009/pacs.010 FI-party messages are deferred; camt.056 is a 2027 scope item. Not a certification. |

### EBCDIC CP037 / CP500

| | |
|---|---|
| **What is covered** | `wf-codec` provides `decode` and `encode` for IBM CP037 (US/Canada financial hosts) and CP500 ("International"). The two tables differ at 7 byte positions. Both directions (byte→char and char→byte) are covered. |
| **Test files** | `crates/wf-cli/tests/ebcdic_sm3.rs` (14 tests; includes EBCDIC decode cases), `crates/wf-mcp/src/tools/ebcdic_decode.rs` in-module tests (4 tests) |
| **Test count** | 18 relevant tests |
| **Caveat** | DBCS / mixed SBCS-DBCS code pages (CP935 / CP1388 / GBK) are explicitly deferred. A one-byte-lookahead seam is reserved in the decoder for a future DBCS state machine but is currently asserted empty. |

### SM3 / SM4 / SM2

| | |
|---|---|
| **What is covered** | SM3 (GM/T 0004-2012) streaming hash via `sm3 0.5`; SM4 (GM/T 0002-2012) block cipher in ECB and CBC with PKCS#7 padding via `sm4 0.5`; SM2 (GM/T 0003-2012) signing and verification with automatic ZA identity pre-hashing via `sm2 0.13`. |
| **Test files** | `crates/wf-sm/src/sm3.rs` in-module tests (7), `crates/wf-sm/src/sm4.rs` (12), `crates/wf-sm/src/sm2.rs` (13), `crates/wf-sm/tests/sm3_throughput.rs` (4 ignored), `crates/wf-cli/tests/ebcdic_sm3.rs` (14 includes SM3 cases), `crates/wf-mcp/src/tools/sm3.rs` (4) |
| **Test count** | 32 in `wf-sm` lib unit tests (all three modules), 4 SM3 MCP tests |
| **Caveat** | All three RustCrypto crates are labeled unaudited. No 密评 (GB/T 39786) certification. Use as functionally-correct primitives for CIPS-adjacent integration work outside the mandatory-certification perimeter. |

### `.wf` format — parse + AST-idempotent serialize + round-trip property test

| | |
|---|---|
| **What is covered** | `wf-format` lexes and parses the Wireforge DSL (a Bruno-inspired Git-native flat file format for ISO 8583 and SWIFT MT/MX specifications). The AST covers `Iso8583Body`, `SwiftMtBody`, and `MxBody`. The writer serializes the AST back to `.wf` text. Property tests use `proptest` to generate arbitrary valid ASTs and assert `parse(serialize(ast)) == ast` (AST-idempotent round-trip). Block comments (`/* … */`) and line comments are stripped at parse; the round-trip is over the parsed form, not the original source text. |
| **Test files** | `crates/wf-format/tests/dsl_grammar.rs` (19 tests), `crates/wf-format/tests/roundtrip_proptest.rs` (7 property tests), `crates/wf-format/src/` in-module tests (22 tests) |
| **Test count** | 48 |
| **Caveat** | `mx` opaque values cannot carry `/*` or `*/` (this is a known parser limitation encoded as a `prop_filter` in the property test). The format is version 0; backward-compatibility is not guaranteed until v1.0. |

### 12 MCP tools

| | |
|---|---|
| **What is covered** | `wf-mcp` exposes 12 MCP tools over stdio transport via `rmcp 0.16`: `wf_parse_iso8583`, `wf_build_iso8583`, `wf_validate_iso8583`, `wf_field_lookup`, `wf_decode_mti`, `wf_explain_message`, `wf_roundtrip_check`, `wf_parse_swift_mt`, `wf_mt_mx_truncation_diff`, `wf_ebcdic_decode`, `wf_sm3`, `wf_mx_address_compliance`. Each handler returns `Result<String, String>`; errors travel out as `isError: true` in the MCP `CallToolResult`. |
| **Test files** | `crates/wf-mcp/src/` in-module tests across 13 tool modules (54 tests total, incl. 14 for `wf_mx_address_compliance`); `crates/wf-mcp/src/hex.rs` (4 tests) |
| **Test count** | 58 |
| **Caveat** | Tests are handler-level unit tests (pure function calls), not end-to-end MCP protocol tests. The `rmcp` proc-macro relaxes three workspace clippy lints (`unwrap_used`, `expect_used`, `panic`) in the generated glue code; hand-written handler code is still `Result`-only. |

### `wf` CLI

| | |
|---|---|
| **What is covered** | The `wf` binary (`wf-cli`) exposes subcommands for parse, build, roundtrip, EBCDIC decode, SM3, SWIFT MT parse, `xform diff`, and `xform address-check` (the SR2026 batch gate). Integration tests exercise end-to-end CLI invocations. |
| **Test files** | `crates/wf-cli/tests/cli_roundtrip.rs` (9 tests), `crates/wf-cli/tests/ebcdic_sm3.rs` (14 tests), `crates/wf-cli/tests/xform_diff.rs` (8 tests); the `address-check` gate's tests are counted under *MX SR2026 address compliance* above (`address_check.rs`, 18 tests) |
| **Test count** | 31 |
| **Caveat** | Tests use `std::process::Command` to invoke the binary; they require a successful debug build. No shell-completion or man-page coverage. |

### Observability (`wf-obs`)

| | |
|---|---|
| **What is covered** | A local-first logging toolkit over the `tracing` facade, modeled on the Starring platform's `bcl_*` logging API: `hexdump` (bounded canonical buffer dump), `dump_buffer(level, …)` (the level-parameterized `bcl_dump_buffer_*` analog), `cli_level` (`-v`/`-vv`/`-vvv` → WARN/INFO/DEBUG/TRACE), and stderr subscriber installers for the CLI and MCP server. The CLI instruments each subcommand with a `cmd` span + outcome log and dumps raw input at TRACE; the MCP server runs each of the 12 tools inside a `tool` span and dumps decoded wire bytes at TRACE. stdout is never touched (CLI results / JSON-RPC framing stay clean). |
| **Test files** | `crates/wf-obs/src/lib.rs` in-module tests (5 tests: hexdump layout, non-printable dotting, empty input, over-cap bounding, verbosity→level mapping) |
| **Test count** | 5 |
| **Caveat** | Emission is verified by binary smoke (CLI `-vvv` and an MCP JSON-RPC handshake under `RUST_LOG=trace`), not by an automated subscriber-capture test. No remote/OTLP export, no metrics, no trace propagation — logging is local stderr only. Field-level (`ep_trace`) and a structured error catalog (`bclerr*`) from the Starring model are not ported. |

---

## What is NOT validated

The following gaps are real and must be disclosed to users:

- **Real production samples**: zero. The Phase 0 exit gate (≥ 5 real ISO 8583
  hex samples from a live acquirer or issuer) is unmet. CI evidence is synthetic
  only.
- **CBPR+ certification or conformance testing**: none. The SR2026 address check
  is a structural presence check, not a full CBPR+ validation suite.
- **国密 密评 (GB/T 39786) certification**: none. Impossible for pure-software
  Rust without an HSM/hardware path.
- **`wf-mx` upstream maintenance**: `mx-message 3.1.4` is frozen; if deprecated,
  the facade depends on an unmaintained crate.
- **DBCS EBCDIC** (CP935 / CP1388 / GBK): explicitly deferred.
- **`.wf` values containing `/*` or `*/`**: cannot be represented in the `mx`
  opaque-value block. This is a known grammar limitation.
- **MT/MX conversion**: the truncation detector explicitly does not convert.
  No converted output message is produced or validated.
- **SM2/SM4 external audit**: the RustCrypto crates used are self-labeled
  unaudited. No third-party cryptographic audit of this codebase has been
  performed.
- **End-to-end MCP protocol testing**: tool handlers are tested as pure Rust
  functions; the full stdio MCP framing layer is not exercised in CI.
