//! MCP tool handlers. Each module exposes a pure `*_handler` function
//! that takes deserialized input and returns a `serde_json::Value` or
//! a String error. The MCP routing layer in [`crate`] turns the result
//! into the protocol-level `CallToolResult`.

pub mod build;
pub mod diff;
pub mod explain;
pub mod field;
pub mod mti;
pub mod parse;
pub mod swift_parse;
pub mod validate;
