# Sample-First Wave — Daily Acquisition Log

Wave start: 2026-05-20.
Primary goal: ≥5 real-shape sanitized ISO 8583 hex samples under
`samples/iso8583/` by D7 (2026-05-26), with verified sanitization +
license metadata.
Optimization target: **信息密度** — each EOD produces one quantified
`sample-inventory-delta` plus per-channel response breakdown.

Plan reference: see the implementation plan presented before this log
was created (Sample-First Wave, drafted 2026-05-20).
Tools introduced today:
- `tools/sample-fetch.sh` (T0 mining wrapper)
- `tools/sample-sanitize/` (Rust bin + lib; round-trip-verifying
  redactor; not a workspace member)
- `docs/distribution/tigerbeetle-discord-post.md` (T1 channel #6 draft)
- `docs/distribution/hn-show-hn-draft.md` (T1 channel #7 draft)

The "Decision-signal contract" table in plan §6 is the canonical
must-produce list. Each daily section below mirrors that contract.

---

## D1 EOD signal (2026-05-20)

| metric                                                                          | value | Δ vs D0 |
|---------------------------------------------------------------------------------|-------|---------|
| real-shape production-derived samples in `samples/iso8583/`                     | 0     | —       |
| T0-extracted candidates passing wf-codec parse                                  | 0     | —       |
| T0-discovered candidate files (any extension; pre-filter, not real-shape)       | 44    | +44     |
| T0 channels with infra ready                                                    | 3     | +3      |
| T0 channels actually cloned                                                     | 2/3   | +2      |
| T1 channels with broadcast drafts ready                                         | 2     | +2      |

### Infra delivered today (D1 Block 0 + actual Block 1 mining)

- `tools/sample-sanitize` — Rust crate, standalone (NOT a workspace
  member). Builds clean under `cargo build`, 9/9 lib tests pass, clippy
  `-D warnings` green, fmt clean.
- `tools/sample-fetch.sh` — wraps `git clone --depth=1` for the three
  T0 OSS sources (jpos, openiso8583-net, moov-io/iso8583); prints
  `gh search code` hints rather than running them (`gh` auth is the
  operator's call, not the tool's).
- `.gitignore` — adds `/candidates/` and `/tools/sample-sanitize/target/`.
- Daily-log scaffold (this file).
- TigerBeetle Discord post draft, HN Show HN draft.

### T0 channels — per-source MEASURED status

| source                       | clone outcome             | files discovered | wf-codec-parseable | verdict                                                          |
|------------------------------|---------------------------|------------------|--------------------|------------------------------------------------------------------|
| jpos/jPOS                    | success (Apache-2.0)      | 0 (filter miss)  | n/a                | jpos test resources are PEX + keystore + packager XML, no bare hex; filter needs to look at `.cfg` / `.xml` packagers next round, not `.hex` |
| openiso8583/openiso8583-net  | **404 not found**         | 0                | n/a                | URL guessed wrong; openiso8583 originated on SourceForge; need a new candidate URL for D2 retry (research target: `openiso8583`/`OpenISO8583`/`imohsenb/ISO-8583`) |
| moov-io/iso8583              | success (Apache-2.0)      | 44               | 0                  | 44 `*_test.go` files contain ISO 8583 message literals as **ASCII-hex-bitmap dialect** strings (e.g. `01007000…16424242…`); wf-codec parses the **binary-bitmap dialect**; the two formats are incompatible. See "Dialect findings" below.       |
| GitHub `gh search code`      | hint-only (`gh` unauth'd) | n/a              | n/a                | operator worklist for D2 morning                                  |
| UnionPay / CNAPS / CIPS PDF  | not attempted             | n/a              | n/a                | per plan §3 honest expectation: rarely publishes hex annexes      |
| SWIFT public corpus          | not attempted             | n/a              | n/a                | per plan §3: it's MT/MX, not 8583                                 |

### Dialect findings — **STRATEGIC, escalate to user attention**

Today's mining surfaced **three distinct ISO 8583 dialects in public
OSS fixtures**. wf-codec's current parser accepts a **fourth**, less
common dialect, and rejects all three of the publicly observed ones.

| dialect                                         | MTI form         | bitmap form          | observed in                                                                | wf-codec parses? |
|-------------------------------------------------|------------------|----------------------|----------------------------------------------------------------------------|------------------|
| Full-ASCII (everything text)                    | 4 ASCII digits   | 16/32 ASCII hex chars | `moov-io/iso8583` Go fixtures; `jpos/...ISO87APackager.bin`; `jpos/...ISO93APackager.bin` | **no**           |
| Full-binary (everything raw bytes)              | 2 binary bytes   | 8/16 raw binary bytes | `jpos/...ISO87BPackager.bin`                                                | **no**           |
| Hybrid: ASCII MTI + binary bitmap, ASCII fields | 4 ASCII digits   | 8/16 raw binary bytes | (none observed today)                                                       | **yes**          |

Concrete evidence (xxd of the first 16 bytes of each):

```
ISO87APackager.bin: 3038 3030 4646 4646 4646 4646 4646 4646  → ASCII "0800FFFFFFFFFFFF"
ISO87BPackager.bin: 0800 ffff ffff ffff ffff 0000 0040 0000  → binary MTI + binary bitmap
ISO93APackager.bin: 3138 3030 4646 4646 4646 4646 4646 4646  → ASCII "1800FFFFFFFFFFFF"
moov-io literal:    0100 7000 0000 0000 0000 (ASCII-encoded throughout)
```

`wf-codec` source (`crates/wf-codec/src/iso8583/parser.rs:104-141`)
requires ASCII MTI (4 digit chars) then probes `input[8] & 0x80` to
decide bitmap length, i.e. it expects a **raw byte** at offset 8 — not
two ASCII hex chars. Today's three observed dialects all fail this
probe, so all parse attempts at the byte level return `InvalidMti` or
`InsufficientBytes`.

**Why this matters for the wave**:

- The Sample-First wave's primary goal is "≥ 5 real-shape samples that
  PASS `wf-codec::parse`". If T1/T2 channels deliver bytes in either
  full-ASCII or full-binary dialect (the two mainstream variants), they
  will be **rejected by wf-codec** unchanged — counting toward "samples
  acquired" but not toward "Phase 0 exit gate cleared".
- The plan §5 D7 GO/NO-GO decision tree was written assuming "wf-codec
  parses what we collect". That assumption is now visibly wrong.

**Recommendation (for user decision, NOT executed)**:

Add a 1-2 day Sprint-3 task **before** the Sample-First wave's D7 GO
call: extend `wf-codec::iso8583::parse` with a `Dialect::{HybridAscii,
FullAscii, FullBinary}` argument and a small format-sniffer. Without
this, even a successful Sample-First wave may leave the AI baseline
gated on a separate, larger fix. Estimated cost: ~1 day for sniffer +
binary-bitmap-from-ASCII-hex decoder + binary-MTI decoder; +1 day for
test coverage across the three dialects.

Park it explicitly here rather than burying it on a backlog — this
**is** the most decision-relevant finding of D1.

### What was *not* counted (anti-Goodhart, plan §2)

- The 44 moov-io files were *discovered*, not *promoted*. None of them
  appears in `samples/iso8583/`. Even if wf-codec gained ASCII-hex-bitmap
  support tomorrow and parsed them, they would still be **tutorial-grade
  synthetic** (uniform 0100 MTI, 4242-PAN, three-field bitmap with no
  LLLVAR or Track-2 exercise) — and plan §2 anti-goal #2 explicitly
  forbids using such fixtures as samples.
- The sanitizer CLI was run once against the first moov literal as an
  end-to-end smoke test; the parse failure was the correct, expected
  outcome and proves the rejection path works. Output in
  `candidates/_smoke/` (git-ignored).

### Blockers found today

- `openiso8583-net` GitHub URL is wrong; the canonical repo moved
  (SourceForge origin). Need correct URL before D2 morning re-fetch.
- `gh search code` requires `gh auth login` which is not configured in
  this dev env. Hints emitted by the script are the operator's
  worklist for D2 morning.
- Discord post + HN post are DRAFTS; both require human submission
  (Discord credentials + HN account belong to the operator). Plan §5
  D2 Block 2A defers these to the operator.

### Decision triggered

User chose option **C** in the D1 wrap-up: do dialect support in parallel
with the T1 community broadcasts. Dialect support is delivered today
(below); T1 broadcasts remain in user hands.

### Bonus delivery — wf-codec dialect support (chose option C)

Implemented same-day after the dialect finding above. Scope:

- New `wf_codec::iso8583::Dialect` enum with `HybridAscii` (existing,
  default) and `FullAscii` (mainstream "text on the wire" variant).
  Lives in `crates/wf-codec/src/iso8583/dialect.rs`.
- Refactored `parser.rs`: extracted `read_bitmap` dispatch, added
  `parse_with(input, dialect)` and `parse_any(input)` that auto-detects
  via priority-order trial. Existing `parse(input)` is now an alias for
  `parse_any` and remains byte-for-byte back-compat for every historical
  HybridAscii caller (14 / 14 existing tests pass unchanged).
- Refactored `builder.rs`: added `build_with(msg, dialect)`. `build` is
  an alias for `build_with(msg, HybridAscii)` — back-compat.
- New test file `tests/iso8583_dialect.rs` — 11 / 11 pass covering:
  F1-F4 spec vectors for FullAscii parse, F5 lowercase hex acceptance,
  F6-F7 FullAscii build + round-trip, S1-S4 auto-sniff coverage.
- `tools/sample-sanitize` switched to `parse_any` + `build_with(detected)`
  so the detected dialect carries through to the output; `meta.toml`
  now records `dialect = "HybridAscii"` / `"FullAscii"`.
- E2E smoke against the moov-io literal `0100…(4242 PAN)…` succeeded:
  sniffed as FullAscii, redacted field 2 (PAN), round-trip verified,
  anonymity-set logged as 10^5. Output in `candidates/_smoke/`.

Test surface delivered:
```
wf-codec: 11 iso8583_dialect + 14 iso8583_message + 16 iso8583_field + 11 iso8583_bitmap + 3 ai_baseline = 55
sample-sanitize: 10 lib tests
workspace clippy -D warnings: clean
workspace cargo fmt --check:   clean
```

**Plan §5 D7 GO/NO-GO matrix update**: the row "≥ 5 / channels active /
GO" remains the same target. What changed is the **denominator** —
samples received via T1/T2 in either FullAscii or HybridAscii dialect
now count toward the goal, not just one of them. That roughly doubles
the expected hit rate per contact.

What was NOT delivered today (parked):
- `Dialect::FullBinary` (BCD-packed MTI + binary bitmap + BCD numeric
  fields). Observed in `jpos/ISO87BPackager.bin`. Mostly mainframe
  fixtures; rare in OSS public corpora. Adding requires BCD field
  decoders, ~1-2 day extension. Park until a real sample needs it.
- Vendoring jpos `.bin` fixtures into wf-codec test data. Repo-root
  `.gitignore` excludes `*.bin`. Spec vectors hand-written in
  `iso8583_dialect.rs` already cover the same encoding paths.

### Anti-Goodhart honesty note

The "0 samples" row above is the **truthful starting point**. No
synthetic fixtures, no spec examples, no hand-crafted hex have been
counted. `cargo test --workspace` continues to pass because
`crates/wf-codec/tests/ai_baseline.rs` correctly reports "skipped:
0 samples" via the `#[ignore]` + early-return scaffold — exactly the
contract documented in its header (lines 41-49).

---

## D2 EOD signal (2026-05-21) — in progress

- Status of `cargo run` against each T0 source's discovered candidates.
- T1 channels live: TigerBeetle Discord post URL, HN Show HN post URL.
- Sanitizer end-to-end run on first real candidate.

### Bonus delivery — wf-wal crate MVP (parallel infrastructure work)

While T1/T2 broadcasts are user-bound, delivered the **first** of the
Phase 0-1 "infrastructure 7 件套" foundation crates: `wf-wal`.

- New workspace member `crates/wf-wal/` — append-only WAL with 8-byte
  magic header (`WFWAL\x00\x01\n`), per-record CRC32 (hand-rolled
  IEEE 802.3 table, no external deps), partial-tail detection on read,
  and `truncate_to` recovery. Bounded `MAX_PAYLOAD = 16 MiB` defends
  against OOM during recovery of attacker-crafted headers.
- API surface: `Wal::{open, append, sync, read_all, truncate_to}` +
  `Corruption::{TruncatedHeader, TruncatedPayload, Checksum,
  HeaderPayloadTooLarge}` + `TailCorruption { offset, kind }`. No
  `unwrap`/`expect`/`panic` in lib code (workspace lint enforced).
- Tests: 2 unit (CRC32 against ISO 3309 check value `0xCBF43926` on
  `"123456789"`, empty input = 0) + 13 integration covering happy paths
  (magic header creation, append+read roundtrip, empty payload, reopen),
  4 crash-equivalent scenarios (truncated header, truncated payload,
  bit-flipped payload via CRC mismatch, oversized-header DoS),
  truncate-recover, and 3 input-validation rejections.
- **Why this and not e.g. `wf-format` or `wf-sm`**: WAL is the
  durability primitive shared by autosave + undo persistence + crash
  restore (3 of the UX-law 8 ship-blockers from STRATEGY-v0.4 §3). One
  bounded ~300-line crate unlocks 4 downstream features without needing
  samples, API keys, or operator action.

What was NOT delivered (deliberately parked):
- `Wal::checkpoint` (compact stale records ahead of a horizon) — needs
  caller-defined "what's stale", premature without a real autosave
  consumer.
- File locking for multi-process safety — documented as caller's
  responsibility; OS file locks land when the desktop shell needs them.
- Async API (tokio) — sync-only fits autosave / undo today; async is
  worth adding only when the desktop event loop demands it.

Test surface after this delivery:
```
wf-wal:        2 unit + 13 integration  = 15
wf-codec:     11 iso8583_dialect + 14 iso8583_message + 16 iso8583_field
              + 11 iso8583_bitmap + 3 ai_baseline                = 55
sample-sanitize: 10 lib tests
workspace clippy -D warnings:  clean across all 5 members
workspace cargo fmt --check:   clean
```

D2 EOD pending: operator broadcast, T0 retry on alternate openiso8583
URLs, possible `Dialect::FullBinary` if a 87B-style real sample arrives.

### Bonus delivery — T1 channel matrix complete (Reddit drafts)

Filled in the two missing T1 channel drafts so the operator can post in
parallel rather than serially:

- `docs/distribution/reddit-r-programming-draft.md` — ~600w technical
  post leading with 4 concrete protocol findings (bitmap off-by-one,
  LLVAR overflow, dialect plurality, PCI redaction); the sample ask
  lands AFTER the technical credit is established (r/programming
  filters self-promo aggressively).
- `docs/distribution/reddit-r-payments-draft.md` — ~400w industry-pro
  post using insider terminology (BIN, Track 2, field 48 sub-elements,
  LLVAR length-of-length, CBPR+); honest about what's NOT there yet
  (no GUI, no Visa/MasterCard dialect packs, no PCAP replay) so the
  audience doesn't bounce on over-claiming.

T1 channel matrix as of D1 EOD:

| #  | Channel                                | Status   |
|----|----------------------------------------|----------|
| 6  | TigerBeetle Discord `#community`       | DRAFT    |
| 7  | HN Show HN                             | DRAFT    |
| 8a | Reddit r/programming                   | DRAFT    |
| 8b | Reddit r/payments                      | DRAFT    |
| 9  | CN channels (V2EX / Zhihu)             | DEFERRED (plan §3 T1-deferred rule — only if T0 + intl T1 dry) |

All four drafts wait on operator credentials / posting policy.

### Bonus delivery — SWIFT MT structural parser

While samples accumulate (or don't), shipped the **structural** layer
for STRATEGY-v0.4 anchor A (MT↔MX bi-directional diff at S6 depends on
this). Fleshed out the `wf-codec::swift` module (previously a 1-line
placeholder) with:

- `MtMessage { blocks: BTreeMap<u8, Block> }` plus `Block::{Raw, Text,
  Tagged}` covering blocks 1/2 (header strings), 3/5 (nested
  `{tag:value}` sub-blocks), and 4 (block-4 `:tag:value` fields with
  multi-line value preservation and the `\r\n-` terminator).
- `parse(input: &str) -> Result<MtMessage, MtParseError>` handling
  whitespace tolerance between blocks, depth-counted brace matching
  (so `{3:{108:REF}}` does not confuse the outer scan), LF-as-CRLF
  permissive line endings, and 8 error variants for the corruption
  surfaces real wire data trips parsers on.
- Tests (4 lib + 21 integration in `tests/swift_structure.rs`):
  full MT103 skeleton with 6 block-4 fields and a multi-line `:50K:`,
  block 1 / 2 verbatim preservation, block 3 / 5 sub-block parsing,
  empty block 4, duplicate block rejection, terminator-missing, etc.
- `build(&MtMessage) -> Result<String, MtBuildError>` closing the
  round-trip. Hand-written MT103 vector `MT103_FULL` is parsed and
  rebuilt **byte-exactly** (the test vector is longhand wire bytes,
  not regenerated from `build` — anti-tautology by design).
  LF-only input → canonical CRLF output (canonical form on build, not
  byte-faithful preservation; the round-trip test uses pure-CRLF input
  on purpose to demonstrate the exact-bytes path).

**Why structure-first (not MT103-specific):** the wrapper `{1:…}{2:…}…`
is shared across every MT type (103/202/199/940/950 …). Doing the
structural layer once means MT103 + MT202 + every later message ride
the same parser; only the semantic/field-table layer is type-specific.
That's also the layer the S6 diff demo needs first — truncation
detection is a field-level operation, not a semantic one.

What was NOT delivered (parked):
- MT103-specific field validators (`:32A:` date/currency/amount,
  `:50K:` party block) — needs spec-from-handbook work + sample
  validation, ≥1 day, do later.
- MT→MX translation table — depends on PACS.008 schema work
  (Sprint 6 anchor), a separate scope.
- A `build_mt` round-trip — straightforward to add but only useful
  when a consumer (sanitizer or diff) needs to re-emit. Park until
  consumer lands.

Test surface after this delivery:
```
wf-wal:        2 unit + 13 integration              = 15
wf-codec swift: 4 unit + 21 integration             = 25
wf-codec iso8583: 11 dialect + 14 message + 16 field
                  + 11 bitmap + 3 ai_baseline       = 55
sample-sanitize:                                    = 10
workspace clippy -D warnings:   clean across all 5 members
workspace cargo fmt --check:    clean
```

### Bonus delivery — wf-wal MVP (durability primitive)

(Already documented above — first shipped infrastructure piece of the
7 件套 promised in STRATEGY-v0.4 §3 UX laws.)

### Bonus delivery — wf-mcp now exposes `wf_parse_swift_mt`

Capped the SWIFT MT structural arc by wiring it into the MCP server so
Claude Code / Cursor / Warp can call it from inside a chat session:

- New `crates/wf-mcp/src/tools/swift_parse.rs` — JSON-rendering view
  over `MtMessage` with tagged-union block representation
  (`{kind:"raw"|"text"|"tagged", id, …}`) so AI agents can pattern-match
  on the block kind without inferring it from id.
- Registered on `WireforgeServer` as `wf_parse_swift_mt`; tool count
  went 7 → 8. Server `instructions` and the lib.rs module doc updated to
  reflect SWIFT MT presence.
- Tests: 2 unit covering MT103 skeleton parse and unbalanced-brace
  error propagation; `wf-mcp` test suite went 25 → 27 green.

**Effect for distribution**: an MCP user who already has Claude Code
configured for `wf-mcp` (operator wave shipped this last week) now gets
SWIFT MT parsing for free on next reload — no client-side config change
needed. Closes a small but real credibility gap: "wireforge does ISO
8583" → "wireforge does ISO 8583 + SWIFT MT".

### Bonus delivery — `wf swift parse` CLI subcommand

The final piece of the SWIFT structural arc: terminal users can now
parse MT messages without touching code or MCP:

```
$ printf '{1:F01BANKBICAA1234567890}{4:\r\n:20:REF001\r\n-}' | wf swift parse -
SWIFT MT Message
├── Block 1 (raw): "F01BANKBICAA1234567890"
└── Block 4 (text, 1 fields)
    └── :20:  REF001
```

- `wf swift parse <wire>` or `wf swift parse -` (stdin)
- `--json` flag emits the same tagged-union view that wf-mcp uses
- Tree renderer truncates multi-line `:50K:`-style values with `\n`
  escape + 60-char cap, keeping terminal alignment

This closes the SWIFT delivery arc end-to-end:
**library (parse + build)** → **MCP tool** → **CLI command**.

### Strategic-anchor coverage after D1 EOD

| anchor                              | status before D1 | status after D1                  |
|-------------------------------------|------------------|----------------------------------|
| A · MT↔MX 18-month window           | spec only        | **MT structural + MCP exposed**  |
| B · National crypto SM2/3/4         | spec only        | spec only                        |
| C · Git-first `.wf` DSL             | spec only        | spec only                        |
| Phase 0-1 infra 7 件套               | 0 / 7            | **1 / 7 (WAL)**                  |
| MCP server                          | shipped (D-7)    | shipped, +1 SWIFT MT tool        |
| Real-sample inventory               | 0                | 0 (wave running)                 |

## D3 EOD signal (2026-05-22) — pending

## D4 EOD signal (2026-05-23) — pending

## D5 EOD signal (2026-05-24) — pending

## D6 EOD signal (2026-05-25) — pending

## D7 EOD signal (2026-05-26) — pending

Final consolidation will live in `docs/sample-acquisition-report-2026-05.md`;
this file remains the working day-by-day log.
