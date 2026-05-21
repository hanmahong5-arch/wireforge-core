//! ISO 8583 message codec: dialect-aware parser + builder driven by the
//! field type table and `wf-bitmap`. See [`Dialect`] for the supported
//! wire flavours.

pub mod bcd;
pub mod builder;
pub mod dialect;
pub mod field;
pub mod parser;

pub use builder::{build, build_with, BuildError};
pub use dialect::Dialect;
pub use parser::{parse, parse_any, parse_with, Iso8583Message, ParseError};
