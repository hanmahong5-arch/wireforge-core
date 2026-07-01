//! Pure helpers for turning a parsed `.wf` file into the inputs the
//! MT/MX truncation detector consumes.
//!
//! These functions are **pure String builders** — wf-format stays
//! zero-dependency, so they reconstruct a SWIFT FIN wire string and pull
//! the opaque MX envelope out of the AST without ever parsing either
//! side. The actual MT/MX parsing happens in the consumer (e.g. wf-cli /
//! wf-mcp via wf-swift / wf-mx).

use crate::ast::{Body, Iso8583Body, MxBody, SwiftMtBody, WfFile};
use core::fmt;
use std::fmt::Write as _;

/// SWIFT FIN block 4 terminator (`-}` on its own, per the FIN wire
/// format).
const BLOCK4_TERMINATOR: &str = "-}";
/// The `.wf` key prefix for a block-4 field tag (`field 32A` → `32A`).
const FIELD_KEY_PREFIX: &str = "field ";

/// Reconstruct a SWIFT FIN wire string from a [`SwiftMtBody`] so that a
/// SWIFT MT parser (e.g. `wf_swift::parse`) accepts it.
///
/// Blocks are emitted in numeric order (1, 2, 3, 4, 5):
///
/// - Blocks 1–3 and 5, when present in [`SwiftMtBody::blocks`], emit as
///   `{<id>:<value>}`.
/// - Block 4 emits as the multi-line `{4:\r\n:<tag>:<value>\r\n…-}` form
///   when [`SwiftMtBody::block_4`] is `Some`; if instead a single-line
///   `block 4: <value>` was captured in `blocks`, that verbatim form is
///   used.
///
/// Field tag keys follow the `.wf` convention `"field <TAG>"`; the
/// `"field "` prefix is stripped to recover the bare SWIFT tag. Each
/// block-4 key MUST be a `field <TAG>` entry whose recovered tag is
/// non-empty `[A-Z0-9]+`; otherwise the reconstructed `:tag:` line
/// would be malformed (a bare `note` becomes `:note:`, which a FIN
/// consumer rejects) or silently mis-split (`field 32a` becomes
/// `:32a:`, mis-parsed as tag `32` + value `a:...`). Such a key yields
/// [`PairError::InvalidBlock4Tag`] rather than emitting corrupt wire.
/// Iteration is over `BTreeMap`s, so the output is deterministic.
pub fn swift_mt_to_fin(body: &SwiftMtBody) -> Result<String, PairError> {
    let mut out = String::new();

    // Blocks 1, 2, 3 (any block id strictly below 4) in sorted order.
    for (id, value) in body.blocks.range(..4u8) {
        let _ = write!(out, "{{{id}:{value}}}");
    }

    // Block 4: prefer a single-line `block 4: …` form if present,
    // otherwise expand the nested `block_4` field map.
    if let Some(value) = body.blocks.get(&4u8) {
        let _ = write!(out, "{{4:{value}}}");
    } else if let Some(fields) = &body.block_4 {
        out.push_str("{4:\r\n");
        for (key, value) in fields {
            let tag = block_4_tag(key)?;
            let _ = write!(out, ":{tag}:{value}\r\n");
        }
        out.push_str(BLOCK4_TERMINATOR);
    }

    // Block 5 (and any block id strictly above 4) after block 4.
    for (id, value) in body.blocks.range(5u8..) {
        let _ = write!(out, "{{{id}:{value}}}");
    }

    Ok(out)
}

/// Recover and validate the bare SWIFT tag from a `.wf` block-4 key.
///
/// The key must start with `"field "` and the recovered tag must be a
/// non-empty run of ASCII uppercase letters and digits (`[A-Z0-9]+`).
/// Anything else would reconstruct to a malformed or mis-split `:tag:`
/// line, so it is rejected with [`PairError::InvalidBlock4Tag`].
fn block_4_tag(key: &str) -> Result<&str, PairError> {
    let tag = key
        .strip_prefix(FIELD_KEY_PREFIX)
        .ok_or_else(|| PairError::InvalidBlock4Tag {
            key: key.to_string(),
        })?;
    let is_valid_tag = !tag.is_empty()
        && tag
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if is_valid_tag {
        Ok(tag)
    } else {
        Err(PairError::InvalidBlock4Tag {
            key: key.to_string(),
        })
    }
}

/// What a [`WfFile`] is missing or malformed when an MT + MX pair cannot
/// be extracted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    /// No `swift-mt { ... }` block was found in the file.
    MissingSwiftMt,
    /// No `mx { ... }` block was found in the file.
    MissingMx,
    /// A block-4 key was not a valid `field <TAG>` entry, so it could not
    /// be reconstructed into a well-formed `:tag:` FIN line.
    InvalidBlock4Tag {
        /// The offending key as written in the `.wf` swift-mt block.
        key: String,
    },
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Three-element messages: what is missing / what was expected /
        // what the caller can do about it.
        match self {
            PairError::MissingSwiftMt => write!(
                f,
                "no `swift-mt` block found; expected a `.wf` holding both a \
                 `swift-mt` and an `mx` block; add the missing `swift-mt` block"
            ),
            PairError::MissingMx => write!(
                f,
                "no `mx` block found; expected a `.wf` holding both a \
                 `swift-mt` and an `mx` block; add the missing `mx` block"
            ),
            PairError::InvalidBlock4Tag { key } => write!(
                f,
                "block-4 key `{key}` is not a valid `field <TAG>` entry \
                 (tag must be A-Z/0-9); expected every block-4 line to be \
                 `field <TAG>: <value>`; fix the offending key in the `.wf` \
                 swift-mt block"
            ),
        }
    }
}

impl std::error::Error for PairError {}

/// Extract a matched `(mt_fin_wire, mx_xml)` pair from a parsed `.wf`
/// file.
///
/// The first [`Body::SwiftMt`] is reconstructed into a FIN wire string
/// via [`swift_mt_to_fin`]; the first [`Body::Mx`]'s opaque envelope is
/// returned verbatim. Both must be present, otherwise a [`PairError`]
/// states which block is missing and how to fix it.
pub fn extract_mt_mx_pair(file: &WfFile) -> Result<(String, String), PairError> {
    let mt = file
        .bodies
        .iter()
        .find_map(|b| match b {
            Body::SwiftMt(s) => Some(s),
            _ => None,
        })
        .ok_or(PairError::MissingSwiftMt)?;
    let mx: &MxBody = file
        .bodies
        .iter()
        .find_map(|b| match b {
            Body::Mx(m) => Some(m),
            _ => None,
        })
        .ok_or(PairError::MissingMx)?;
    Ok((swift_mt_to_fin(mt)?, mx.xml.clone()))
}

/// What a [`WfFile`] is missing or malformed when an oracle req/legacy/migrated
/// triple cannot be extracted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OraclePairError {
    /// No `iso8583` body carried `role: <role>`. `role` is one of `req`,
    /// `legacy`, `migrated`.
    MissingRole {
        /// The role that was not found.
        role: &'static str,
    },
    /// Two `iso8583` bodies carried the same `role: <role>`, so the triple has
    /// two sources of truth for one slot.
    DuplicateRole {
        /// The role that appeared more than once.
        role: &'static str,
    },
}

impl fmt::Display for OraclePairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Three-element messages: what is missing / what was expected /
        // what the caller can do about it.
        match self {
            OraclePairError::MissingRole { role } => write!(
                f,
                "no `iso8583` block tagged `role: {role}`; expected a `.wf` holding three \
                 `iso8583` blocks tagged `role: req`, `role: legacy`, and `role: migrated`; \
                 add the missing `role: {role}` block"
            ),
            OraclePairError::DuplicateRole { role } => write!(
                f,
                "two `iso8583` blocks tagged `role: {role}`; expected exactly one block per \
                 role (req / legacy / migrated); remove the duplicate `role: {role}` block"
            ),
        }
    }
}

impl std::error::Error for OraclePairError {}

/// Extract the `(req, legacy, migrated)` ISO 8583 bodies from a parsed `.wf`
/// file for the conformance engine.
///
/// The three bodies are identified by a `role:` line inside each `iso8583`
/// block (which the parser stashes in [`Iso8583Body::extra`]): `role: req`,
/// `role: legacy`, `role: migrated`. Each must appear exactly once. The
/// `oracle-spec` block — if present — rides separately as a
/// [`Body::Raw`](crate::ast::Body::Raw); this function does not touch it, so
/// the spec→`OracleSpec` adapter can live in the consumer (keeping wf-format
/// zero-dependency). Bodies are cloned out so the caller owns them.
///
/// Returns an [`OraclePairError`] naming the missing or duplicated role.
pub fn extract_oracle_triple(
    file: &WfFile,
) -> Result<(Iso8583Body, Iso8583Body, Iso8583Body), OraclePairError> {
    let mut req: Option<Iso8583Body> = None;
    let mut legacy: Option<Iso8583Body> = None;
    let mut migrated: Option<Iso8583Body> = None;
    for body in &file.bodies {
        let Body::Iso8583(iso) = body else { continue };
        // Roles other than the three known ones are ignored rather than
        // rejected, so a file may carry extra annotated iso8583 blocks.
        match iso.extra.get("role").map(String::as_str) {
            Some("req") => take_role(&mut req, iso, "req")?,
            Some("legacy") => take_role(&mut legacy, iso, "legacy")?,
            Some("migrated") => take_role(&mut migrated, iso, "migrated")?,
            _ => {}
        }
    }
    let req = req.ok_or(OraclePairError::MissingRole { role: "req" })?;
    let legacy = legacy.ok_or(OraclePairError::MissingRole { role: "legacy" })?;
    let migrated = migrated.ok_or(OraclePairError::MissingRole { role: "migrated" })?;
    Ok((req, legacy, migrated))
}

/// Fill `slot` with a clone of `iso`, or error if it was already filled
/// (duplicate role).
fn take_role(
    slot: &mut Option<Iso8583Body>,
    iso: &Iso8583Body,
    role: &'static str,
) -> Result<(), OraclePairError> {
    if slot.is_some() {
        return Err(OraclePairError::DuplicateRole { role });
    }
    *slot = Some(iso.clone());
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::ast::Meta;
    use std::collections::BTreeMap;

    #[test]
    fn swift_mt_to_fin_builds_expected_wire_longhand() {
        // Anti-tautology: the expected wire is written out by hand, not
        // derived from the function under test.
        let mut block_4 = BTreeMap::new();
        block_4.insert("field 20".to_string(), "REF12345".to_string());
        block_4.insert("field 32A".to_string(), "240115USD1234,56".to_string());
        let mut blocks = BTreeMap::new();
        blocks.insert(1u8, "F01BANKUS33AXXX0000000000".to_string());
        blocks.insert(2u8, "I103BANKGB22XXXXN".to_string());
        blocks.insert(5u8, "{CHK:ABCDEF123456}".to_string());
        let body = SwiftMtBody {
            blocks,
            block_4: Some(block_4),
            extra: BTreeMap::new(),
        };

        // BTreeMap sorts block-4 keys: "field 20" < "field 32A".
        let expected = "{1:F01BANKUS33AXXX0000000000}\
            {2:I103BANKGB22XXXXN}\
            {4:\r\n:20:REF12345\r\n:32A:240115USD1234,56\r\n-}\
            {5:{CHK:ABCDEF123456}}";
        assert_eq!(
            swift_mt_to_fin(&body).expect("valid block-4 tags"),
            expected
        );
    }

    #[test]
    fn swift_mt_to_fin_rejects_block_4_key_without_field_prefix() {
        // A bare `note` key (no `field ` prefix) would emit a malformed
        // `:note:` line a FIN consumer rejects — must error instead.
        let mut block_4 = BTreeMap::new();
        block_4.insert("note".to_string(), "free text".to_string());
        let body = SwiftMtBody {
            blocks: BTreeMap::new(),
            block_4: Some(block_4),
            extra: BTreeMap::new(),
        };
        assert_eq!(
            swift_mt_to_fin(&body),
            Err(PairError::InvalidBlock4Tag {
                key: "note".to_string(),
            })
        );
    }

    #[test]
    fn swift_mt_to_fin_rejects_lowercase_tag() {
        // `field 32a` (lowercase tag) would emit `:32a:` which a consumer
        // mis-splits into tag `32` + value `a:...` — silent corruption.
        let mut block_4 = BTreeMap::new();
        block_4.insert("field 32a".to_string(), "240115USD1,00".to_string());
        let body = SwiftMtBody {
            blocks: BTreeMap::new(),
            block_4: Some(block_4),
            extra: BTreeMap::new(),
        };
        assert_eq!(
            swift_mt_to_fin(&body),
            Err(PairError::InvalidBlock4Tag {
                key: "field 32a".to_string(),
            })
        );
    }

    #[test]
    fn swift_mt_to_fin_uses_single_line_block_4_when_present() {
        let mut blocks = BTreeMap::new();
        blocks.insert(1u8, "F01TESTXXX".to_string());
        blocks.insert(4u8, ":20:REF\r\n:32A:240115USD1,00\r\n-".to_string());
        let body = SwiftMtBody {
            blocks,
            block_4: None,
            extra: BTreeMap::new(),
        };
        let expected = "{1:F01TESTXXX}{4::20:REF\r\n:32A:240115USD1,00\r\n-}";
        assert_eq!(
            swift_mt_to_fin(&body).expect("single-line block 4 present"),
            expected
        );
    }

    #[test]
    fn extract_pair_errors_when_swift_mt_missing() {
        let file = WfFile {
            meta: Meta::default(),
            bodies: vec![Body::Mx(MxBody {
                xml: "<Envelope/>".to_string(),
            })],
        };
        assert_eq!(extract_mt_mx_pair(&file), Err(PairError::MissingSwiftMt));
    }

    #[test]
    fn extract_pair_errors_when_mx_missing() {
        let file = WfFile {
            meta: Meta::default(),
            bodies: vec![Body::SwiftMt(SwiftMtBody::default())],
        };
        assert_eq!(extract_mt_mx_pair(&file), Err(PairError::MissingMx));
    }

    #[test]
    fn extract_pair_returns_first_of_each() {
        let body = SwiftMtBody {
            blocks: BTreeMap::from([(1u8, "F01TESTXXX".to_string())]),
            block_4: None,
            extra: BTreeMap::new(),
        };
        let file = WfFile {
            meta: Meta::default(),
            bodies: vec![
                Body::SwiftMt(body),
                Body::Mx(MxBody {
                    xml: "<Envelope>opaque</Envelope>".to_string(),
                }),
            ],
        };
        let (mt, mx) = extract_mt_mx_pair(&file).expect("pair present");
        assert_eq!(mt, "{1:F01TESTXXX}");
        assert_eq!(mx, "<Envelope>opaque</Envelope>");
    }

    /// An `iso8583` body whose `extra` carries `role: <role>` and one field,
    /// so a test can tell the three slots apart.
    fn iso_role(role: &str, field2: &str) -> Iso8583Body {
        Iso8583Body {
            mti: Some("0210".to_string()),
            fields: BTreeMap::from([(2u8, field2.to_string())]),
            extra: BTreeMap::from([("role".to_string(), role.to_string())]),
        }
    }

    #[test]
    fn extract_oracle_triple_returns_three_by_role() {
        // Source order is migrated, req, legacy — extraction is by `role:`,
        // not position, so the returned tuple is (req, legacy, migrated).
        let file = WfFile {
            meta: Meta::default(),
            bodies: vec![
                Body::Iso8583(iso_role("migrated", "MIG")),
                Body::Iso8583(iso_role("req", "REQ")),
                Body::Iso8583(iso_role("legacy", "LEG")),
            ],
        };
        let (req, legacy, migrated) = extract_oracle_triple(&file).expect("triple present");
        assert_eq!(req.fields.get(&2).map(String::as_str), Some("REQ"));
        assert_eq!(legacy.fields.get(&2).map(String::as_str), Some("LEG"));
        assert_eq!(migrated.fields.get(&2).map(String::as_str), Some("MIG"));
    }

    #[test]
    fn extract_oracle_triple_errors_on_missing_role() {
        // req + legacy present, migrated missing.
        let file = WfFile {
            meta: Meta::default(),
            bodies: vec![
                Body::Iso8583(iso_role("req", "REQ")),
                Body::Iso8583(iso_role("legacy", "LEG")),
            ],
        };
        assert_eq!(
            extract_oracle_triple(&file),
            Err(OraclePairError::MissingRole { role: "migrated" })
        );
    }

    #[test]
    fn extract_oracle_triple_errors_on_duplicate_role() {
        let file = WfFile {
            meta: Meta::default(),
            bodies: vec![
                Body::Iso8583(iso_role("req", "REQ")),
                Body::Iso8583(iso_role("legacy", "LEG")),
                Body::Iso8583(iso_role("migrated", "MIG")),
                Body::Iso8583(iso_role("legacy", "LEG2")),
            ],
        };
        assert_eq!(
            extract_oracle_triple(&file),
            Err(OraclePairError::DuplicateRole { role: "legacy" })
        );
    }
}
