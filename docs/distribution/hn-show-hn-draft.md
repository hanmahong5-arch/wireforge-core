# Show HN draft — wf-cli + sanitized-sample request

Target outlet: Hacker News `Show HN` category.
Posted by: USER (operator submits; HN does not allow automated posting).
Date drafted: 2026-05-20.
Long-form companion: `docs/blog/debugging-iso8583-tigerbeetle.md`.

## Title (≤ 80 chars; HN strips trailing punctuation)

```
Show HN: wf-cli – local-first ISO 8583 parser, looking for sanitized samples
```

## URL field

Link to the GitHub repo (NOT the blog post — HN downranks blog posts in
Show HN). Repo:

```
https://github.com/wireforge/wireforge-core
```

## Text body (HN strips most formatting; plain text + URLs only)

```
Hi HN — wf-cli is a small Rust CLI + MCP server I've been building for
folks who deal with ISO 8583 (card-payment messages) and want to
parse/inspect them locally instead of pasting hex into a SaaS form.

What's there today:
- wf-codec: ISO 8583 parser/builder, ASCII variant, 105 well-defined
  fields from the 1987 spec, round-trip-verified.
- wf-cli: command-line parse/decode tool, terminal-only, no network.
- wf-mcp: MCP server so Claude / Cursor / Warp can invoke the parser
  as a tool from inside a chat session.

Repo + docs: https://github.com/wireforge/wireforge-core

I'm posting here mainly because the next thing I need can't be solved
by writing more code: **real-shape sanitized samples to test against.**

Every public ISO 8583 fixture I can find is a tutorial-grade "hello
world" 0200 with three fields. Real production messages set 8-20
fields, exercise LLVAR/LLLVAR length prefixes, carry vendor-specific
field 48 sub-elements, and have dialect quirks (CUPS, Visa, MasterCard).
None of that is testable without samples.

If you work in payments and can share a few hex captures with PAN
masked, Track 2 zeroed, merchant name generic, byte-length preserved
— I'd be grateful. There's a redaction tool in the repo
(tools/sample-sanitize) that does parse → redact → rebuild →
round-trip-verify on-device; I'm happy to run it on bytes that
never leave your laptop and only take the output.

License: Apache-2.0. Local-first by design — no telemetry, no
cloud upload. Contributions and feedback welcome; samples doubly so.

(Tech notes: parser is in safe Rust, uses a static field-def table
generated from the 1987 spec; the bitmap implementation is a separate
crate so it's swappable; MCP server is the same parser exposed as a
tool, not a re-implementation.)
```

## Notes for operator

- HN Show HN guidelines: post must be something the audience can try
  themselves. "Repo + docs + a CLI you can `cargo install`" passes.
- Best window for Show HN: 06:00-09:00 Pacific weekdays (HN front-page
  algorithm boosts early-day posts that catch organic upvotes).
- Don't pre-seed votes — HN flags voting rings aggressively and the
  account would lose Show HN privileges. Submit, walk away, check
  in 30 minutes.
- If asked "is this AI-generated", answer honestly: the code is
  hand-written, the blog draft was Claude-assisted, the strategy
  doc is human-led with AI critique. HN tolerates honest disclosure;
  it does not tolerate ghost-written claims.
- Reply window: stay on the thread for ~2 hours after submit to answer
  technical questions. After that, monitor for daily digest.
