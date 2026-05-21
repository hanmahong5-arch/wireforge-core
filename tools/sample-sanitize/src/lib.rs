//! Library half of the `sample-sanitize` tool.
//!
//! Splits the testable sanitize/redact/round-trip logic out of `main.rs`,
//! which is just CLI glue + filesystem I/O. The bin re-exports nothing from
//! here directly — it consumes [`sanitize::sanitize`] and [`meta::SampleMeta`].
//!
//! See `docs/sample-policy.md` for the redaction rule table this implements
//! and the project-wide sample-handling policy.

pub mod meta;
pub mod sanitize;
