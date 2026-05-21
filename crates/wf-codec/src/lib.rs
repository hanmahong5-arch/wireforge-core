//! Financial message codec primitives for Wireforge.
//!
//! Scope:
//! - `iso8583`: ISO 8583 message parse / build (depends on `wf-bitmap`).
//! - `swift`: SWIFT MT series (MT103, MT202, ...) text codec.
//! - `ebcdic`: EBCDIC <-> GBK / UTF-8 conversion for mainframe interop.

pub mod ebcdic;
pub mod iso8583;
pub mod swift;
