//! Wireforge SWIFT MT facade — "parse as strongly as possible, never
//! fail to no structure".
//!
//! This crate lifts the codec-layer philosophy of the structural SWIFT
//! parser (be lossless, never reject a well-framed message) up to the
//! whole-message level by combining two parsers:
//!
//! 1. A vetted third-party typed parser that produces fully-typed bodies
//!    for the ~30 MT types it supports (MT103, MT202, MT940, …).
//! 2. The Wireforge structural codec in [`wf_codec::swift`], which is
//!    MT-type-agnostic and only tokenises blocks/fields — so it accepts
//!    any well-framed `{1:…}{2:…}{4:…-}` message regardless of type.
//!
//! [`parse`] tries the typed parser first; if the message is an
//! unsupported MT type, or the typed parse fails for any reason, it
//! falls back to the structural parser. Only when *both* fail does it
//! return [`WfMtError`]. The returned [`WfMt`] tells the caller which
//! path won, so semantic richness is available when possible and raw
//! structure is always available otherwise.
//!
//! ## Coupling note (deliberate)
//!
//! [`WfMt::Typed`] exposes the third-party [`ParsedSwiftMessage`] type
//! directly. Re-wrapping its ~30 richly-typed message bodies behind a
//! Wireforge-owned mirror enum would duplicate the entire typed surface
//! for no behavioural gain — the *whole point* of the Typed path is to
//! hand callers that rich body. We therefore re-export it from this
//! crate ([`ParsedSwiftMessage`]) and accept the coupling consciously.
//! Everything else (the wrapper enum, the error type, the path tag) is
//! Wireforge-owned, so callers that only need "did it parse, and what's
//! the structure" never touch the third-party types.

use std::fmt;

// Re-exported on purpose: the typed body is the value of the Typed path.
// See the module-level "Coupling note".
pub use swift_mt_message::ParsedSwiftMessage;

// The structural fallback's owned types. Re-exported so callers can match
// on `WfMt::Structural(_)` without adding a direct `wf-codec` dependency.
pub use wf_codec::swift::{Block, MtField, MtMessage, MtSubBlock};

/// A parsed SWIFT MT message, tagged by how strongly it was parsed.
///
/// The variant *is* the "which path won" signal the caller needs: there
/// is no separate flag to keep in sync.
#[derive(Debug, Clone)]
pub enum WfMt {
    /// The message was a supported MT type and parsed into a fully-typed
    /// body. Carries the third-party [`ParsedSwiftMessage`] (see the
    /// module-level coupling note).
    Typed(Box<ParsedSwiftMessage>),
    /// The message was well-framed but its MT type is not supported by
    /// the typed parser (or the typed parse failed); it was parsed
    /// losslessly into structural blocks instead.
    Structural(MtMessage),
}

impl WfMt {
    /// `true` if the typed parser produced a fully-typed body.
    pub fn is_typed(&self) -> bool {
        matches!(self, WfMt::Typed(_))
    }

    /// `true` if the message fell back to the structural parser.
    pub fn is_structural(&self) -> bool {
        matches!(self, WfMt::Structural(_))
    }

    /// The fully-typed body, if the typed path won.
    pub fn as_typed(&self) -> Option<&ParsedSwiftMessage> {
        match self {
            WfMt::Typed(m) => Some(m),
            WfMt::Structural(_) => None,
        }
    }

    /// The structural blocks, if the fallback path won.
    pub fn as_structural(&self) -> Option<&MtMessage> {
        match self {
            WfMt::Structural(m) => Some(m),
            WfMt::Typed(_) => None,
        }
    }
}

/// Error returned only when *both* the typed parser and the structural
/// fallback reject the input.
///
/// Carries human-readable summaries of each underlying failure rather
/// than the third-party / codec error types themselves, so callers stay
/// decoupled from both. The [`fmt::Display`] impl states the three
/// things a caller needs: what failed, what was expected, and what the
/// caller can do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfMtError {
    /// Summary of the typed parser's failure.
    typed_error: String,
    /// Summary of the structural parser's failure.
    structural_error: String,
}

impl WfMtError {
    /// Summary of why the typed parse failed.
    pub fn typed_error(&self) -> &str {
        &self.typed_error
    }

    /// Summary of why the structural fallback failed.
    pub fn structural_error(&self) -> &str {
        &self.structural_error
    }
}

impl fmt::Display for WfMtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Three-element error: (1) what failed, (2) what was expected,
        // (3) what the caller can do.
        write!(
            f,
            "could not parse SWIFT MT message by any path \
             (typed parser: {}; structural parser: {}); \
             expected a well-framed SWIFT MT message with balanced \
             `{{1:…}}…{{4:…-}}` blocks; \
             check the input is a complete MT message (not a fragment, \
             MX/ISO-20022 XML, or non-SWIFT payload) before retrying",
            self.typed_error, self.structural_error
        )
    }
}

impl std::error::Error for WfMtError {}

/// Parse a raw SWIFT MT message as strongly as possible.
///
/// Strategy:
/// 1. Attempt the typed parse (auto-detecting the MT type). On success,
///    return [`WfMt::Typed`].
/// 2. On *any* typed-parse error — unsupported MT type or otherwise —
///    attempt the structural parse. On success, return
///    [`WfMt::Structural`].
/// 3. If the structural parse also fails, return [`WfMtError`] carrying
///    summaries of both failures.
///
/// This never panics on caller input: every failure mode is a `Result`.
pub fn parse(raw: &str) -> Result<WfMt, WfMtError> {
    match swift_mt_message::SwiftParser::parse_auto(raw) {
        Ok(typed) => Ok(WfMt::Typed(Box::new(typed))),
        Err(typed_err) => match wf_codec::swift::parse(raw) {
            Ok(structural) => Ok(WfMt::Structural(structural)),
            Err(structural_err) => Err(WfMtError {
                typed_error: typed_err.to_string(),
                structural_error: structural_err.to_string(),
            }),
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Public, documented MT103 example.
    ///
    /// Source: the `swift-mt-message` crate's own published crate-level
    /// doc example (`src/lib.rs`, the "Quick Start" doctest), which is a
    /// standard MT103 with blocks {1:}{2:}{4: …}. Reproduced here as the
    /// external anchor for the typed path. CRLF line endings as in the
    /// upstream example.
    const MT103_EXAMPLE: &str = "{1:F01BANKDEFFAXXX0000000000}{2:I103BANKDEFFAXXXU3003}{4:\r\n:20:REF123\r\n:23B:CRED\r\n:32A:240719USD1234,56\r\n:50K:/12345678\r\nJOHN DOE\r\n:59:/98765432\r\nJANE SMITH\r\n:71A:OUR\r\n-}";

    #[test]
    fn mt103_takes_typed_path_and_extracts_semantics() {
        let parsed = parse(MT103_EXAMPLE).expect("MT103 example must parse");
        assert!(parsed.is_typed(), "MT103 must take the Typed path");

        let typed = parsed.as_typed().expect("typed body present");
        // The auto-detector must have classified this as an MT103.
        let mt103 = match typed {
            ParsedSwiftMessage::MT103(m) => m,
            other => panic!("expected MT103 variant, got {other:?}"),
        };
        assert_eq!(mt103.message_type, "103");

        // Semantic-field anchor: re-serialise the *typed* body and assert
        // that the parsed-then-rebuilt message still carries the field-20
        // reference and the 32A date/currency/amount. Round-tripping
        // through the typed model (not raw string slicing) proves the
        // typed parser actually decoded these fields, not merely copied
        // bytes. This is functional self-consistency of the typed model.
        let rebuilt = mt103.to_mt_message();
        assert!(
            rebuilt.contains("REF123"),
            "field 20 reference must survive typed round-trip; got: {rebuilt}"
        );
        assert!(
            rebuilt.contains("240719") && rebuilt.contains("USD") && rebuilt.contains("1234,56"),
            "field 32A date/currency/amount must survive typed round-trip; got: {rebuilt}"
        );
    }

    #[test]
    fn unsupported_mt_type_falls_back_to_structural_with_blocks() {
        // MT799 (free-format) is well-framed but NOT one of the ~30
        // typed-supported types, so the typed parser rejects it with
        // UnsupportedMessageType and the facade must fall back.
        let mt799 = "{1:F01BANKDEFFAXXX0000000000}{2:I799BANKDEFFAXXXN}{4:\r\n:20:FREEFORM01\r\n:79:HELLO STRUCTURAL WORLD\r\n-}";
        let parsed = parse(mt799).expect("well-framed MT799 must parse structurally");
        assert!(
            parsed.is_structural(),
            "unsupported MT type must take the Structural path"
        );

        let structural = parsed.as_structural().expect("structural body present");
        // Structure must actually be present, not an empty shell.
        assert!(
            structural.blocks.contains_key(&1),
            "block 1 must be present"
        );
        assert!(
            structural.blocks.contains_key(&4),
            "block 4 must be present"
        );
        let f20 = structural
            .field("20")
            .expect("field 20 present in structural block 4");
        assert_eq!(f20.value, "FREEFORM01");
        let f79 = structural
            .field("79")
            .expect("field 79 present in structural block 4");
        assert_eq!(f79.value, "HELLO STRUCTURAL WORLD");
    }

    #[test]
    fn garbage_input_returns_error_without_panicking() {
        // Neither a SWIFT MT frame nor anything the structural parser
        // accepts: no `{` blocks at all.
        let garbage = "this is definitely not a swift message at all";
        let err = parse(garbage).expect_err("garbage must not parse by any path");
        // Three-element message must mention what the caller can do.
        let msg = err.to_string();
        assert!(
            msg.contains("typed parser") && msg.contains("structural parser"),
            "error must summarise both failures; got: {msg}"
        );
        // Both underlying summaries are populated.
        assert!(!err.typed_error().is_empty());
        assert!(!err.structural_error().is_empty());
    }

    #[test]
    fn structural_path_is_lossless_round_trip() {
        // Functional self-consistency: a structurally-parsed message,
        // rebuilt and re-parsed, yields the same blocks. This is NOT a
        // standards-conformance measurement — it only proves the
        // facade's fallback path is internally consistent.
        let mt799 = "{1:F01BANKDEFFAXXX0000000000}{2:I799BANKDEFFAXXXN}{4:\r\n:20:FREEFORM01\r\n:79:HELLO STRUCTURAL WORLD\r\n-}";
        let first = parse(mt799).expect("first parse");
        let structural = first.as_structural().expect("structural").clone();
        let wire = wf_codec::swift::build(&structural).expect("build wire");
        let second = wf_codec::swift::parse(&wire).expect("re-parse wire");
        assert_eq!(
            structural, second,
            "structural round-trip must be lossless (functional self-consistency)"
        );
    }
}
