# SYNTHETIC — NOT PRODUCTION

Every file in this directory is **hand-authored synthetic test data**. None
of it is a real captured banking message. It exists so a new user can run the
`wf` CLI end-to-end immediately, without waiting on the real-sample channel
(which is **blocked** — see below).

## What's here

| file                       | what it is                                              |
|----------------------------|---------------------------------------------------------|
| `*.json`                   | the authored source: an MTI + field map (`wf build` input) |
| `*.hex`                    | the wire bytes rendered from that JSON by `wf build`    |

Messages:

- `0200-auth-request` — POS authorization request. PAN is `4242424242424242`,
  the **public Stripe test card** (not a real account). Amount/STAN/terminal
  are placeholder values.
- `0210-auth-response` — the matching authorization response (adds field 39
  response code `00` = approved).
- `0800-network-mgmt` — a network-management / echo message (exercises the
  secondary bitmap via field 70).

## Try it

```sh
# parse the rendered wire bytes into a field tree
wf parse "$(cat 0200-auth-request.hex)"

# rebuild the wire bytes from the JSON source (reproduces the .hex)
wf build < 0200-auth-request.json
```

## Honesty boundaries

1. **These are demo fixtures, NOT correctness evidence.** The `.hex` files were
   produced by `wf build` and parse back with `wf parse`. That round-trip shows
   the codec is **internally self-consistent** (`parse ∘ build` is the identity
   on the field set) — it does **not** prove the wire format matches what a real
   acquirer/switch emits. Measuring against real-world layout quirks is a
   separate thing, and it is blocked.

2. **Real-sample accuracy is `⏳ 待验证 (blocked: ≥5 real samples)`.** The
   parse-accuracy baseline (`crates/wf-codec/tests/parse_accuracy.rs`) loads only
   from `samples/iso8583/` (real, sanitized, git-ignored) and stays `#[ignore]`
   until ≥5 real production-shape messages land there. Synthetic data is
   explicitly **not** accepted by that experiment — feeding it self-generated
   bytes would be a tautology, not a measurement.

3. **Never put real samples here.** Real captures go to `samples/iso8583/`
   (git-ignored) under the redaction rules in [`docs/sample-policy.md`](../../../docs/sample-policy.md),
   and out-of-tree per the channel agreement. This `synthetic/` tree is the only
   sample data the repo commits.
