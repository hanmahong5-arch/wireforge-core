# Discussions pinned post — real-sample call

**Where**: GitHub Discussions → Announcements (pin it).
**When**: Day 0–1, immediately after the repo goes public.
**Goal**: ≥ 5 sanitized real samples donated (ISO 8583 hex and/or CBPR+ MX XML).
**Posted by**: maintainer, by hand.

---

**Title**: `Call for sanitized real samples — help us close the #1 honesty gap`

**Body** (paste below the line):

---

## The gap, stated plainly

Every accuracy claim wireforge-core makes today is grounded on **synthetic,
spec-derived test vectors only**. No real production SWIFT MT, ISO 20022 / MX,
or ISO 8583 message has ever been validated against this code. That is the
single biggest limitation of the project, and we would rather close it with
your help than paper over it.

Real-world messages carry layout quirks no spec reproduces: national-dialect
field overloads, odd padding, legacy addenda, "creative" address line usage.
Five real samples teach us more than five hundred synthetic ones.

## What we are asking for

Sanitized (redacted) samples of any of the following — **≥ 5 total is our
current milestone**:

1. **ISO 8583** authorization/financial messages, as hex (any dialect —
   ASCII, BCD, EBCDIC-framed all welcome).
2. **CBPR+ MX XML** — pacs.008 / pacs.004 / pacs.003 / pain.001, especially
   ones with **unstructured or partially structured postal addresses**
   (the exact shape the SR2026 2026-11-14 mandate breaks).
3. **SWIFT MT103** raw text, ideally paired with its MX equivalent.

## How to sanitize before sharing

**Never post a raw capture.** Follow
[`docs/sample-policy.md`](../sample-policy.md) — the redaction table there is
non-negotiable (PAN, track data, names, account numbers, merchant IDs all
replaced; structure and framing kept byte-identical).

For ISO 8583 hex, `tools/sample-sanitize/` automates the redaction:

```bash
cargo run --manifest-path tools/sample-sanitize/Cargo.toml -- your-capture.hex
```

For MX XML, replace names/BICs/IBANs/account numbers with obvious synthetic
values but **keep the address block shape exactly as-is** (that shape is the
data we need). If in doubt, open a discussion thread first and we will walk
through the redaction together before anything is shared.

## What you get

- Your sample becomes a named fixture in the conformance suite (credited or
  anonymous — your call).
- Any parse failure it exposes gets fixed with priority and a changelog entry.
- You directly move the project's honesty gate: the README's "no real
  production samples yet" line only changes when this milestone is met.

## Ground rules

- Only share what you are contractually allowed to share. When unsure, don't —
  or ask your compliance team first.
- Maintainers will never ask for unredacted data, credentials, or anything
  traceable to a real customer.
- Samples are accepted here in the thread, or via a private channel if you
  prefer (see profile for contact).

Thank you. Honest reports and ugly real-world samples beat polished stars.
