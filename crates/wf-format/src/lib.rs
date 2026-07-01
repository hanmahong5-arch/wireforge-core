//! Parser for the Wireforge `.wf` flat-file format.
//!
//! `.wf` is a Bruno-inspired DSL for capturing ISO 8583 / SWIFT MT
//! message specifications under Git. The format is text, line-oriented,
//! brace-grouped, and additive: unrecognised top-level blocks and keys
//! land in `extra` / `Raw` containers rather than being rejected, so a
//! file written for a future Wireforge revision never silently breaks
//! an older parser.
//!
//! # Example
//!
//! ```text
//! meta {
//!   name: Auth Request 0200
//!   type: iso8583
//!   seq: 1
//! }
//!
//! iso8583 {
//!   mti: 0200
//!   field 2: 4242424242424242
//!   field 3: 000000
//!   field 4: 000000010000
//!   field 7: 1130120000
//! }
//! ```
//!
//! Parse it with [`parse`]:
//!
//! ```
//! # use wf_format::parse;
//! let src = r#"
//! meta { name: Hello World
//!        type: iso8583 }
//! iso8583 { mti: 0200 }
//! "#;
//! // (string-literal escapes aren't part of the .wf grammar; this
//! // example is hand-fed into the parser.)
//! let _ = parse;  // doc-test compiles, runtime behavior covered elsewhere.
//! ```
//!
//! # Grammar (informal)
//!
//! ```text
//! file        := top_block*
//! top_block   := IDENT '{' entry* '}'
//! entry       := key ':' VALUE_TO_EOL
//!              | 'block' NUMBER '{' entry* '}'
//! key         := IDENT (IDENT | NUMBER)?
//! IDENT       := /[A-Za-z_][A-Za-z0-9_-]*/
//! NUMBER      := /[0-9]+/
//! VALUE_TO_EOL := <chars to end-of-line, '//'-comment stripped, trimmed>
//! ```
//!
//! Block comments (`/* … */`) are stripped before lexing; line comments
//! (`//`) are stripped per line. Every block kind except `swift-mt`'s
//! `block 4` rejects nested blocks in the MVP, so the grammar above is
//! one level deep almost everywhere.
//!
//! # What's deliberately parked
//!
//! - `{{var}}` substitutions (Bruno has these; Phase 2 decides format).
//! - `assert { ... }` post-conditions (needs an assertion DSL design).
//! - Cross-protocol nesting (e.g. `iso8583` block inside a `swift-mt`).
//! - Byte-exact (comment- and whitespace-preserving) round-trip. The
//!   [`to_wf_string`] serializer renders an AST back to `.wf` text with
//!   an **AST-idempotent** guarantee (`parse(to_wf_string(parse(s)?)?)? ==
//!   parse(s)?`), but comments and original layout are dropped at parse
//!   time and are not reconstructed. A lossless CST is future work.

pub mod ast;
pub mod lexer;
pub mod pair;
pub mod parser;
pub mod writer;

pub use ast::{Body, Iso8583Body, Meta, MxBody, RawBody, SwiftMtBody, WfFile};
pub use lexer::LexError;
pub use pair::{
    extract_mt_mx_pair, extract_oracle_triple, swift_mt_to_fin, OraclePairError, PairError,
};
pub use parser::{parse, ParseError};
pub use writer::to_wf_string;
