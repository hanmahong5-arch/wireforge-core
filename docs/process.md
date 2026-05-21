# process.md — Wireforge development log

Append-only ≤15-line summaries of multi-step work. Diffs live in
`git log`; the "why" lives in `E:\fintx\` strategy memos.

## 2026-05-21 — Parallel anchor-A / anchor-B / anchor-C kick-off

Four independent increments on top of `git init` baseline (5 commits).

- `4ff1773` ISO 8583 FullBinary BCD dialect; closes 3rd OSS dialect.
  Sanitizer parks BCD PAN redaction. +12 dialect / +2 sanitize tests.
- `fd7be19` SWIFT MT semantic decoders 20/32A/50K behind
  `MtFieldDecoder` trait + Raw fall-through. +25 unit / +7 integration.
- `9d93475` `wf-sm` new crate; SM3 via smcrypto 0.3.1 (chose over
  plan-named gmsm after upstream attribution check). Measured 56-98
  MB/s vs plan estimate 150 — recorded honestly. +6 tests + 4 benches
  + `docs/sm-crypto-research-2026-05.md`.
- `e09c992` `wf-format` new crate; `.wf` Bruno-style DSL with
  brace-tracked values + raw fall-through. +14 tests.

Gate per commit: cargo test + clippy `-D warnings` + fmt all green;
manual smoke walked all 4 hands-on against spec vectors.
