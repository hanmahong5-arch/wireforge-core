//! Serializer for the `.wf` flat-file format — the inverse of
//! [`crate::parse`].
//!
//! # What "round-trip" means here
//!
//! The serializer targets **AST idempotency**, not byte-level fidelity:
//!
//! ```text
//! parse(to_wf_string(parse(src)?)?)? == parse(src)?
//! ```
//!
//! Comments (`//`, `/* */`) and original whitespace / key ordering are
//! discarded by the parser before an AST ever exists (see
//! [`crate::lexer`]), so they cannot be reconstructed from a [`WfFile`].
//! Re-emitting and re-parsing therefore yields an equal AST, but not
//! necessarily the original source text. A byte-exact round-trip would
//! require a lossless concrete syntax tree (CST); that is deliberately
//! out of scope for this writer.
//!
//! # Why AST values are safe to emit verbatim
//!
//! Every string a [`WfFile`] can hold has already passed through
//! [`crate::lexer::Lexer::read_value_until_newline`], which:
//!
//! - truncates at the first `//` (so AST values never contain a line
//!   comment),
//! - breaks at a newline (so AST values are single-line), and
//! - breaks at a `}` seen at brace depth 0 (so every `}` inside an AST
//!   value is balanced by a preceding `{`).
//!
//! Re-emitting such a value on a `key: value` line and re-parsing it
//! reproduces the identical value. Composite map keys are likewise only
//! ever `"name"` or `"name arg"` (a single identifier plus an optional
//! identifier / number), so emitting the key verbatim re-parses to the
//! same key. These invariants are what make the idempotency guarantee
//! hold without escaping or quoting.

use crate::ast::{Body, Iso8583Body, Meta, MxBody, RawBody, SwiftMtBody, WfFile};
use core::fmt;

/// Indentation for entries one level inside a block.
const INDENT: &str = "  ";
/// Indentation for entries two levels deep (e.g. inside `block 4`).
const INDENT2: &str = "    ";

/// Render a [`WfFile`] back into `.wf` source text.
///
/// The output re-parses to an AST equal to the input (see the module
/// docs for the exact idempotency contract). Top-level layout is fixed:
/// the `meta` block first, then each payload block in order, every block
/// separated by a blank line. Within a block, typed fields come first in
/// a stable order, followed by `extra` entries in sorted (`BTreeMap`)
/// order.
pub fn to_wf_string(file: &WfFile) -> String {
    let mut out = String::new();
    write_meta(&mut out, &file.meta);
    for body in &file.bodies {
        out.push('\n');
        write_body(&mut out, body);
    }
    out
}

impl fmt::Display for WfFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&to_wf_string(self))
    }
}

fn write_meta(out: &mut String, meta: &Meta) {
    out.push_str("meta {\n");
    if let Some(name) = &meta.name {
        write_kv(out, INDENT, "name", name);
    }
    if let Some(type_) = &meta.type_ {
        write_kv(out, INDENT, "type", type_);
    }
    if let Some(seq) = &meta.seq {
        write_kv(out, INDENT, "seq", seq);
    }
    for (key, value) in &meta.extra {
        write_kv(out, INDENT, key, value);
    }
    out.push_str("}\n");
}

fn write_body(out: &mut String, body: &Body) {
    match body {
        Body::Iso8583(iso) => write_iso8583(out, iso),
        Body::SwiftMt(mt) => write_swift_mt(out, mt),
        Body::Mx(mx) => write_mx(out, mx),
        Body::Raw(raw) => write_raw(out, raw),
    }
}

fn write_mx(out: &mut String, mx: &MxBody) {
    out.push_str("mx {\n");
    write_kv(out, INDENT, "xml", &mx.xml);
    out.push_str("}\n");
}

fn write_iso8583(out: &mut String, iso: &Iso8583Body) {
    out.push_str("iso8583 {\n");
    if let Some(mti) = &iso.mti {
        write_kv(out, INDENT, "mti", mti);
    }
    for (num, value) in &iso.fields {
        let key = format!("field {num}");
        write_kv(out, INDENT, &key, value);
    }
    for (key, value) in &iso.extra {
        write_kv(out, INDENT, key, value);
    }
    out.push_str("}\n");
}

fn write_swift_mt(out: &mut String, mt: &SwiftMtBody) {
    out.push_str("swift-mt {\n");
    for (id, value) in &mt.blocks {
        let key = format!("block {id}");
        write_kv(out, INDENT, &key, value);
    }
    if let Some(block_4) = &mt.block_4 {
        out.push_str(INDENT);
        out.push_str("block 4 {\n");
        for (tag, value) in block_4 {
            write_kv(out, INDENT2, tag, value);
        }
        out.push_str(INDENT);
        out.push_str("}\n");
    }
    for (key, value) in &mt.extra {
        write_kv(out, INDENT, key, value);
    }
    out.push_str("}\n");
}

fn write_raw(out: &mut String, raw: &RawBody) {
    out.push_str(&raw.name);
    out.push_str(" {\n");
    for (key, value) in &raw.entries {
        write_kv(out, INDENT, key, value);
    }
    out.push_str("}\n");
}

/// Emit one `key: value` line. An empty value is written as `key:` with
/// no trailing space so the output stays clean; the value-mode reader
/// skips leading whitespace and reads an empty rest-of-line either way,
/// so both forms re-parse to the empty string.
fn write_kv(out: &mut String, indent: &str, key: &str, value: &str) {
    out.push_str(indent);
    out.push_str(key);
    if value.is_empty() {
        out.push_str(":\n");
    } else {
        out.push_str(": ");
        out.push_str(value);
        out.push('\n');
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::parser::parse;

    /// Core idempotency check: parsing the source, serializing it, and
    /// re-parsing the output yields an AST equal to the first parse.
    /// Also asserts a second serialization is byte-identical to the
    /// first (the writer is a deterministic function of the AST).
    fn assert_round_trip(src: &str) {
        let first = parse(src).expect("source should parse");
        let rendered = to_wf_string(&first);
        let second = parse(&rendered).unwrap_or_else(|e| {
            panic!("serialized output should re-parse, but failed: {e}\n--- output ---\n{rendered}")
        });
        assert_eq!(
            first, second,
            "AST changed across serialize/re-parse\n--- output ---\n{rendered}"
        );
        // Serializing the re-parsed AST must reproduce the same text:
        // the writer is a pure function of the AST, so a fixed AST has a
        // fixed rendering.
        let rendered_again = to_wf_string(&second);
        assert_eq!(
            rendered, rendered_again,
            "serialization is not stable across a second pass"
        );
    }

    #[test]
    fn meta_only() {
        assert_round_trip("meta { name: Hello\n type: iso8583\n seq: 1 }");
    }

    #[test]
    fn meta_with_extra_keys() {
        assert_round_trip(
            "meta {\n name: X\n type: iso8583\n author: anita\n note here: free text\n}",
        );
    }

    #[test]
    fn meta_empty_block() {
        // A meta block with no entries is legal (template skeleton).
        assert_round_trip("meta {\n}");
    }

    #[test]
    fn iso8583_full() {
        assert_round_trip(
            "meta { name: Auth 0200\n type: iso8583\n }\n\
             iso8583 {\n\
               mti: 0200\n\
               field 2: 4242424242424242\n\
               field 3: 000000\n\
               field 4: 000000010000\n\
               field 7: 1130120000\n\
               field 127: deadbeef\n\
             }",
        );
    }

    #[test]
    fn iso8583_with_extra_and_empty_value() {
        assert_round_trip(
            "meta { name: Edge\n type: iso8583 }\n\
             iso8583 {\n\
               mti: 0800\n\
               field 11:\n\
               note: a value with spaces\n\
               custom tag: paired key\n\
             }",
        );
    }

    #[test]
    fn swift_mt_with_block_4() {
        assert_round_trip(
            "meta { name: MT103\n type: swift-mt }\n\
             swift-mt {\n\
               block 1: F01BANKBEBBAXXX0000000000\n\
               block 2: I103BANKDEFFXXXXN\n\
               block 4 {\n\
                 field 20: REF12345\n\
                 field 32A: 240520USD12345,67\n\
                 field 50K: /12345\n\
               }\n\
               block 5: CHK123456789ABC\n\
             }",
        );
    }

    #[test]
    fn swift_mt_blocks_only_no_block_4() {
        assert_round_trip(
            "meta { name: NoBlock4\n type: swift-mt }\n\
             swift-mt {\n\
               block 1: F01TESTXXX\n\
               block 3: {108:REF}{121:UUID}\n\
             }",
        );
    }

    #[test]
    fn swift_mt_empty_block_4() {
        assert_round_trip(
            "meta { name: EmptyB4\n type: swift-mt }\n\
             swift-mt {\n\
               block 4 {\n\
               }\n\
             }",
        );
    }

    #[test]
    fn mx_block_round_trips() {
        // An `mx` block with an opaque single-line XML envelope value.
        // The XML uses `<`/`>` only (no braces), so it survives the
        // value reader intact.
        assert_round_trip(
            "meta { name: MX\n type: mx }\n\
             mx {\n\
               xml: <Envelope><AppHdr></AppHdr><Document></Document></Envelope>\n\
             }",
        );
    }

    #[test]
    fn mx_xml_with_namespace_uri_is_not_truncated_at_double_slash() {
        // An ISO 20022 envelope's `xml:` value legitimately contains `//`
        // inside an `http://...` namespace URI. The opaque value reader
        // must NOT treat that `//` as a line comment, so the whole
        // envelope — including the closing `</Document>` after the URI —
        // survives parse and round-trips.
        let src = "meta { name: NS\n type: mx }\n\
             mx {\n\
               xml: <Document xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><Dbtr><Nm>ACME</Nm></Dbtr></Document>\n\
             }";
        let parsed = parse(src).expect("namespaced MX must parse");
        let mx = parsed
            .bodies
            .iter()
            .find_map(|b| match b {
                Body::Mx(m) => Some(m),
                _ => None,
            })
            .expect("mx body present");
        assert!(
            mx.xml.contains("http://www.w3.org/2001/XMLSchema-instance"),
            "namespace URI (with `//`) must survive verbatim, got: {}",
            mx.xml
        );
        assert!(
            mx.xml.contains("</Document>"),
            "content after the `//` URI must not be truncated, got: {}",
            mx.xml
        );
        // The writer's idempotency contract must hold for this value.
        assert_round_trip(src);
    }

    #[test]
    fn multi_body_pair_round_trips() {
        // A matched swift-mt + mx pair in one file must round-trip with
        // both bodies preserved in order.
        let src = "meta { name: Pair\n type: pair }\n\
             swift-mt {\n\
               block 1: F01BANKBEBBAXXX0000000000\n\
               block 2: I103BANKDEFFXXXXN\n\
               block 4 {\n\
                 field 20: REF12345\n\
                 field 32A: 240520USD12345,67\n\
               }\n\
             }\n\
             mx {\n\
               xml: <Envelope><AppHdr></AppHdr><Document></Document></Envelope>\n\
             }";
        let parsed = parse(src).expect("pair source should parse");
        assert_eq!(parsed.bodies.len(), 2, "both bodies must be present");
        assert!(matches!(parsed.bodies[0], Body::SwiftMt(_)));
        assert!(matches!(parsed.bodies[1], Body::Mx(_)));
        assert_round_trip(src);
    }

    #[test]
    fn raw_unknown_block() {
        assert_round_trip(
            "meta { name: Future\n type: cnaps2 }\n\
             cnaps2 {\n\
               header: 0123\n\
               field 99: payload\n\
               trailing key: value\n\
             }",
        );
    }

    #[test]
    fn value_with_balanced_braces_round_trips() {
        // A value containing a balanced `{...}` (SWIFT sub-block) must
        // survive serialize → re-parse, since the value reader tracks
        // brace depth.
        assert_round_trip(
            "meta { name: Braces\n type: swift-mt }\n\
             swift-mt { block 3: {103:EBA}{108:MYREF} }",
        );
    }

    #[test]
    fn display_matches_to_wf_string() {
        let file = parse("meta { name: D\n type: iso8583 }\niso8583 { mti: 0200 }").unwrap();
        assert_eq!(format!("{file}"), to_wf_string(&file));
    }

    #[test]
    fn empty_value_emits_no_trailing_space() {
        let file = parse("meta { name: E\n type: iso8583 }\niso8583 { field 11: }").unwrap();
        let rendered = to_wf_string(&file);
        assert!(
            rendered.contains("field 11:\n"),
            "empty value should render as `field 11:` with no trailing space, got:\n{rendered}"
        );
    }
}
