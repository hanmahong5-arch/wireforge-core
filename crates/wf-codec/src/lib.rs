//! Financial message codec primitives for Wireforge.
//!
//! Scope:
//! - `iso8583`: ISO 8583 message parse / build (depends on `wf-bitmap`).
//! - `swift`: SWIFT MT series (MT103, MT202, ...) text codec.
//! - `ebcdic`: EBCDIC <-> Unicode single-byte conversion (CP037, CP500) for
//!   mainframe interop. DBCS host code pages (CP935 / CP1388) and any GBK
//!   bridge are out of scope / deferred.

pub mod ebcdic;
pub mod iso8583;
pub mod swift;

pub use ebcdic::{
    decode as ebcdic_decode, encode as ebcdic_encode, CodePage, EbcdicDecoder, EbcdicError,
};
