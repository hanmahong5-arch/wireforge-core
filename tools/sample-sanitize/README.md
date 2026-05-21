# sample-sanitize — ISO 8583 redaction tool

Internal tool, not a public crate. Lives outside the `wireforge-core`
workspace on purpose (own `[workspace]` table in `Cargo.toml`) so
`cargo test --workspace` in the parent never sees it.

## What it does

Reads a hex-encoded ISO 8583 wire capture, runs it through
`wf-codec::iso8583::parse`, applies the redaction rule table from
[`../../docs/sample-policy.md`](../../docs/sample-policy.md) to each
PII-bearing field, re-builds via `wf-codec::iso8583::build`, verifies
the byte length is unchanged and the round-trip parses identically,
then emits the redacted hex and a `meta.toml` audit record.

Anything the parser rejects is REJECTED. Anything whose round-trip
fails after redaction is REJECTED. The tool refuses to emit bytes
that didn't survive both gates — by design, per the project's
"sanitization corrupts byte-length / structure" risk mitigation.

## Build

```
cd tools/sample-sanitize
cargo build --release
```

Standalone workspace — does NOT trigger a wireforge-core rebuild.

## Use

```
target/release/sample-sanitize candidates/jpos/auth-request.hex.raw \
    --source jpos-1.10.0 \
    --source-url https://github.com/jpos/jPOS/blob/v1.10.0/jpos/src/test/resources/foo \
    --source-commit <sha> \
    --license Apache-2.0 \
    --fetched-at 2026-05-21T10:00:00Z \
    --sanitized-at 2026-05-21T10:05:00Z \
    --notes 'exercises field 48 sub-elements' \
    --out ../../samples/iso8583/jpos-001.hex
```

Output:

- `../../samples/iso8583/jpos-001.hex` — redacted hex, one line.
- `../../samples/iso8583/jpos-001.meta.toml` — audit record.

`samples/` is git-ignored at the repo root, so no risk of accidentally
committing the output.

## Anonymity-set semantics

The `anonymity_set_size` field in `meta.toml` records the number of
Luhn-valid full-PAN completions consistent with the masked digits in
field 2 of the redacted sample. For a 16-digit PAN with first-6 + last-4
preserved, this is `10^(6-1) = 100_000`.

This is an **anonymity property, not an irreversibility property.** It
defends against a casual reader who only has the redacted sample; it
does NOT defend against an adversary who also holds acquirer-side
transaction logs that can re-identify by amount/timestamp/STAN.

Samples whose anonymity-set size falls below `10_000` get a warning
flag in the sanitizer's stderr summary; operators should review those
samples before sharing externally.

## Tests

```
cd tools/sample-sanitize
cargo test
```

Tests use a synthetic ISO 8583 message built via `wf-codec::iso8583::build`
to validate the sanitize → round-trip → re-parse contract. Per
CLAUDE.md §4.1 ③ ("测量与被测不可同源"), this is acceptable: the
synthetic input drives the *sanitizer code path*, not the AI baseline
that the sanitized samples will eventually feed into.
