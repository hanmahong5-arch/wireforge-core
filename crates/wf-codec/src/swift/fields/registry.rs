//! Tag → decoder routing.
//!
//! [`decode_field`] is the public entry point: hand it a tag and value
//! (typically pulled straight out of [`super::super::MtField`]) and it
//! dispatches to the correct decoder, falling back to
//! [`FieldSemantic::Raw`] for tags this build does not yet understand.
//!
//! Adding a new tag is a one-line change here once its decoder file is
//! in place — see the module-level docs on [`super`].

use super::{DecodeError, Field20, Field32A, Field50K, FieldSemantic, MtFieldDecoder};

/// Route a raw `(tag, value)` pair to the matching decoder.
///
/// Returns `Ok(FieldSemantic::Raw(value))` for any tag with no
/// registered decoder, so the function never *fails* on an unknown tag
/// (callers can always rely on it returning a comparable typed value).
/// A registered decoder still returns its native error variants on
/// malformed data.
pub fn decode_field(tag: &str, value: &str) -> Result<FieldSemantic, DecodeError> {
    match tag {
        "20" => Field20.decode(value),
        "32A" => Field32A.decode(value),
        "50K" => Field50K.decode(value),
        _ => Ok(FieldSemantic::Raw(value.to_string())),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn routes_to_field_20() {
        let out = decode_field("20", "REF001").unwrap();
        assert!(matches!(out, FieldSemantic::Reference(_)));
    }

    #[test]
    fn routes_to_field_32a() {
        let out = decode_field("32A", "240520USD1000,00").unwrap();
        assert!(matches!(out, FieldSemantic::ValueDateAmount { .. }));
    }

    #[test]
    fn unknown_tag_falls_back_to_raw() {
        let out = decode_field("99Z", "anything goes").unwrap();
        match out {
            FieldSemantic::Raw(s) => assert_eq!(s, "anything goes"),
            other => panic!("expected Raw, got {other:?}"),
        }
    }
}
