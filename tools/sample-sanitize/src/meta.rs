//! `meta.toml` schema for sanitized samples and helpers to render/validate it.
//!
//! The sanitizer emits one `<source>-<idx>.meta.toml` next to each
//! `<source>-<idx>.hex` it writes into `samples/iso8583/`. The metadata is
//! the audit trail for redaction: source URL, license, sanitize version,
//! round-trip status, anonymity-set size, and sha256 of the redacted bytes.
//!
//! TOML is hand-rendered to avoid a serde dependency in a tool that already
//! depends on `wf-codec`. Field set is small and stable; a real serde build
//! would add ~30 transitive deps for marginal gain.

use std::fmt::Write as _;

/// One sanitized-sample audit record. Mirrors the schema documented in
/// `<plan>/Sample-First Wave §4.1`, extended on 2026-05-20 to carry the
/// wire `dialect` detected by [`crate::sanitize::sanitize`] so the D7
/// coverage matrix can split samples by dialect.
#[derive(Debug, Clone)]
pub struct SampleMeta {
    pub source: String,
    pub source_url: String,
    pub source_commit: Option<String>,
    pub license: String,
    pub fetched_at: String,
    pub sanitize_version: u32,
    pub sanitized_at: String,
    pub round_trip_verified: bool,
    pub byte_length: usize,
    pub sha256_redacted: String,
    pub fields_redacted: Vec<u8>,
    pub anonymity_set_size: u128,
    /// Detected source dialect (`"HybridAscii"` / `"FullAscii"`). Stored as a
    /// human-readable string to keep the meta file readable without a
    /// dialect-enum dependency.
    pub dialect: String,
    pub notes: String,
}

/// Sanitizer rule version. Bump when redaction logic changes so older meta
/// records remain interpretable.
pub const SANITIZE_VERSION: u32 = 1;

/// Minimum anonymity-set size we consider acceptable without operator review.
/// Below this, the sanitizer still writes the file but flags a warning so the
/// final report can surface low-anonymity samples (see plan §7 risks).
pub const MIN_ANONYMITY_SET_WARN: u128 = 10_000;

impl SampleMeta {
    /// Render to TOML. Strings are escaped with the limited subset we need
    /// (backslash, double-quote) — meta values are tool-controlled, not
    /// user input from untrusted sources, so a full TOML escaper is overkill.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        writeln!(out, "source = {}", quote(&self.source)).ok();
        writeln!(out, "source_url = {}", quote(&self.source_url)).ok();
        if let Some(c) = &self.source_commit {
            writeln!(out, "source_commit = {}", quote(c)).ok();
        }
        writeln!(out, "license = {}", quote(&self.license)).ok();
        writeln!(out, "fetched_at = {}", quote(&self.fetched_at)).ok();
        writeln!(out, "sanitize_version = {}", self.sanitize_version).ok();
        writeln!(out, "sanitized_at = {}", quote(&self.sanitized_at)).ok();
        writeln!(out, "round_trip_verified = {}", self.round_trip_verified).ok();
        writeln!(out, "byte_length = {}", self.byte_length).ok();
        writeln!(out, "sha256_redacted = {}", quote(&self.sha256_redacted)).ok();
        write!(out, "fields_redacted = [").ok();
        for (i, n) in self.fields_redacted.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            write!(out, "{n}").ok();
        }
        writeln!(out, "]").ok();
        writeln!(out, "anonymity_set_size = {}", self.anonymity_set_size).ok();
        writeln!(out, "dialect = {}", quote(&self.dialect)).ok();
        writeln!(out, "notes = {}", quote(&self.notes)).ok();
        out
    }

    /// `true` if the recorded anonymity set is below the warn threshold —
    /// caller surfaces this in stderr and in the daily report.
    pub fn low_anonymity(&self) -> bool {
        self.anonymity_set_size < MIN_ANONYMITY_SET_WARN
    }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fixture() -> SampleMeta {
        SampleMeta {
            source: "jpos-1.10.0".into(),
            source_url: "https://example.invalid/path".into(),
            source_commit: Some("abc123".into()),
            license: "Apache-2.0".into(),
            fetched_at: "2026-05-21T10:00:00Z".into(),
            sanitize_version: SANITIZE_VERSION,
            sanitized_at: "2026-05-21T10:05:00Z".into(),
            round_trip_verified: true,
            byte_length: 187,
            sha256_redacted: "deadbeef".into(),
            fields_redacted: vec![2, 35, 43],
            anonymity_set_size: 99_840,
            dialect: "HybridAscii".into(),
            notes: "exercises field 48 sub-elements".into(),
        }
    }

    #[test]
    fn meta_roundtrips_through_known_keys() {
        let toml = fixture().to_toml();
        for key in [
            "source",
            "source_url",
            "source_commit",
            "license",
            "fetched_at",
            "sanitize_version",
            "sanitized_at",
            "round_trip_verified",
            "byte_length",
            "sha256_redacted",
            "fields_redacted",
            "anonymity_set_size",
            "dialect",
            "notes",
        ] {
            assert!(
                toml.contains(&format!("{key} =")),
                "missing key {key} in:\n{toml}"
            );
        }
    }

    #[test]
    fn quote_escapes_backslash_and_quote() {
        let q = quote("a\"b\\c");
        assert_eq!(q, "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn low_anonymity_threshold_is_inclusive_below() {
        let mut m = fixture();
        m.anonymity_set_size = MIN_ANONYMITY_SET_WARN - 1;
        assert!(m.low_anonymity());
        m.anonymity_set_size = MIN_ANONYMITY_SET_WARN;
        assert!(!m.low_anonymity());
    }
}
