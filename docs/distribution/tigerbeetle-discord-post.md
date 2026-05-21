# TigerBeetle Discord — `#community` post draft

Target channel: TigerBeetle Discord `#community`.
Target length: ~600 words (Discord visual cap before "click to expand").
Posted by: USER (operator must paste manually — bot relay not appropriate).
Date drafted: 2026-05-20.
Long-form blog companion: `docs/blog/debugging-iso8583-tigerbeetle.md` (1016 words).

The long blog post is the *artifact* the Discord post points to. The Discord
text below is the **invitation + sample ask**, not a duplicate of the blog.

---

## POST BODY (paste from here down)

Hey folks — I've been writing a small toolchain (`wf-cli`, `wf-mcp`) for
folks who already use TigerBeetle as the ledger underneath a card processor
and have to live with the ISO 8583 side of the pipeline. Sharing here
because the audience overlap is unusually high and I'd love feedback.

**The piece itself** is a field guide on going from a raw ISO 8583
authorization request to a TigerBeetle `create_transfers` call without
losing byte-fidelity, including the parts that are usually skipped:

- how to read the bitmap and walk the 1987 field table (105 well-defined
  slots, the rest are reserved-private),
- what changes when you encounter Track 2 / Track 1 (field 35 / 45) and
  why straight-through processors usually screw up the redaction step,
- a worked example that decodes a hex blob, identifies a real-world
  parser bug (LLVAR length overflow), and fixes it with one
  `create_transfers` per leg so debit/credit stays balanced in the
  ledger.

Repo: <https://github.com/wireforge/wireforge-core>
Long-form draft: <link to blog draft once published>

The toolchain itself is local-first (no cloud round-trips, no telemetry,
parses entirely in your machine). It exists because every ISO-8583
"playground" out there is a SaaS form that wants your hex pasted into
their backend, which is the opposite of what you want when you're
debugging a real auth request that hasn't been redacted yet.

---

**Honest ask, which is the actual reason I'm posting:**

The hardest part of building `wf-codec` (the ISO 8583 parser library) has
been **finding real-shape sanitized samples** to test against. Every
tutorial blob on the open web is a "hello world" 0200 with three fields;
real production messages set 8–20 fields and exercise LLVAR/LLLVAR length
prefixes, vendor-specific field 48 sub-elements, and dialect-specific
quirks (CUPS vs Visa vs MasterCard).

If anyone here works in payments and has the means to share **a few
sanitized hex captures** — PAN masked, Track 2 zeroed, merchant name
generic, byte-length preserved — I would owe you a beer. I'm strict
about not asking for raw production data: there's a redaction tool in
the repo (`tools/sample-sanitize`) that does parse → redact → rebuild
→ round-trip-verify on-device, and I'm happy to run it on bytes
that never leave your laptop and just take the output.

The `wf-codec` parser is BSD/Apache-2.0, the sanitizer too. Anything
contributed gets a credit line in the docs (or stays anonymous, your
call).

DM works, or comment here. Thanks for reading. ⚙️

---

## NOTES FOR OPERATOR (do NOT paste below the line above)

- TigerBeetle Discord etiquette: technical, on-topic, no spam. This post
  meets that bar because the technical content is real (LLVAR length
  overflow story is from the moov-io issue tracker, verifiable). The
  sample ask is at the END, not the top, on purpose.
- If a moderator asks for trim, the trimmable section is the second
  paragraph ("how to read the bitmap..."). The ask paragraph is the
  load-bearing one.
- Do NOT cross-post to `#help` or `#general` — community is the right
  surface for "I built a thing, looking for feedback".
- Do NOT @ anyone.
