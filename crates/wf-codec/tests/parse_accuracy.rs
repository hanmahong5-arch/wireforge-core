//! External-parser accuracy baseline harness.
//!
//! Measures how accurately an *external* message parser (e.g. an LLM-backed
//! service reached over HTTP) extracts ISO 8583 fields, scored against this
//! crate's deterministic codec as the ground-truth oracle. Two parser
//! candidates are compared — referred to here only as `model-a` and
//! `model-b`; the concrete endpoints are operator configuration, never
//! hard-coded (per the project's no-vendor-names-in-source policy).
//!
//! # Current status
//!
//! **SCAFFOLD ONLY.** Every measuring test is `#[ignore]` and exits early as
//! skipped. This file establishes the *structure* of the experiment; it
//! produces zero accuracy numbers and must not be cited as evidence of any
//! parsing capability.
//!
//! # Blocking prerequisites
//!
//! Before this harness can produce real baseline numbers, ALL of the
//! following must be in place:
//!
//! 1. **≥ 5 real ISO 8583 hex samples** placed under
//!    `wireforge-core/samples/iso8583/*.hex`. See `docs/sample-policy.md` for
//!    sanitization rules. Synthetic / hand-crafted hex is NOT acceptable —
//!    the experiment requires real production-shape messages.
//! 2. **Endpoint credentials** for the two parser candidates, supplied via
//!    the environment variables `WF_MODEL_A_API_KEY` and `WF_MODEL_B_API_KEY`
//!    (the harness reads only these neutral names; map them to whatever
//!    service you are evaluating).
//! 3. **`wf-codec` ISO 8583 parser** complete enough to act as a ground-truth
//!    oracle for per-field accuracy comparison (it is, today).
//!
//! # How to enable once unblocked
//!
//! 1. Drop real samples into `samples/iso8583/*.hex` (one hex string per
//!    file, whitespace tolerated).
//! 2. Export the two credential variables.
//! 3. Wire concrete [`Parser`] impls for each candidate and replace the
//!    placeholder bodies of `baseline_model_a` / `baseline_model_b`.
//! 4. Remove the `#[ignore]` attributes (or keep them and invoke with
//!    `--ignored`):
//!    ```text
//!    cargo test --test parse_accuracy -- --ignored --nocapture
//!    ```
//! 5. Record the per-field accuracy / error modes / cost / latency in
//!    `docs/parse-accuracy-baseline-2026-Q2.md`. Per the project's honesty
//!    policy, every number written there must be a real measurement, not a
//!    placeholder.
//!
//! # Honesty notes
//!
//! - Per the project's honesty policy, this harness MUST NOT fabricate
//!   accuracy numbers via stubs or simulators when real samples / credentials
//!   are missing. The
//!   `#[ignore]` + early-skip pattern is the contractual way this file
//!   reports "blocked", and is preferred over an "always passes" test that
//!   would mislead callers into believing a baseline was measured.
//! - Scoring uses `wf-codec`'s own parser as the oracle: the candidate under
//!   test and the oracle are independent code paths (an external service vs.
//!   this crate), so the comparison is a measurement, not a tautology
//!   (per the project's test-independence policy).
//! - `placeholder_runs_compile` exists solely to prove the file compiles in
//!   the workspace; it asserts nothing about parser behavior.

#![allow(clippy::unwrap_used)]
#![allow(dead_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

// NOTE: when the per-field diff is implemented, import wf_codec's canonical
// message type here and compare candidate output against `parse(bytes)`.

// ---------------------------------------------------------------------------
// Candidates under test — neutral identifiers, NOT product names.
// ---------------------------------------------------------------------------

/// The two external parser candidates this baseline compares. Kept abstract
/// on purpose: the mapping from these labels to real endpoints lives in the
/// operator's environment, not in version control.
const MODELS_UNDER_TEST: [&str; 2] = ["model-a", "model-b"];

// ---------------------------------------------------------------------------
// Local trait + DTO — placeholders, NOT a public API.
// ---------------------------------------------------------------------------

/// Parsed ISO 8583 fields as returned by an external parser candidate.
///
/// Intentionally minimal: the per-field comparison will convert this into
/// `wf-codec`'s canonical message and diff against the oracle.
#[derive(Debug, Default)]
pub struct ParsedFields {
    /// Field number (2..=128) -> raw string value as the candidate returned it.
    pub fields: Vec<(u8, String)>,
    /// MTI as the candidate parsed it (e.g. "0200").
    pub mti: Option<String>,
}

/// Minimal contract every external parser candidate must satisfy for the
/// baseline. Implementations are deferred until credentials + samples are
/// available.
pub trait Parser {
    /// Parse one hex-encoded ISO 8583 message.
    ///
    /// Returns the candidate's structured answer, or an `Err` describing the
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

/// Shared skip logic for a candidate baseline: returns `Some(reason)` when a
/// blocking prerequisite is missing, or `None` when env + samples are present
/// (at which point the concrete client still needs wiring before any number
/// is emitted). Centralised so both candidates report identically.
fn skip_reason(label: &str, key_env: &str) -> Option<String> {
    if env::var(key_env).is_err() {
        return Some(format!(
            "{label}: {key_env} not set; see crates/wf-codec/tests/parse_accuracy.rs \
             header for unlock steps"
        ));
    }
    let samples = load_samples();
    if samples.is_empty() {
        return Some(format!(
            "{label}: 0 samples in samples/iso8583/; this experiment requires \
             ≥ 5 real hex samples"
        ));
    }
    // Env + samples present, but emitting a number still requires a concrete
    // Parser impl. Refuse to fabricate one (per the project's honesty policy).
    Some(format!(
        "{label}: env + {} sample(s) present, but the concrete Parser impl is \
         not yet wired",
        samples.len()
    ))
}

// ---------------------------------------------------------------------------
// Compile-only sanity test (NOT a baseline measurement).
// ---------------------------------------------------------------------------

/// Proves the test file compiles and the helpers run. Asserts nothing about
/// parser behavior, codec correctness, or sample inventory — by design.
#[test]
fn placeholder_runs_compile() {
    let _ = load_samples();
    assert_eq!(MODELS_UNDER_TEST.len(), 2);
}

// ---------------------------------------------------------------------------
// Baseline tests — ALL `#[ignore]` until the three blockers above clear.
// ---------------------------------------------------------------------------

/// Baseline accuracy for candidate `model-a` on the canonical sample set.
///
/// Currently a no-op that prints why it's skipped. Wire a real [`Parser`] +
/// real per-field diff before removing `#[ignore]`.
#[test]
#[ignore = "blocked: needs real samples + WF_MODEL_A_API_KEY + concrete Parser impl"]
fn baseline_model_a() {
    if let Some(reason) = skip_reason("baseline_model_a", "WF_MODEL_A_API_KEY") {
        eprintln!("skipped: {reason}");
    }
}

/// Baseline accuracy for candidate `model-b` on the canonical sample set.
///
/// See `baseline_model_a` for the unlock contract — identical shape.
#[test]
#[ignore = "blocked: needs real samples + WF_MODEL_B_API_KEY + concrete Parser impl"]
fn baseline_model_b() {
    if let Some(reason) = skip_reason("baseline_model_b", "WF_MODEL_B_API_KEY") {
        eprintln!("skipped: {reason}");
    }
}
