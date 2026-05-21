# Reddit r/programming post draft

Target subreddit: `r/programming` (~6M subscribers, technical-content first).
Posted by: USER (operator submits; reddit account belongs to operator).
Date drafted: 2026-05-20.
Long-form companion: `docs/blog/debugging-iso8583-tigerbeetle.md`.

## Title (≤ 300 chars; rule-of-thumb: < 80 for skim density)

```
A field guide to parsing ISO 8583 card-payment messages locally, including the bitmap walk and the LLVAR length-prefix gotcha
```

Alternative if the above gets auto-flagged as too "click-baity":
```
ISO 8583 parser in Rust — bitmap walk, LLVAR overflow bug, and what the public samples don't tell you
```

## Link field

```
https://github.com/wireforge/wireforge-core
```

If a moderator asks for a text-only self-post instead, paste the body in
the text box and use the repo URL only inside the body.

## Body (only used for self-post; reddit truncates at ~10k chars,
target ~600 words for readability)

```
I've been writing a small Rust toolchain (wf-codec, wf-cli, wf-mcp) that
parses ISO 8583 — the 1987 message protocol that still moves a large
fraction of the world's card-present transactions — locally, with no
SaaS round-trip. Sharing here because I keep meeting devs who learned
the protocol the hard way (parser eats a 100k-row file and writes
nothing) and the public learning material is thin.

A few things I had to figure out the hard way:

1. The bitmap is the entire spec in 16 bytes. Bit N of the primary
   bitmap tells you whether field N is set, bit 1 is reserved as the
   "secondary bitmap present" indicator (covering fields 65-128), and
   every parser implementation I read in the open had at least one
   off-by-one in iterating set fields. Worth writing this once,
   cleanly, in safe Rust.

2. LLVAR/LLLVAR length prefixes are decimal digits encoded as ASCII
   characters, not binary integers. Easy to confuse, easy to overflow:
   the moov-io issue tracker has a real bug where a 3-digit
   length-of-length got mistakenly treated as a 2-digit one, walking
   the parser off a cliff at field 35.

3. There are at least three dialects in the wild: ASCII MTI + binary
   bitmap (what jPOS calls "Hybrid"), full-ASCII (jPOS 87A / 93A,
   moov-io tests), and full-binary BCD (jPOS 87B, mostly mainframe).
   Most "ISO 8583 parser" libraries only handle one and tell you
   nothing in the README. wf-codec auto-sniffs the first two and
   parks the BCD path until a real sample needs it.

4. PCI redaction is a hard requirement before any sample lands on
   disk. PAN (field 2): keep BIN + last 4, mask middle. Track 2
   (field 35): zero entirely. Cardholder name (field 43):
   length-preserving redaction. The redactor in the repo verifies
   byte-length preservation and round-trip-parse before emitting.

Repo: https://github.com/wireforge/wireforge-core
Apache-2.0. No telemetry, no cloud upload, parses entirely in-process.

The reason I'm posting here (rather than waiting for a polished
release): the hardest unsolved problem is **finding real-shape
sanitized samples**. Every public hex blob I can find is a tutorial
"hello world" 0200 with three fields; real production traffic sets
8-20 fields and exercises the LLVAR / vendor-private / secondary
bitmap paths that tutorials skip. If anyone here works in payments
and has the means to share sanitized captures, there's a redaction
tool in the repo I can point at; bytes never have to leave your
machine and I only take the output. DM or comment works.

(Tech detail: wf-bitmap and wf-codec are separate crates so the
bitmap implementation is independently testable and the field
table is generated from the spec; wf-mcp exposes the parser as
a Model Context Protocol tool so Claude / Cursor / Warp can call
it from inside a chat. The MCP piece I shipped first because
existing MCP servers around payments are zero today, which is a
small but real distribution moat.)
```

## Notes for operator

- r/programming is moderated for substance. The draft leads with three
  concrete technical findings (bitmap off-by-ones, LLVAR overflow,
  dialect plurality) before the project link, which is what survives
  the "is this just self-promo?" filter.
- The "auto-sniffs the first two and parks the BCD path" line is
  defensible because the parker is in code (`crates/wf-codec/src/iso8583/dialect.rs`).
  If a commenter asks "why not BCD too", the honest answer is "no real
  sample has hit the bug surface yet — I prefer parking work over
  shipping untested" — that's a respectable r/programming answer.
- Best window: weekday 09:00-12:00 US Eastern (Eastern weekday traffic
  peaks higher than Pacific for technical subs).
- Do NOT cross-post the same title to r/rust or r/cscareerquestions —
  r/programming's mods downrank cross-posters. r/rust is a separate
  draft if we want it later.
- Comment-reply policy: stay 1-2 hours post-submit. Top-of-thread
  technical questions ("why this hash polynomial / why this bitmap
  walk / how does FullAscii sniff work") deserve linked answers; the
  source files are self-contained enough to link line ranges.
- If accused of being AI-generated: the code is hand-written, the
  blog draft was AI-assisted, strategy docs are human-led with AI
  critique. Honest disclosure converts skeptics faster than denial.
