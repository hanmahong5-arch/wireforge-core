# Wireforge User Guide

A task-oriented manual for the `wf` CLI and the `wf-mcp` AI-agent server.
Written for payment engineers, integration testers, and compliance teams —
no Rust knowledge required.

> **Honesty first.** Every check below is validated against synthetic,
> spec-derived test vectors — no real production samples yet (help us fix
> that: [sample call](https://github.com/hanmahong5-arch/wireforge-core/discussions/1)).
> Wireforge produces **detector verdicts and evidence**, never certifications.
> Everything runs locally: your messages never leave your machine.

---

## 1. Install

**Option A — prebuilt binary (fastest, no toolchain).** Download the archive
for your platform from the
[latest release](https://github.com/hanmahong5-arch/wireforge-core/releases/latest),
unpack, and put `wf` (and `wf-mcp` if you want the AI-agent server) on your
`PATH`. Each archive ships a `.sha256` next to it — verify before running:

```bash
sha256sum -c wf-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

**Option B — build from source (any platform Rust supports):**

```bash
cargo install --git https://github.com/hanmahong5-arch/wireforge-core wf-cli
cargo install --git https://github.com/hanmahong5-arch/wireforge-core wf-mcp   # optional
```

**Option C — crates.io:** `cargo install wf-cli` (being rolled out; use
Option B until the crates are indexed).

Check it works:

```bash
wf --version
```

## 2. Five-minute tour

```bash
# 1. Is this pacs.008 ready for SR2026? (exit code = CI verdict)
wf xform address-check payment.xml

# 2. What would an MT103 lose against this pacs.008?
wf xform diff payment.mt103 payment.xml

# 3. What is in this ISO 8583 frame?
echo "303230302000000000000000303030303030" | wf parse -
```

---

## 3. SR2026 address-compliance gate — `wf xform address-check`

### The problem

CBPR+ Structured Rich Data / SR2026 makes a **structured** debtor/creditor
postal address — Town Name (`TwnNm`) and Country (`Ctry`) in dedicated
`PstlAdr` fields — **mandatory on 2026-11-14** for cross-border payment
messages. Messages that still carry the address as free-text `AdrLine`
blobs face rejection, repair fees, and manual-investigation queues after
the deadline. Most institutions have a backlog of templates and generated
messages that were never migrated.

Wireforge scans your **outbound message store in bulk, offline**, and tells
you exactly which messages and which parties (debtor / creditor) still fail
the structural rule.

### Scan things

```bash
wf xform address-check payment.xml            # one file — full verdict tree
wf xform address-check a.xml b.xml c.xml      # several files — compact lines + summary
wf xform address-check outbox/                # every *.xml in a directory (one level)
cat payment.xml | wf xform address-check -    # stdin
```

Supported message types (auto-detected per file): `pacs.008.001.08`,
`pacs.004.001.09`, `pacs.003.001.08`, `pain.001.001.09`. Each input must be a
full ISO 20022 envelope (`AppHdr` + `Document`).

### Read the verdicts

Per message you get one row per party (debtor, creditor):

| Verdict | Meaning | What to do |
|---|---|---|
| `compliant` | `TwnNm` + `Ctry` present as structured fields | Nothing. |
| `missing_structured` | Address exists but the required structured fields are absent (details show what is missing and how many `AdrLine` lines await migration) | Restructure the address at the source system / template. SWIFT's free single-message translator can suggest a structured form. |
| `no_address` | The party carries no postal address at all | Check whether your flow requires one — SR2026 mandates structure *when* an address is present. |

### Wire it into CI

The exit code is diff-style, so the command *is* the gate:

- `0` — every input compliant
- `1` — ran cleanly, at least one input non-compliant
- `2` — at least one input unreadable / unparseable / unsupported (errors
  dominate; one bad file never aborts the rest of the batch)

GitHub Actions example:

```yaml
sr2026-gate:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Install wf
      run: |
        curl -sL https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/wf-v0.1.0-x86_64-unknown-linux-gnu.tar.gz | tar xz
        echo "$PWD/wf-v0.1.0-x86_64-unknown-linux-gnu" >> "$GITHUB_PATH"
    - name: SR2026 address gate
      run: wf xform address-check outbox/
```

GitLab CI example:

```yaml
sr2026-gate:
  script:
    - curl -sL https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/wf-v0.1.0-x86_64-unknown-linux-gnu.tar.gz | tar xz
    - export PATH="$PWD/wf-v0.1.0-x86_64-unknown-linux-gnu:$PATH"
    - wf xform address-check outbox/
```

### Scope, honestly

This is a **structural presence DETECTOR for the one cited SR2026 rule** —
not a full CBPR+ validation, not a certification. Directory scan is one
level, `*.xml` only, sorted; machine-readable (`--format json`) output is on
the roadmap. All fixtures used to validate the checker are SYNTHETIC.

---

## 4. MT ↔ MX truncation detector — `wf xform diff`

### The problem

During the MT/MX coexistence era the same payment often exists as both an
MT103 and a pacs.008. MX fields are richer and longer (structured names,
140-char remittance) than their MT counterparts (4×35 text blocks) — so
somewhere in the chain data gets **silently truncated or dropped**. That is
an AML/sanctions-screening and repair-cost problem, and nobody reports it,
because converters convert; they do not account for loss.

### Diff a pair

```bash
wf xform diff payment.mt103 payment.xml     # MT file + MX envelope file
wf xform diff --wf pair.wf                  # both halves in one .wf file
```

Per role you get a verdict: `equal`, `reformatted`, `truncated` (with the
exact lost suffix), `dropped`, `added`, `mismatch`, or `absent_both`.

### Scope, honestly

DETECTOR, not a converter. Coverage today: **pacs.008.001.08 vs MT103,
five roles** — debtor name, creditor name, remittance info, settlement
amount, settlement currency — each capped against cited standard lengths.

---

## 5. Use it from an AI agent (MCP)

`wf-mcp` exposes the same engine as **12 read-only MCP tools** — no network
access, no side effects, message bytes stay on your machine.

**Claude Desktop** (easiest): download
[`wireforge.mcpb`](https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/wireforge.mcpb)
from the release and open it with Claude Desktop (Settings → Extensions) —
the bundle carries macOS (Apple Silicon) and Windows binaries.

**Claude Code / Cursor / other stdio clients**: install `wf-mcp` (Section 1)
and register it:

```json
{ "mcpServers": { "wireforge": { "command": "wf-mcp" } } }
```

**MCP Registry**: the server is published as
[`io.github.hanmahong5-arch/wireforge`](https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.hanmahong5-arch/wireforge).

Then just ask, in plain language:

> "Check every pacs.008 in this folder for SR2026 address compliance and
> summarize what fails." — the agent batches `wf_mx_address_compliance`
>
> "Parse this ISO 8583 hex and explain field 39." — `wf_parse_iso8583` +
> `wf_explain_message` + `wf_field_lookup`
>
> "Diff this MT103 against its pacs.008 — what would we lose?" —
> `wf_mt_mx_truncation_diff`

Full tool list and payload conventions: [`docs/mcp-integration.md`](mcp-integration.md).

---

## 6. ISO 8583 toolbox — `wf parse` / `wf build`

```bash
# hex in, field tree out (dialect auto-sniffed: HybridAscii / FullAscii / FullBinary)
echo "303230302000000000000000303030303030" | wf parse -

# JSON message description in, wire hex out
echo '{"mti":"0200","fields":{"3":"000000","4":"000000010000"}}' | wf build
```

Fields 1–104 and 128 have concrete built-in definitions; 105–127 are opaque
envelopes. National / private dialects can override the field table at
runtime with a TOML `FieldSpec` (library feature `spec-load`) — no recompile.

SWIFT MT structural parsing works the same way:

```bash
wf swift parse message.mt   # block 1-5 tree; tags decoded for 20 / 32A / 50K
```

## 7. Legacy-encoding utilities — `wf ebcdic` / `wf sm3`

```bash
wf ebcdic decode C1C2C3            # EBCDIC hex -> "ABC"   (--cp 037 | 500)
wf ebcdic encode "ABC" --cp 500    # text -> EBCDIC hex
wf sm3 --text abc                  # SM3 (GB/T 32905) digest, or: wf sm3 <hex>
```

Useful when your captures come off IBM-mainframe rails or CN rails. The
GM/T crypto is functional only — **no** 密评 / GB/T 39786 / OSCCA
certification claim.

---

## 8. Migration regression evidence — `wf oracle check`

### The problem

You are replacing a legacy ISO 8583 endpoint. The vendor says the new one
"behaves the same". Prove it — field by field, run by run — without trusting
anyone's word, including the new system's own logs.

### Mode-A replay

Feed the same request's two responses (captured legacy vs migrated) plus an
operator-approved **mask spec** that says what may legitimately differ:

```toml
interface = "iso8583"
default_mask = "stable"      # fail closed: unlisted fields must be byte-identical

[[mask]]
field = 11                   # STAN — varies every run
mask = "volatile"

[[mask]]
field = 52                   # re-derived security data
mask = "crypto"

[[mask]]
field = 63                   # migration intentionally bumps V1 -> V2
mask = "intended-delta"
expect = "V2"
```

```bash
wf oracle check --req req.hex --legacy legacy.hex --migrated migrated.hex --spec masks.toml
wf oracle check --wf triple.wf     # or all four in one .wf artifact
```

Inputs accept a file path, `hex:<bytes>`, or `-` (stdin, at most one).
Exit codes: `0` conformant, `1` **unexplained drift found**, `2` could not
compare. The report carries a coverage meter counting only value-bearing
baseline fields — volatile/crypto masks can never inflate it.

This output is **EVIDENCE of regression-conformance under a given capture —
not a proof, certification, or equivalence claim.** A worked example lives in
[`crates/wf-format/examples/iso8583-oracle.wf`](../crates/wf-format/examples/iso8583-oracle.wf).

---

## 9. Spec-recovery verification — `wf layout check`

### The problem

You inherited a fixed-length host interface with a stale (or missing) spec.
Someone drafts a field table from documentation or by eyeballing captures —
how do you know the draft actually matches the bytes **before** code gets
written against it?

### Check a draft against captured frames

```toml
# layout.toml — declared field widths; optional variable tail
[[field]]
name = "len"
len = 4
[[field]]
name = "code"
len = 2
[[field]]
name = "body"
rest = true          # only allowed on the last field
```

```bash
wf layout check --layout layout.toml --trace capture.log   # log with [buffer dump: …] blocks
wf layout check --layout layout.toml --frame 'hex:30303132...'
```

A layout "matches" when its widths tile a captured frame exactly — no
truncation, no remainder. Structural check only; values and semantics are
not validated. Exit codes: `0` explains ≥1 frame, `1` explains none,
`2` bad input.

---

## 10. Capture message specs in git — the `.wf` format

Matched MT/MX pairs, oracle triples, and ISO 8583 examples can live in one
reviewable flat file (Bruno-style, line-oriented, diff-friendly):

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

Unknown block kinds are preserved raw, so newer files never break older
parsers. Examples: [`crates/wf-format/examples/`](../crates/wf-format/examples/).

---

## 11. Reference

**Exit codes** (uniform across `address-check`, `oracle check`, `layout check`):

| Code | Meaning |
|---|---|
| 0 | Gate passes (compliant / conformant / layout explains frames) |
| 1 | Ran cleanly, gate fails (non-compliance / drift / no match found) |
| 2 | Could not check (unreadable, unparseable, bad spec) — errors dominate |

**Logging**: add `-v` / `-vv` / `-vvv` for info / debug / trace on **stderr**
(stdout stays machine-clean for pipes); at trace level raw buffers are
hex-dumped. `RUST_LOG` overrides. Local only — no telemetry, ever.

**Getting help / contributing**:

- Bugs & wrong verdicts: [open an issue](https://github.com/hanmahong5-arch/wireforge-core/issues)
  — honest failure reports are the most valuable thing you can send.
- Donate a sanitized real sample (the project's #1 need):
  [Discussion #1](https://github.com/hanmahong5-arch/wireforge-core/discussions/1).
- Full scope-and-honesty statement: [README](../README.md#scope--honesty).
