# External-Parser Accuracy Baseline — 2026 Q2

Status: **BLOCKED — 0 real samples / 0 endpoint credentials configured**
Date: 2026-05-19 (harness renamed from `ai_baseline` 2026-05-30)
Scope: 5 real ISO 8583 hex messages × 2 external parser candidates
(`model-a`, `model-b`) = 10 parses; record per-field accuracy, error modes,
call cost, latency. Scored against `wf-codec`'s deterministic parser as the
ground-truth oracle.

> Per the project honesty rules ("perfect numbers = reflexive doubt"; "markers
> present ≠ metric met"), this document intentionally contains **zero
> numbers** until the blocking items below clear. No placeholders, no
> "0%", no "TBD: 99%". The harness scaffold under
> `crates/wf-codec/tests/parse_accuracy.rs` likewise emits nothing measurable.
>
> Candidate endpoints are referred to only as `model-a` / `model-b`; concrete
> vendor / model names are operator configuration, never written into source
> (project naming convention).

## Blocking items

1. **Samples.** ≥ 5 real ISO 8583 hex messages from a production-shaped
   banking source, sanitized per `docs/sample-policy.md`, placed under
   `samples/iso8583/*.hex`. Owner: external bank-channel acquisition.
2. **Endpoint credentials.** `WF_MODEL_A_API_KEY` and `WF_MODEL_B_API_KEY`
   provisioned in the runner environment.
3. **`wf-codec` ISO 8583 parser.** Ground-truth oracle that the candidate
   outputs are diffed against, per field. Available today.

## Methodology

Pending — will be populated when blocking items resolve.

## Per-Field Accuracy

Pending — will be populated when blocking items resolve.

## Error Modes

Pending — will be populated when blocking items resolve.

## Cost & Latency

Pending — will be populated when blocking items resolve.
