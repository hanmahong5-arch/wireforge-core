//! ISO 8583 message codec: dialect-aware parser + builder driven by the
//! field type table and `wf-bitmap`. See [`Dialect`] for the supported
//! wire flavours.

pub mod bcd;
pub mod builder;
pub mod dialect;
pub mod field;
pub mod parser;
pub mod spec;

pub use builder::{build, build_with, build_with_spec, BuildError};
pub use dialect::Dialect;
pub use parser::{parse, parse_any, parse_with, parse_with_spec, Iso8583Message, ParseError};
#[cfg(feature = "spec-load")]
pub use spec::SpecLoadError;
pub use spec::{FieldMeta, FieldSpec, SpecError, SpecField};
