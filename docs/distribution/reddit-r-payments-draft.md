# Reddit r/payments post draft

Target subreddit: `r/payments` (~13k subscribers, payments professionals).
Posted by: USER (operator submits; reddit account belongs to operator).
Date drafted: 2026-05-20.
Long-form companion: `docs/blog/debugging-iso8583-tigerbeetle.md`.

## Title (≤ 300 chars; keep < 70 for mobile readability)

```
Local-first ISO 8583 / SWIFT toolchain (OSS, Apache-2.0) — looking for sanitized samples from anyone who's debugged production card auth
```

Alternative more direct:
```
OSS ISO 8583 parser with auto-dialect sniffing and PCI-safe redaction — need sanitized samples from production payments folks
```

## Link field

```
https://github.com/wireforge/wireforge-core
```

## Body (target ~400 words; payments folks read carefully but
quickly — front-load the value, end with the ask)

```
Hi r/payments — building an OSS toolchain (wireforge) for the part of
the job that doesn't have great dev tools today: parsing, diffing,
and validating ISO 8583 + SWIFT MT/MX messages locally, no SaaS
upload, no telemetry, Apache-2.0.

What's there now (as of this week):

- ISO 8583 parser/builder, auto-sniffs between the two ASCII dialects
  (jPOS 87A/93A "full ASCII" and the hybrid "ASCII MTI + binary
  bitmap" variant). BCD-packed binary dialect is parked until a real
  sample needs it — happy to keep that honest.
- PCI redaction tool: parse → mask PAN (BIN + last 4 only) → zero
  Track 2 / 35 / 45 → length-preserving redact on cardholder name and
  merchant → re-build → verify round-trip = original byte length.
  Anonymity-set size logged in the sanitizer metadata so you can
  see exactly how many decoy PANs a masked record sits inside.
- MCP server so Claude / Cursor / Warp can call the parser as a tool
  inside a chat session. Useful for the workflow where you paste a
  hex blob into the assistant and ask "what does this auth request
  actually carry" — answer comes from real parsing, not the LLM
  hallucinating field names.

Repo: https://github.com/wireforge/wireforge-core

Why I'm posting here specifically:

The hardest unsolved problem is **real-shape sanitized samples**.
Tutorial fixtures on the open web are all "hello world" 0200 with
three fields; production traffic sets 8-20 fields, uses field 48
sub-elements that vary per acquirer, and trips parsers on LLVAR
length-of-length boundaries that synthetic tests miss.

If anyone here has worked through "we lost an auth in transit and
need to diff what we sent vs what the network received" and could
share a sanitized capture, the redaction tool can run on-device so
the unredacted bytes never leave your machine — I only need the
output. Anything contributed gets a credit line in the docs, or
stays anonymous, your call. DM works.

Roadmap context if it's interesting: MT↔MX bi-directional diff is
the next big build (Month 2-3) to catch in-flow translation
truncation while the ISO 20022 CBPR+ window is still open;
SM2/3/4 (national crypto) integration is on the same arc for the
信创 segment, mostly because nobody in the OSS world has done it
and the compliance regulator window is closing fast.

Honest about what's NOT there yet: no GUI, no Visa/MasterCard
field-48 dialect packs, no PCAP replay (Sprint 15). This is a
parser + sanitizer + MCP server in a corner of a strategic roadmap,
shipped early because validation > polish.

License Apache-2.0. Feedback (especially "this parser is wrong about
my field N") and sample contributions equally welcome.
```

## Notes for operator

- r/payments is small, professional, and tolerant of industry tool
  posts — but readers are paid practitioners, not enthusiasts. The
  draft uses insider terminology (BIN, Track 2, field 48 sub-elements,
  LLVAR length-of-length, MX, CBPR+) on purpose; the audience filters
  out vague pitches fast.
- Mention of **"信创"** (xinchuang — domestic-tech-stack) is in there
  because r/payments has CN-banking-IT readers who recognize it. If
  posting feels too niche, the line is trimmable; the SM2/3/4 mention
  carries the same signal in English.
- The "BCD parked until a real sample needs it" line is calibrated:
  it tells production-grade readers we're not over-claiming, and it
  invites a contribution ("I can give you a 87B-style sample if it
  unblocks the work").
- DO NOT cross-post the same body to r/cscareerquestions, r/fintech,
  or r/devops — different cultures, different titles needed. r/fintech
  is a separate draft if we want it later (~150k subs, less technical,
  more "what does this tool do" framing).
- Reply window: payments folks read on lunch breaks; first 3-4 hours
  post-submit is when DMs and comments will land. After ~6 hours the
  post falls off page 1.
- If asked about license: Apache-2.0, no CLA today, contributions
  by PR. If asked about telemetry: literally zero, no analytics SDK,
  no phone-home — grep `crates/` for `reqwest` and `surf` to verify.
