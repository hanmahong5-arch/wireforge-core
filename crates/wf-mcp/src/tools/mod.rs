//! MCP tool handlers. Each module exposes a pure `*_handler` function
//! that takes deserialized input and returns a `serde_json::Value` or
//! a String error. The MCP routing layer in [`crate`] turns the result
//! into the protocol-level `CallToolResult`.

pub mod address_compliance;
pub mod address_scan;
pub mod build;
pub mod diff;
pub mod ebcdic_decode;
pub mod explain;
pub mod field;
pub mod mt_mx_diff;
pub mod mti;
pub mod parse;
pub mod sm3;
pub mod swift_parse;
pub mod validate;
