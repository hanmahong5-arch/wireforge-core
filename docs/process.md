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

## 2026-05-26 — v0.0.1 ship + remote + release (Goal A)

Pushed to `hanmahong5-arch/wireforge-core` (public). README's `wireforge`
org didn't exist + can't be CLI-created; chose personal account to
unblock — can transfer later.

- `fcc7a18` LICENSE (Apache-2.0, was missing despite README claim),
  `.github/workflows/release.yml` (tag-triggered linux/macos-aarch64/
  windows binary build), `.github/ISSUE_TEMPLATE/feedback.md`,
  README "5-minute try" section, CI master branch trigger fix,
  Cargo.toml repository URL.
- Tag `v0.0.1` → Actions built 3 binaries + 3 sha256, release page
  published with 6 assets.

Acceptance verified: `cargo install --git <url> wf-cli` from /tmp
shell installed wf v0.0.1, `wf parse` on README hex output MTI 0200
+ bitmap + field 3 correctly.
