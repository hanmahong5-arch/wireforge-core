# Sample Policy

Real-world banking messages (ISO 8583, SWIFT MT, EBCDIC dumps) are required to
develop and benchmark `wireforge-core`, but they carry regulatory, contractual,
and customer-trust risk. This document is the project-wide rule for how
samples are obtained, sanitized, stored, and shared.

## TL;DR

- **Never commit raw samples to git.** `samples/.gitignore` excludes every
  file by default; only the directory shape (`samples/iso8583/.gitkeep`) is
  versioned.
- **Real samples live out-of-tree**: developer laptop, encrypted shared
  bucket, or git-lfs with a restricted ACL. The exact transport is decided
  per-engagement, not in this repo.
- **Sanitize before sharing**, even with teammates. Redaction rules below
  are non-negotiable.

## Why no public commits

1. **Regulatory.** PAN (primary account number), expiry, CVV-equivalent data
   are governed by PCI-DSS. Account numbers, names, and remitter/beneficiary
   pairs may fall under GDPR / PIPL. A public git history makes redaction
   impossible after the fact.
2. **Contractual.** Bank-channel access agreements typically prohibit
   redistribution of message captures, even structurally.
3. **Trust.** A leaked sample — even if "just test data" — is a brand event
   for both Wireforge and the source institution.

## Redaction rules (mandatory before any sharing)

Apply these to every field before a sample leaves a controlled environment:

| Field type            | ISO 8583 examples         | Rule                                                |
|-----------------------|---------------------------|-----------------------------------------------------|
| PAN                   | Field 2                   | Replace all but first 6 + last 4 digits with `X`.   |
| Track 2 data          | Field 35                  | Replace entirely with `XXXX...` of original length. |
| Cardholder name       | Field 43 (positions vary) | Replace with `JOHN DOE` (preserve length/charset).  |
| Account numbers       | Field 102 / 103           | Replace with `0` runs of original length.           |
| Merchant identifiers  | Field 41 / 42             | Replace with `WIREFORGE_TEST` (truncated to len).   |
| Timestamps            | Field 7 / 12 / 13         | OK to keep — non-identifying.                       |
| Amounts               | Field 4                   | OK to keep — non-identifying once accounts gone.    |
| Free-text / addenda   | Field 48 / 60-63          | Manually inspect; redact any name/address/email.    |

After redaction:

- The MTI, bitmap, and structural framing MUST remain byte-identical to the
  original. The whole point of a sample is to reproduce real-world layout
  quirks; over-redacting structure defeats it.
- Save as `<source-tag>-<index>.hex` (e.g. `bank-a-001.hex`), one hex
  string per file, whitespace allowed for readability.

## Contributing a sanitized sample

1. Capture the message in a controlled env (your laptop, a jump-box you own).
2. Apply the redaction table above. Use `wf-codec` itself (once available)
   to round-trip the redacted bytes and confirm the structure still parses.
3. Place the file under `samples/iso8583/`. It will be git-ignored locally.
4. Upload to the team's encrypted bucket / lfs store per the channel
   agreement; share the path in the PR description, not the file itself.
5. In the PR, note the source tag, count, and any structural notes
   ("this batch exercises field 48 sub-elements") — but **no payload text**.

## Local dev: running tests against samples

```bash
# 1. Drop one or more sanitized files:
cp ~/my-secure-store/bank-a-001.hex E:/wireforge/wireforge-core/samples/iso8583/

# 2. Export API keys if running AI baseline:
export ANTHROPIC_API_KEY=sk-ant-...
export DEEPSEEK_API_KEY=...

# 3. Run the (currently ignored) baseline harness explicitly:
cargo test --test ai_baseline -- --ignored --nocapture
```

If `samples/iso8583/` is empty the harness exits with a `skipped: ...`
message — by design, never a fabricated "0 samples → 0% accuracy" result.

## Review trigger

Any PR that adds, modifies, or removes files outside the
`samples/.gitignore` allow-list MUST be reviewed by a maintainer with
explicit "sample-policy" sign-off. CI will reject new tracked files under
`samples/` that are not in the allow-list.
