# AI Message Parsing Accuracy Baseline — 2026 Q2

Status: **BLOCKED — 0 real samples / 0 API keys configured**
Date: 2026-05-19
Scope: Plan Sprint 2 §2 — 5 real ISO 8583 hex messages × 2 LLM candidates
(Claude Sonnet 4.6, DeepSeek-V3) = 10 parses; record per-field accuracy,
error modes, call cost, latency.

> Per CLAUDE.md §4.1 ① ("perfect numbers = reflexive doubt") and ⑥ ("markers
> present ≠ metric met"), this document intentionally contains **zero
> numbers** until the blocking items below clear. No placeholders, no
> "0%", no "TBD: 99%". The harness scaffold under
> `crates/wf-codec/tests/ai_baseline.rs` likewise emits nothing measurable.

## Blocking items

1. **Samples.** ≥ 5 real ISO 8583 hex messages from a production-shaped
   banking source, sanitized per `docs/sample-policy.md`, placed under
   `samples/iso8583/*.hex`. Sprint 1 parallel task — owner: lead engineer
   external bank-channel acquisition.
2. **API keys.** `ANTHROPIC_API_KEY` and `DEEPSEEK_API_KEY` provisioned in
   the runner environment.
3. **`wf-codec` ISO 8583 parser.** Ground-truth oracle that the LLM outputs
   are diffed against, per field. Tracked under Sprint 2 §1.

## Methodology

Pending — will be populated when blocking items resolve.

## Per-Field Accuracy

Pending — will be populated when blocking items resolve.

## Error Modes

Pending — will be populated when blocking items resolve.

## Cost & Latency

Pending — will be populated when blocking items resolve.
