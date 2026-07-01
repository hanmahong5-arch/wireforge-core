# Wireforge v0.1.0 Publish Runbook

**Audience**: the maintainer holding the `CARGO_REGISTRY_TOKEN` crates.io token.
**Scope**: one-way checklist — execute top to bottom. Steps are manual fallback;
the automated path via CI is noted inline.

---

## 1. Pre-flight checks

Run all three gates locally before touching tags or publishing. All three must be
green before proceeding.

### 1a. Clippy — zero warnings, zero errors

```bash
"C:/Users/Anita/.cargo/bin/cargo.exe" clippy \
  --workspace \
  --manifest-path E:/wireforge/wireforge-core/Cargo.toml \
  --all-targets -- -D warnings
```

Expected: `Finished` with exit code 0. Any `error[clippy::]` line = stop and fix.

### 1b. Test suite — all pass

```bash
"C:/Users/Anita/.cargo/bin/cargo.exe" test \
  --workspace \
  --manifest-path E:/wireforge/wireforge-core/Cargo.toml
```

Expected: every `test result` line reads `ok. N passed; 0 failed`. The known
ignored tests (2 in `parse_accuracy` blocked on real samples; 4 throughput
benches in `sm3_throughput`) are expected to remain ignored — that is fine.
As of 2026-06-01: **378 passed / 0 failed / 6 ignored**.

### 1c. Dry-run each leaf crate

The six leaf crates have no internal `wf-*` path dependencies, so they can be
dry-run individually to catch missing `description`, `keywords`, or metadata
issues before any real upload.

```bash
for crate in wf-bitmap wf-format wf-wal wf-sm wf-mx wf-obs; do
  echo "--- $crate ---"
  "C:/Users/Anita/.cargo/bin/cargo.exe" publish \
    --dry-run \
    --manifest-path "E:/wireforge/wireforge-core/crates/$crate/Cargo.toml" \
    --locked
done
```

Expected: `Uploading` line per crate, no `error` lines.

---

## 2. crates.io publish — dependency order

> **Automated path**: pushing the `v0.1.0` tag (step 3) triggers
> `.github/workflows/publish.yml`, which runs this sequence on `ubuntu-latest`
> using the `CARGO_REGISTRY_TOKEN` Actions secret. If CI is green and the secret
> is set, you do not need to run the manual commands below.
>
> **Manual fallback**: if CI is unavailable or you need to publish outside of
> GitHub Actions, run the commands below from a Linux or macOS machine with the
> `CARGO_REGISTRY_TOKEN` environment variable set to your crates.io API token.
> On Windows the same commands work in Git Bash with the variable exported.

The order is driven by the dependency graph in `publish.yml`. A crate cannot be
published until every internal `wf-*` crate it depends on is already indexed on
crates.io. The 30-second sleeps give the sparse index time to propagate.

### Tier 1 — pure leaves (no internal wf dependency)

```bash
export CARGO_REGISTRY_TOKEN=<your token>

for crate in wf-bitmap wf-format wf-wal wf-sm wf-mx wf-obs; do
  cargo publish --locked -p "$crate"
  sleep 30
done
```

Dependency map for reference:
- `wf-bitmap` — no internal deps
- `wf-format` — no internal deps
- `wf-wal` — no internal deps
- `wf-sm` — no internal deps
- `wf-mx` — no internal deps (wraps `mx-message = "=3.1.4"`)
- `wf-obs` — no internal deps (tracing helpers only)

### Tier 2 — dependents (publish in this exact order)

```bash
for crate in wf-codec wf-swift wf-xform wf-oracle wf-cli wf-mcp; do
  cargo publish --locked -p "$crate"
  sleep 30
done
```

Why this order:
- `wf-codec` depends on `wf-bitmap`
- `wf-swift` depends on `wf-codec`
- `wf-xform` depends on `wf-swift`, `wf-mx`, `wf-codec`
- `wf-oracle` depends on `wf-codec`
- `wf-cli` depends on all of the above (incl. `wf-oracle`, `wf-obs`)
- `wf-mcp` depends on all of the above (incl. `wf-obs`)

If `cargo publish` fails with `crate ... not found` for a just-published dep,
increase the `sleep` to 60 seconds and retry from the failing crate.

---

## 3. Tag and GitHub release

```bash
git tag v0.1.0
git push --tags
```

Pushing the `v*` tag triggers two independent CI jobs:

- **`publish.yml`** — runs the Tier 1 + Tier 2 publish sequence above.
- **`release.yml`** — builds `wf` (CLI) and `wf-mcp` (MCP server) release
  binaries for three targets and creates a GitHub Release with attached archives:
  - `wf-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` (+ `.sha256`)
  - `wf-v0.1.0-aarch64-apple-darwin.tar.gz` (+ `.sha256`)
  - `wf-v0.1.0-x86_64-pc-windows-msvc.zip` (+ `.sha256`)

Do NOT push the tag until pre-flight (step 1) is fully green. Tags cannot be
re-used without a force-push, which would invalidate SHA-256 checksums already
consumed by the MCP registry.

---

## 4. MCP registry submission

See `docs/distribution/mcp-registry-submission.md` for the full mcpb bundle
procedure (binary + manifest + SHA-256 from the GitHub Release). Do not duplicate
that content here. The submission depends on the `release.yml` artifacts being
published first (step 3 must complete before mcpb can reference the release URL).

---

## 5. Launch sequence

The strategy document (`docs/strategy/next-steps-2026-06.md`) specifies this
channel order and angle. Follow it exactly — do not lead with parser breadth or
benchmarks.

**Content angle**: the MT103↔pacs.008 field-truncation **detector** and the
SWIFT coexistence-era field-loss problem. The SR2026 address-mandate deadline
(2026-11-14) is a concrete urgency hook.

**Mandatory honesty disclosure** — include this verbatim (or a close paraphrase)
in every post:

> "All test vectors are spec-derived synthetic data. No real production SWIFT MT
> or ISO 8583 samples have been validated. This is a known limitation stated
> openly."

### Channel order

- [ ] **r/rust** — post first; Rust engineers are the early adopter audience.
      Target the `/r/rust` "Show r/rust" flair. 600–900 words.

- [ ] **lobste.rs** — requires an invitation from an existing member. Source the
      invite before the r/rust post goes live so you can follow up within 24–48
      hours. Do not skip this step — lobste.rs carries strong signal-to-noise for
      systems programmers.

- [ ] **Show HN** — submit after r/rust has had time to surface organic signal
      (typically 24–72 hours). Title must fit HN's 80-char limit; lead with the
      truncation-detector angle, not the library count. The existing draft at
      `docs/distribution/hn-show-hn-draft.md` can be adapted.

### What NOT to lead with

Do not lead with: "supports 10 crates", "MCP server", "EBCDIC", benchmarks, or
SM2/3/4 cryptography. These are supporting facts, not the story.

---

## Checklist summary

```
[ ] cargo clippy --workspace -- -D warnings  → exit 0
[ ] cargo test --workspace               → 0 failed
[ ] cargo publish --dry-run (5 leaves)   → no errors
[ ] git tag v0.1.0 && git push --tags    → triggers CI
[ ] Confirm publish.yml green (crates.io)
[ ] Confirm release.yml green (GitHub Release artifacts)
[ ] Complete mcpb submission (see mcp-registry-submission.md)
[ ] Source lobste.rs invite
[ ] Post r/rust
[ ] Post lobste.rs
[ ] Post Show HN
```
