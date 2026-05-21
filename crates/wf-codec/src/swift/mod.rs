//! SWIFT MT message parsing — structural layer.
//!
//! Scope of this module is **structural**, not semantic: blocks are
//! tokenised, block 4 fields are split into tag/value pairs, but the
//! values themselves are not interpreted. Semantic decoding of fields
//! like `:32A:` (date + currency + amount) or `:50K:` (multi-line
//! party identifier) happens in a later sprint, on top of these types.
//!
//! ## Why structure-first
//!
//! 1. Structural parsing is independent of MT type (MT103 / MT202 /
//!    MT199…) — the wrapper format `{1:…}{2:…}{3:…}{4:…\r\n-}{5:…}`
//!    is shared, so one parser covers every MT message we'll ever
//!    handle.
//! 2. The MT↔MX bi-directional diff demo (PLAN-v0.4 S6) needs to
//!    detect truncation at the field level, which means it needs the
//!    raw tag/value list before any semantic step. Doing semantics
//!    first would be solving the wrong problem.
//! 3. Real production samples vary in field set per acquirer; nailing
//!    the structure first lets us soak in real captures before
//!    over-fitting a semantic field table.
//!
//! ## Block conventions
//!
//! | block | content                                            | parsed as          |
//! |-------|----------------------------------------------------|--------------------|
//! | 1     | Basic header (sender BIC, session, sequence)       | [`Block::Raw`]     |
//! | 2     | Application header (input / output direction)      | [`Block::Raw`]     |
//! | 3     | User header — nested `{NNN:value}` sub-blocks      | [`Block::Tagged`]  |
//! | 4     | Text — `\r\n:tag:value`-delimited fields, `-` end  | [`Block::Text`]    |
//! | 5     | Trailer — nested `{TAG:value}` sub-blocks          | [`Block::Tagged`]  |
//!
//! Blocks 1 and 2 are kept as opaque strings on purpose: their layouts
//! are fixed-position and trivially decoded once we have a typed
//! application layer, but the structural parser does not need to know.

use std::collections::BTreeMap;

/// One parsed SWIFT MT message — a sparse map of block id → block content.
///
/// Block ids are 1..=5 per the SWIFT MT spec; any other id encountered
/// during parsing produces [`MtParseError::InvalidBlockId`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtMessage {
    pub blocks: BTreeMap<u8, Block>,
}

impl MtMessage {
    /// Block 4 (the text body) if present, as a slice of fields.
    pub fn text(&self) -> Option<&[MtField]> {
        match self.blocks.get(&4) {
            Some(Block::Text(fields)) => Some(fields),
            _ => None,
        }
    }

    /// Find the first field with the given tag in block 4. Tags are
    /// matched case-sensitively (SWIFT MT tags are uppercase A-Z plus
    /// digits, no normalisation needed).
    pub fn field(&self, tag: &str) -> Option<&MtField> {
        self.text()?.iter().find(|f| f.tag == tag)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// Opaque string content (blocks 1, 2). Stored verbatim — leading
    /// and trailing whitespace inside the block braces is preserved so
    /// round-trip rendering can reproduce the wire bytes exactly.
    Raw(String),
    /// Block 4 text body — ordered list of `:tag:value` fields.
    Text(Vec<MtField>),
    /// Blocks 3 / 5 — nested `{TAG:value}` sub-blocks. Order preserved.
    Tagged(Vec<MtSubBlock>),
}

/// One block-4 field: tag + value. Values may be multi-line; the parser
/// preserves embedded `\r\n` exactly so callers that re-emit can match
/// byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtField {
    pub tag: String,
    pub value: String,
}

/// One nested `{tag:value}` entry inside block 3 or block 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MtSubBlock {
    pub tag: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtParseError {
    /// Unbalanced or missing `{ … }` for a block at this offset.
    UnbalancedBrace { offset: usize },
    /// Block header was malformed (no `:` after the id digits).
    MissingBlockSeparator { offset: usize },
    /// Block id was not a 1..=5 digit.
    InvalidBlockId { offset: usize, found: String },
    /// Same block id appeared twice in one message.
    DuplicateBlock { id: u8 },
    /// Block 4 terminator (`-` at start of a line) was missing.
    MissingTextTerminator,
    /// Input had non-whitespace bytes that fell outside any `{…}` block.
    UnexpectedTrailingBytes { offset: usize },
    /// Block 4 field had no tag (`:` with no body before the next `:`
    /// or end of block).
    MalformedField { offset: usize },
    /// Sub-block (in block 3 / 5) was malformed at this offset.
    MalformedSubBlock { offset: usize },
}

impl std::fmt::Display for MtParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MtParseError::UnbalancedBrace { offset } => {
                write!(f, "unbalanced brace at offset {}", offset)
            }
            MtParseError::MissingBlockSeparator { offset } => {
                write!(f, "missing ':' after block id at offset {}", offset)
            }
            MtParseError::InvalidBlockId { offset, found } => write!(
                f,
                "invalid block id at offset {}: {:?} is not 1..=5",
                offset, found
            ),
            MtParseError::DuplicateBlock { id } => {
                write!(f, "duplicate block {} in message", id)
            }
            MtParseError::MissingTextTerminator => {
                write!(f, "block 4 missing '-' terminator on its own line")
            }
            MtParseError::UnexpectedTrailingBytes { offset } => {
                write!(f, "unexpected non-whitespace bytes at offset {}", offset)
            }
            MtParseError::MalformedField { offset } => {
                write!(f, "block 4 field at offset {} has no tag", offset)
            }
            MtParseError::MalformedSubBlock { offset } => {
                write!(f, "sub-block at offset {} is malformed", offset)
            }
        }
    }
}

impl std::error::Error for MtParseError {}

/// Parse a SWIFT MT message into structural blocks.
///
/// The input is the raw on-the-wire text (typically ASCII; non-ASCII
/// bytes are preserved verbatim inside block values). Whitespace
/// *between* top-level blocks is tolerated; whitespace *inside* block
/// values is preserved exactly.
pub fn parse(input: &str) -> Result<MtMessage, MtParseError> {
    let bytes = input.as_bytes();
    let mut blocks: BTreeMap<u8, Block> = BTreeMap::new();
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            break;
        }
        if bytes[cursor] != b'{' {
            return Err(MtParseError::UnexpectedTrailingBytes { offset: cursor });
        }
        let block_start = cursor;
        let body_end = match find_matching_brace(bytes, block_start) {
            Some(idx) => idx,
            None => {
                return Err(MtParseError::UnbalancedBrace {
                    offset: block_start,
                })
            }
        };
        let inner = &bytes[block_start + 1..body_end];
        let colon = match inner.iter().position(|b| *b == b':') {
            Some(i) => i,
            None => {
                return Err(MtParseError::MissingBlockSeparator {
                    offset: block_start,
                })
            }
        };
        let id_bytes = &inner[..colon];
        // Block ids are exactly one digit in 1..=5. Reject empty, "00",
        // "10", "1a", etc.
        if id_bytes.len() != 1 || !(b'1'..=b'5').contains(&id_bytes[0]) {
            return Err(MtParseError::InvalidBlockId {
                offset: block_start + 1,
                found: String::from_utf8_lossy(id_bytes).into_owned(),
            });
        }
        let id = id_bytes[0] - b'0';
        if blocks.contains_key(&id) {
            return Err(MtParseError::DuplicateBlock { id });
        }
        let content_bytes = &inner[colon + 1..];
        let content_offset = block_start + 1 + colon + 1;
        let content = match id {
            1 | 2 => Block::Raw(String::from_utf8_lossy(content_bytes).into_owned()),
            4 => Block::Text(parse_text_block(content_bytes, content_offset)?),
            3 | 5 => Block::Tagged(parse_tagged_block(content_bytes, content_offset)?),
            _ => unreachable!("id validated to 1..=5 above"),
        };
        blocks.insert(id, content);
        cursor = body_end + 1;
    }

    Ok(MtMessage { blocks })
}

/// Locate the byte offset of the `}` that closes the `{` at `start`.
///
/// Nested `{` / `}` are counted, so a block 3 like `{3:{108:REF}}` reads
/// as one outer block whose content contains a sub-block.
fn find_matching_brace(bytes: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Block 4 grammar (informal):
///
/// ```text
/// text-block := opt-newline ":" field ( "\r\n" ":" field )* "\r\n" "-"
/// field      := tag value
/// tag        := /[A-Z0-9]+/
/// value      := /.+/   (may span multiple physical lines)
/// ```
fn parse_text_block(content: &[u8], base_offset: usize) -> Result<Vec<MtField>, MtParseError> {
    let s = String::from_utf8_lossy(content);
    let trimmed = s.trim_start();
    // Find the trailing `-` terminator on its own line. Accept "\r\n-",
    // "\n-", or "-" as a sole content (degenerate empty block 4).
    let body_without_term = if let Some(stripped) = trimmed.strip_suffix("\r\n-") {
        stripped
    } else if let Some(stripped) = trimmed.strip_suffix("\n-") {
        stripped
    } else if trimmed == "-" {
        ""
    } else {
        return Err(MtParseError::MissingTextTerminator);
    };
    if body_without_term.is_empty() {
        return Ok(Vec::new());
    }
    let body = body_without_term.trim_start_matches(':');
    let mut chunks: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let body_bytes = body.as_bytes();
    let mut i = 0usize;
    while i < body_bytes.len() {
        let is_crlf_colon = i + 2 < body_bytes.len() && &body_bytes[i..i + 3] == b"\r\n:";
        let is_lf_colon = i + 1 < body_bytes.len() && &body_bytes[i..i + 2] == b"\n:";
        if is_crlf_colon || is_lf_colon {
            chunks.push(&body[start..i]);
            i += if is_crlf_colon { 2 } else { 1 };
            i += 1; // skip the ':'
            start = i;
        } else {
            i += 1;
        }
    }
    chunks.push(&body[start..]);

    let mut fields = Vec::with_capacity(chunks.len());
    for (idx, chunk) in chunks.iter().enumerate() {
        if chunk.is_empty() {
            return Err(MtParseError::MalformedField {
                offset: base_offset + idx,
            });
        }
        let tag_end = chunk
            .bytes()
            .position(|b| !(b.is_ascii_uppercase() || b.is_ascii_digit()))
            .unwrap_or(chunk.len());
        if tag_end == 0 {
            return Err(MtParseError::MalformedField {
                offset: base_offset + idx,
            });
        }
        let tag = chunk[..tag_end].to_string();
        let rest = &chunk[tag_end..];
        let value = if let Some(stripped) = rest.strip_prefix(':') {
            stripped.to_string()
        } else {
            rest.to_string()
        };
        fields.push(MtField { tag, value });
    }
    Ok(fields)
}

/// Block 3 / block 5 grammar:
///
/// ```text
/// tagged-block := ( "{" tag ":" value "}" )*
/// tag         := /[A-Z0-9]+/
/// value       := /[^}]*/
/// ```
fn parse_tagged_block(content: &[u8], base_offset: usize) -> Result<Vec<MtSubBlock>, MtParseError> {
    let mut subs = Vec::new();
    let mut cursor = 0usize;
    while cursor < content.len() {
        while cursor < content.len() && content[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor >= content.len() {
            break;
        }
        if content[cursor] != b'{' {
            return Err(MtParseError::MalformedSubBlock {
                offset: base_offset + cursor,
            });
        }
        let end = match find_matching_brace(content, cursor) {
            Some(e) => e,
            None => {
                return Err(MtParseError::UnbalancedBrace {
                    offset: base_offset + cursor,
                })
            }
        };
        let inner = &content[cursor + 1..end];
        let colon =
            inner
                .iter()
                .position(|b| *b == b':')
                .ok_or(MtParseError::MalformedSubBlock {
                    offset: base_offset + cursor,
                })?;
        let tag = String::from_utf8_lossy(&inner[..colon]).into_owned();
        let value = String::from_utf8_lossy(&inner[colon + 1..]).into_owned();
        if tag.is_empty() {
            return Err(MtParseError::MalformedSubBlock {
                offset: base_offset + cursor,
            });
        }
        subs.push(MtSubBlock { tag, value });
        cursor = end + 1;
    }
    Ok(subs)
}

/// Failure modes for [`build`].
///
/// The structural builder cannot construct an invalid wire layout from
/// well-typed inputs except in the cases below — most fields are
/// length-elastic strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MtBuildError {
    /// Block id outside the 1..=5 SWIFT spec range.
    InvalidBlockId { id: u8 },
    /// Block 4 field had a tag with non-`[A-Z0-9]` characters; the
    /// resulting wire bytes would mis-parse on the receiver side.
    InvalidFieldTag { tag: String },
    /// Sub-block tag was empty.
    EmptySubBlockTag,
}

impl std::fmt::Display for MtBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MtBuildError::InvalidBlockId { id } => {
                write!(f, "block id {} is outside the 1..=5 SWIFT spec range", id)
            }
            MtBuildError::InvalidFieldTag { tag } => {
                write!(
                    f,
                    "block 4 field tag {:?} contains non-[A-Z0-9] characters",
                    tag
                )
            }
            MtBuildError::EmptySubBlockTag => {
                write!(f, "sub-block has an empty tag")
            }
        }
    }
}

impl std::error::Error for MtBuildError {}

/// Serialise an [`MtMessage`] back to its wire form.
///
/// Block 4 is emitted with `\r\n` line endings (the SWIFT canonical
/// form). For a round-trip on an LF-only input, parse it first and the
/// emitted output will be in CRLF — equivalent meaning, canonical bytes.
///
/// Blocks are emitted in ascending id order (1, 2, 3, 4, 5). Empty
/// blocks (e.g. an `MtMessage` with no entry for id 3) are simply
/// skipped — SWIFT permits omitting blocks 3 and 5.
pub fn build(msg: &MtMessage) -> Result<String, MtBuildError> {
    let mut out = String::new();
    for (&id, block) in &msg.blocks {
        if !(1..=5).contains(&id) {
            return Err(MtBuildError::InvalidBlockId { id });
        }
        match (id, block) {
            (1 | 2, Block::Raw(s)) => {
                out.push('{');
                out.push((b'0' + id) as char);
                out.push(':');
                out.push_str(s);
                out.push('}');
            }
            (3 | 5, Block::Tagged(subs)) => {
                out.push('{');
                out.push((b'0' + id) as char);
                out.push(':');
                for sub in subs {
                    if sub.tag.is_empty() {
                        return Err(MtBuildError::EmptySubBlockTag);
                    }
                    out.push('{');
                    out.push_str(&sub.tag);
                    out.push(':');
                    out.push_str(&sub.value);
                    out.push('}');
                }
                out.push('}');
            }
            (4, Block::Text(fields)) => {
                out.push_str("{4:");
                if fields.is_empty() {
                    out.push_str("\r\n-}");
                } else {
                    for f in fields {
                        if f.tag.is_empty()
                            || !f
                                .tag
                                .bytes()
                                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                        {
                            return Err(MtBuildError::InvalidFieldTag { tag: f.tag.clone() });
                        }
                        out.push_str("\r\n:");
                        out.push_str(&f.tag);
                        out.push(':');
                        out.push_str(&f.value);
                    }
                    out.push_str("\r\n-}");
                }
            }
            // Block id and content kind disagreed — caller built an
            // MtMessage with e.g. `Block::Text` under id 1. Surface as
            // InvalidBlockId rather than silently re-categorising.
            _ => return Err(MtBuildError::InvalidBlockId { id }),
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn minimal_block_1_only() {
        let wire = "{1:F01BANKBICAA1234567890}";
        let msg = parse(wire).unwrap();
        let b1 = msg.blocks.get(&1).unwrap();
        match b1 {
            Block::Raw(s) => assert_eq!(s, "F01BANKBICAA1234567890"),
            _ => panic!("block 1 should be Raw"),
        }
        assert_eq!(msg.blocks.len(), 1);
    }

    #[test]
    fn duplicate_block_rejected() {
        let wire = "{1:AAA}{1:BBB}";
        let err = parse(wire).unwrap_err();
        assert_eq!(err, MtParseError::DuplicateBlock { id: 1 });
    }

    #[test]
    fn invalid_block_id_rejected() {
        let wire = "{7:nope}";
        let err = parse(wire).unwrap_err();
        assert!(matches!(err, MtParseError::InvalidBlockId { .. }));
    }

    #[test]
    fn unbalanced_brace_rejected() {
        let wire = "{1:no-close";
        let err = parse(wire).unwrap_err();
        assert!(matches!(err, MtParseError::UnbalancedBrace { .. }));
    }
}
