//! AI message parsing accuracy baseline harness.
//!
//! Scaffold per Plan Sprint 2 §2 — "AI 报文解析准确率基线实验".
//!
//! # Current status (2026-05-19)
//!
//! **SCAFFOLD ONLY.** All meaningful tests are `#[ignore]` and exit early as
//! skipped. This file currently establishes the *structure* of the experiment
//! only; it produces zero accuracy numbers and must not be cited as evidence
//! of any AI-parsing capability.
//!
//! # Blocking prerequisites
//!
//! Before this harness can produce real baseline numbers, ALL of the following
//! must be in place:
//!
//! 1. **≥ 5 real ISO 8583 hex samples** placed under
//!    `wireforge-core/samples/iso8583/*.hex`. See `docs/sample-policy.md` for
//!    sanitization rules. Synthetic / hand-crafted hex is NOT acceptable —
//!    the experiment requires real production-shape messages.
//! 2. **Environment variables** `ANTHROPIC_API_KEY` and `DEEPSEEK_API_KEY`
//!    exported in the shell that runs `cargo test`.
//! 3. **`wf-codec` ISO 8583 parser implementation** complete enough to act as
//!    a ground-truth oracle for per-field accuracy comparison (Sprint 2 §1).
//!
//! # How to enable once unblocked
//!
//! 1. Drop real samples into `samples/iso8583/*.hex` (one hex string per file,
//!    whitespace tolerated).
//! 2. Export the two API keys.
//! 3. Wire concrete `LlmClient` impls (Claude Sonnet 4.6, DeepSeek-V3) and
//!    replace the placeholder bodies of `baseline_claude_sonnet_46` /
//!    `baseline_deepseek_v3`.
//! 4. Remove the `#[ignore]` attributes (or keep them and invoke with
//!    `--ignored`):
//!    ```text
//!    cargo test --test ai_baseline -- --ignored --nocapture
//!    ```
//! 5. Record the per-field accuracy / error modes / cost / latency in
//!    `docs/ai-baseline-2026-Q2.md`. Per CLAUDE.md §4.1 ① and ⑥, every number
//!    written there must be a real measurement, not a placeholder.
//!
//! # Honesty notes
//!
//! - Per CLAUDE.md §4.1, this harness MUST NOT fabricate accuracy numbers
//!   via stubs or simulators when real samples / keys are missing. The
//!   `#[ignore]` + early-skip pattern is the contractual way this file
//!   reports "blocked", and is preferred over an "always passes" test that
//!   would mislead callers into believing a baseline was measured.
//! - `placeholder_runs_compile` exists solely to prove the file compiles in
//!   the workspace; it asserts nothing about AI behavior.

#![allow(clippy::unwrap_used)]
#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// NOTE: we intentionally do NOT `use wf_codec::iso8583::field::FieldDef` here.
// The Architect + Dev swarm is still landing those types; importing them now
// would couple this scaffold to unstable API surface. When the parser is
// ready, the ground-truth comparison helpers will be added below.

// ---------------------------------------------------------------------------
// Local trait + DTO — placeholders, NOT a public API.
// ---------------------------------------------------------------------------

/// Parsed ISO 8583 fields as returned by an LLM parser candidate.
///
/// Intentionally minimal: real schema lands once `wf-codec` exposes a
/// canonical `IsoMessage` type. Per-field comparison logic will then convert
/// `ParsedFields` -> canonical -> diff.
#[derive(Debug, Default)]
pub struct ParsedFields {
    /// Field number (2..=128) -> raw string value as the LLM returned it.
    pub fields: Vec<(u8, String)>,
    /// MTI as the LLM parsed it (e.g. "0200").
    pub mti: Option<String>,
}

/// Minimal contract every LLM parser candidate must satisfy for the baseline.
///
/// Implementations are deferred until API keys + samples are available.
pub trait LlmClient {
    /// Parse one hex-encoded ISO 8583 message.
    ///
    /// Returns the LLM's structured answer, or an `Err` describing the
    /// transport/parse failure (network, rate-limit, malformed JSON).
    fn parse_iso8583_hex(&self, hex: &str) -> Result<ParsedFields, String>;
}

// ---------------------------------------------------------------------------
// Sample loader.
// ---------------------------------------------------------------------------

fn samples_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR points at `crates/wf-codec/`.
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest).join("../../samples/iso8583")
}

/// Load `(filename, raw_bytes)` pairs from `samples/iso8583/*.hex`.
///
/// Returns an empty `Vec` if the directory is missing or empty — callers MUST
/// treat empty as "blocked, skip the test", not as "0 samples passed".
fn load_samples() -> Vec<(String, Vec<u8>)> {
    let dir = samples_dir();
    let Ok(read) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("hex") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let cleaned: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let Ok(bytes) = hex_decode(&cleaned) else {
            continue;
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unknown>")
            .to_string();
        out.push((name, bytes));
    }
    out
}

/// Tiny hex decoder kept local to avoid adding a dep just for the scaffold.
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = nibble(bytes[i])?;
        let lo = nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!("non-hex byte: {b:#x}")),
    }
}

// ---------------------------------------------------------------------------
// Compile-only sanity test (NOT a baseline measurement).
// ---------------------------------------------------------------------------

/// Proves the test file compiles and the helper runs. Asserts nothing about
/// AI behavior, parser correctness, or sample inventory — by design.
#[test]
fn placeholder_runs_compile() {
    let _ = load_samples();
}

// ---------------------------------------------------------------------------
// Baseline tests — ALL `#[ignore]` until the three blockers above clear.
// ---------------------------------------------------------------------------

/// Baseline accuracy for Claude Sonnet 4.6 on the canonical sample set.
///
/// Currently a no-op that prints why it's skipped. Wire a real client + real
/// per-field diff before removing `#[ignore]`.
#[test]
#[ignore = "blocked: needs real samples + ANTHROPIC_API_KEY + wf-codec parser"]
fn baseline_claude_sonnet_46() {
    if env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!(
            "skipped: baseline_claude_sonnet_46 — ANTHROPIC_API_KEY not set; \
             see crates/wf-codec/tests/ai_baseline.rs header for unlock steps"
        );
        return;
    }
    let samples = load_samples();
    if samples.is_empty() {
        eprintln!(
            "skipped: baseline_claude_sonnet_46 — 0 samples in \
             samples/iso8583/; this experiment requires ≥ 5 real hex samples"
        );
        return;
    }
    // Intentionally NO real API call here. Wiring the concrete client is the
    // unlock step; until then, even with env + samples present we refuse to
    // emit fake numbers (CLAUDE.md §4.1 ①).
    eprintln!(
        "skipped: baseline_claude_sonnet_46 — env + {} sample(s) present, \
         but concrete LlmClient impl for Claude Sonnet 4.6 is not yet wired",
        samples.len()
    );
}

/// Baseline accuracy for DeepSeek-V3 on the canonical sample set.
///
/// See `baseline_claude_sonnet_46` for the unlock contract — identical shape.
#[test]
#[ignore = "blocked: needs real samples + DEEPSEEK_API_KEY + wf-codec parser"]
fn baseline_deepseek_v3() {
    if env::var("DEEPSEEK_API_KEY").is_err() {
        eprintln!(
            "skipped: baseline_deepseek_v3 — DEEPSEEK_API_KEY not set; \
             see crates/wf-codec/tests/ai_baseline.rs header for unlock steps"
        );
        return;
    }
    let samples = load_samples();
    if samples.is_empty() {
        eprintln!(
            "skipped: baseline_deepseek_v3 — 0 samples in \
             samples/iso8583/; this experiment requires ≥ 5 real hex samples"
        );
        return;
    }
    eprintln!(
        "skipped: baseline_deepseek_v3 — env + {} sample(s) present, but \
         concrete LlmClient impl for DeepSeek-V3 is not yet wired",
        samples.len()
    );
}
