# Wireforge — Next-Steps Plan (2026-06)

Source: a 4-stream web research swarm (validation-sample sourcing, competitive
state, wedge defensibility, adoption path), **24 claims researched → 19 survived
adversarial source-verification** (cite-or-kill; 5 claims dropped as fabricated
or inflated — see the appendix). Every assertion below traces to a verified,
cited primary source. Items the research could not establish are in
**Honest Unknowns**, not asserted.

> Status at time of writing: v0.1.0 code is publish-ready — `cargo clippy
> --workspace -D warnings` = 0, `cargo test --workspace` = 366 passed / 0 failed
> / 6 ignored, all 5 leaf crates pass `cargo publish --dry-run`. The work below
> is the product/go-to-market layer, **not** more code hardening (the parser is
> already hardened across three adversarial audits + a property-fuzz suite;
> further breadth investment is explicitly discouraged).

## Executive summary

Wireforge holds a **defensible, currently-unique** position: the only Rust-native
MCP server for ISO 8583 + SWIFT MT/MX + a MT103↔pacs.008 **field-truncation
detector**. The MCP fintech slot is empty across every major registry today. The
single biggest risk is **temporal, not technical** — GoPlasmatic has an "Orion"
MCP pipeline not yet pointed at message formats; the first-mover window may close
in 6–18 months. The single biggest credibility gap is the **absence of real
production samples** (all vectors are spec-derived synthetic) — this must be
disclosed proactively, never hidden.

## What only the user can fire (credentials / outward-facing)

1. **Publish v0.1.0 to crates.io** (dependency order in `.github/workflows/publish.yml`)
   and push the `v0.1.0` tag to trigger `release.yml` binaries. Needs
   `CARGO_REGISTRY_TOKEN`. *Effort low · leverage high · time-sensitive.*
2. **Submit `wf-mcp` to the MCP registry via the mcpb path** — crates.io is **not**
   a supported registry type (verified: only `npm`/`pypi`/`nuget`/`oci`/`mcpb`).
   The route for a Rust MCP server is an `mcpb` bundle (compiled release binary +
   manifest, hosted on a GitHub Release, SHA-256 in `server.json`). *Low · high.*
3. **Launch post** (600–900 words) centered on the **truncation-detector design
   and the SWIFT field-loss problem** — NOT parser breadth or benchmarks. Sequence:
   r/rust → lobste.rs (source an invite first) → Show HN. Must proactively state
   "spec-derived synthetic test vectors, not real production samples." *Medium · high.*

## Code-actionable (can be done in-repo; recommended order)

4. **Vendor GoPlasmatic's Apache-2.0 synthetic fixtures** (195 `SwiftMTMessage`
   scenarios + ~191 `MXMessage` scenarios — the *same* upstreams `wf-swift`/`wf-mx`
   already wrap). Place under `tests/fixtures/` with a NOTICE per file:
   source repo URL, Apache-2.0, and the label *"spec-derived synthetic /
   CBPR+ SR2025-compliant / not real SWIFT network traffic."* Run `wf-xform`'s
   detector over all MT103↔pacs.008 pairs and publish the report as a CI artifact.
   *Effort low · leverage high.* **Caveat (why this needs a deliberate go-ahead):**
   the scenarios are `datafake`-style *templates* with generator nodes, not
   ready-to-parse strings, so this requires running GoPlasmatic's generator (or
   hand-instantiating) — a real provenance + tooling decision, not a file copy.
   **It partially closes the launch-credibility gap but does NOT substitute for
   real samples** (synthetic can't close that gap — see Unknown #6).
5. **README "Scope & Limitations" section** (added this round): synthetic vectors
   only; detector ≠ converter, no certification claim; SM2/3/4 functional but not
   密评-certified; `wf-mx` rides a frozen upstream (`mx-message` 3.1.4, frozen
   Oct 2025). *Low · medium — trust with technically sophisticated early adopters.*
6. **crates.io discoverability**: category `finance`; keywords drawn from
   `iso-8583`, `swift-mt`, `iso-20022`, `ebcdic`, `mcp`, `financial-messaging`,
   `sm2`. Ensure docs.rs renders cleanly (auto-builds on publish; lib.rs indexes
   within hours). *Low · medium.*
7. **GitHub Discussions** — pinned "Real Sample Validation" thread asking early
   users for sanitized ISO 8583 hex / MT103 strings via the existing
   `tools/sample-sanitize` crate. Public goal: *Phase 0 exit gate = ≥ 5 real
   ISO 8583 hex samples.* *Low · medium — the only honest path to close Unknown #6.*

## Bigger bets (v0.2 candidates)

8. **`wf_mx_address_compliance`** — ✅ SHIPPED (2026-06-02; pacs.008.001.08 first,
   then extended to pacs.004.001.09 under one unified auto-detecting tool). A
   structured-address checker: does the message carry Town Name + Country Code in
   dedicated structured fields (mandatory from **2026-11-14** per CBPR+ SR2026)?
   ~65% of messages still carry unstructured addresses in early 2026; no OSS
   Rust/MCP tool covers this audit. Extends the detector's positioning from
   "coexistence-era tool" to "SR2026 compliance tool" with a concrete
   deadline-driven demand signal. *Medium · high.* (camt.056 is a later, 2027
   milestone — see Unknown #3.)
9. **Complementary-positioning outreach** to moov-io and GoPlasmatic communities:
   Wireforge *adds* an MCP layer + truncation detection, it does not compete on
   parser breadth (it wraps GoPlasmatic). Honest, costs nothing, may surface the
   first real-user issues. *Low · medium.*

## Competitive snapshot (verified)

- **GoPlasmatic** — provides the Rust MT/MX parsing depth Wireforge's facades
  wrap; **Reframe converts but reports zero field-loss to the caller** (README,
  verified twice) → the truncation gap is real and open. Org has **< 40K crates.io
  downloads, 2–14 GitHub stars** — confirming the Rust-fintech bottleneck is
  *distribution, not capability*. **Orion** MCP runtime is the threat to monitor.
- **ToolOracle / iso20022oracle** — the only MX MCP competitor (v1.0.0, SaaS-hosted,
  no ISO 8583, no truncation detection, no EBCDIC, not open-source).
- **Prowide / jPOS** (Java) dominate breadth; Prowide's truncation-equivalent is
  paywalled behind a commercial product. **moov-io** (Go) covers ISO 8583 + EBCDIC.
- The eight MCP fintech servers in the registry are all crypto/trading/treasury —
  **none touch payment-message parsing.**

## Honest unknowns (do not assert these)

1. **GoPlasmatic Orion MCP timeline** — unknown; highest-consequence uncertainty
   for the MCP moat. Only mitigation: publish first + build `.wf` git-first lock-in.
2. **SWIFT MyStandards redistribution** — IPR policy is ambiguous on embedding
   sample messages in an Apache-2.0 crate. Conservative default: **reference-only,
   do not redistribute SWIFT-origin messages.**
3. **SR2026 address-mandate scope** — pacs.008 + pacs.004 confirmed for 2026-11;
   camt.056 send/receive is 2027, not 2026. Target the checker at pacs.008/pacs.004.
4. **`wf-mx` upstream freeze** — `mx-message` 3.1.4 frozen since Oct 2025 (MX effort
   moved to Reframe). If deprecated, the facade sits on an unmaintained crate;
   owning MX parsing is a later option. Disclose in README.
5. **国密 密评** — no pure-software Rust impl can pass GB/T 39786; certification
   needs a hardware/HSM path via a real enterprise customer. Frame SM2/3/4 as
   *functionally-correct primitives for CIPS integration work outside the
   mandatory-certification perimeter*, not a certified product.
6. **Real samples = zero** as of now. Phase 0 exit gate (≥5 real ISO 8583 hex
   samples) is **unmet**; all CI evidence is synthetic. State this explicitly.
7. **GTM channel ranking** — the r/rust-first sequence rests on one informal
   single-author data point. A reasonable prior, not a validated strategy.

## Appendix — claims dropped by source-verification (transparency)

The fact-checking phase killed 5 of 24 claims. Notable kills: an "OCI registry
supports docker.io/ghcr.io/quay.io/GCP/Azure/MCR" claim **inflated 2 supported
registries to 6**; an "mcpb hosting is GitHub/GitLab-releases-only" restriction
that **does not appear in the source**; a "97% ISO 20022 adoption" figure
**contradicted by its own cited source**; a DFDLSchemas license assertion that was
**materially wrong**; and an iso20022.org "publishes sample instances" claim where
the site provides **schemas only**. These are recorded so the plan is not built on
unverified market lore.
