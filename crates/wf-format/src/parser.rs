//! Recursive-descent parser for the `.wf` flat-file format.
//!
//! See the [`crate`] module documentation for the grammar overview. In
//! short, a file is a `meta { ... }` block followed by zero or more
//! payload blocks (`iso8583`, `swift-mt`, `mx`, or unrecognised raw
//! blocks). A matched MT + MX pair file holds two payload blocks.
//! Inside a block, each non-empty line is one of:
//!
//! - `key: value` — `value` runs from the `:` to end-of-line (trimmed).
//! - `key arg: value` — same, with an extra identifier / number arg.
//! - `key { ... }` — nested block.
//! - `key arg { ... }` — nested block with extra arg.
//!
//! No nested blocks inside `meta`; nested `block 4 { ... }` is the only
//! nested block in MVP `swift-mt`. Anything beyond that flows through
//! the [`crate::ast::RawBody`] catch-all so a future spec extension
//! never has to round-trip through a parser change.

use crate::ast::{Body, Iso8583Body, Meta, MxBody, RawBody, SwiftMtBody, WfFile};
use crate::lexer::{strip_block_comments, LexError, Lexer, Tok};
use core::fmt;
use std::collections::BTreeMap;

/// Failure modes for [`parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Lexer-level failure: malformed comment, stray character, etc.
    Lex(LexError),
    /// Top-level `meta { ... }` block was missing (every `.wf` file
    /// must declare one).
    MissingMeta,
    /// Two `meta` blocks appeared in one file. Payload blocks may repeat
    /// (a matched MT + MX pair has two), so only `meta` triggers this.
    DuplicateBlock { name: String },
    /// `}` appeared where a key was expected, but the matching block
    /// was never opened (more closes than opens).
    UnmatchedClose { offset: usize },
    /// Reached EOF before a `{` block was closed.
    UnclosedBlock { name: String },
    /// Unexpected token in the position a key or block opener was
    /// expected. Carries a short label of what was expected for
    /// readable error messages.
    UnexpectedToken { expected: &'static str, got: String },
    /// `field NNN:` was used but `NNN` was not in `0..=255`.
    InvalidFieldNumber { value: String },
    /// `block N:` or `block N { ... }` was used but `N` was not in
    /// `1..=5` (per the SWIFT MT spec).
    InvalidBlockNumber { value: String },
    /// Same key appeared twice in one block (e.g. two `mti:` lines).
    DuplicateKey { key: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Lex(e) => write!(f, "lex error: {e}"),
            ParseError::MissingMeta => write!(f, "file is missing a top-level `meta` block"),
            ParseError::DuplicateBlock { name } => write!(f, "duplicate top-level block `{name}`"),
            ParseError::UnmatchedClose { offset } => {
                write!(f, "unexpected `}}` at offset {offset} (no matching `{{`)")
            }
            ParseError::UnclosedBlock { name } => {
                write!(f, "block `{name}` never closed before end of input")
            }
            ParseError::UnexpectedToken { expected, got } => {
                write!(f, "expected {expected}, got {got}")
            }
            ParseError::InvalidFieldNumber { value } => {
                write!(f, "`field {value}` — field number must be 0..=255")
            }
            ParseError::InvalidBlockNumber { value } => {
                write!(f, "`block {value}` — block id must be 1..=5")
            }
            ParseError::DuplicateKey { key } => {
                write!(f, "duplicate key `{key}` in block")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError::Lex(e)
    }
}

/// Parse a `.wf` source string into a [`WfFile`].
pub fn parse(input: &str) -> Result<WfFile, ParseError> {
    let stripped = strip_block_comments(input)?;
    let mut lex = Lexer::new(&stripped);
    let mut meta: Option<Meta> = None;
    let mut bodies: Vec<Body> = Vec::new();

    loop {
        // Skip blank-line newlines between top-level blocks.
        let tok = next_non_newline(&mut lex)?;
        match tok {
            Tok::Eof => break,
            Tok::Ident(name) => match name.as_str() {
                "meta" => {
                    expect_lbrace(&mut lex)?;
                    if meta.is_some() {
                        return Err(ParseError::DuplicateBlock {
                            name: "meta".to_string(),
                        });
                    }
                    meta = Some(parse_meta(&mut lex)?);
                }
                "iso8583" => {
                    expect_lbrace(&mut lex)?;
                    bodies.push(Body::Iso8583(parse_iso8583(&mut lex)?));
                }
                "swift-mt" => {
                    expect_lbrace(&mut lex)?;
                    bodies.push(Body::SwiftMt(parse_swift_mt(&mut lex)?));
                }
                "mx" => {
                    expect_lbrace(&mut lex)?;
                    bodies.push(Body::Mx(parse_mx(&mut lex)?));
                }
                other => {
                    expect_lbrace(&mut lex)?;
                    bodies.push(Body::Raw(parse_raw(&mut lex, other)?));
                }
            },
            other => {
                return Err(ParseError::UnexpectedToken {
                    expected: "a top-level block name",
                    got: format!("{other:?}"),
                });
            }
        }
    }

    let meta = meta.ok_or(ParseError::MissingMeta)?;
    Ok(WfFile { meta, bodies })
}

/// One key + optional argument (`field 2`, `block 4`, `name`).
struct Key {
    name: String,
    arg: Option<String>,
}

/// What the key+arg pair resolves to once a separator is seen.
enum Resolved {
    Value(String),
    Nested,
}

/// Read a `key arg? (':' value | '{' )` opener. Returns `(Key, Resolved)`.
/// Trailing `Newline` after a value is consumed.
fn parse_entry_or_block(lex: &mut Lexer<'_>, first: Tok) -> Result<(Key, Resolved), ParseError> {
    let name = match first {
        Tok::Ident(s) => s,
        other => {
            return Err(ParseError::UnexpectedToken {
                expected: "a key name",
                got: format!("{other:?}"),
            })
        }
    };
    let mut arg: Option<String> = None;
    let line_start_pos = lex.pos();
    let next = lex.next_token()?;
    let after = match next {
        Tok::Ident(s) => {
            arg = Some(s);
            lex.next_token()?
        }
        Tok::Number(n) => {
            arg = Some(n);
            lex.next_token()?
        }
        other => other,
    };
    let key = Key { name, arg };
    match after {
        Tok::Colon => {
            let value = lex.read_value_until_newline();
            // After value, the lexer may queue a `Newline` (line broke
            // naturally), leave a `}` pending (value ended at the
            // enclosing block close), or sit at EOF. All three are
            // legal entry terminators — the outer block-parser loop
            // handles whichever comes next, so we don't drain a token
            // here.
            Ok((key, Resolved::Value(value)))
        }
        Tok::LBrace => Ok((key, Resolved::Nested)),
        other => Err(ParseError::UnexpectedToken {
            expected: "`:` or `{` after key",
            got: format!("{other:?}, line started at offset {line_start_pos}"),
        }),
    }
}

fn parse_meta(lex: &mut Lexer<'_>) -> Result<Meta, ParseError> {
    let mut meta = Meta::default();
    let mut seen_keys: BTreeMap<String, ()> = BTreeMap::new();
    loop {
        let tok = next_non_newline(lex)?;
        if matches!(tok, Tok::RBrace) {
            return Ok(meta);
        }
        if matches!(tok, Tok::Eof) {
            return Err(ParseError::UnclosedBlock {
                name: "meta".to_string(),
            });
        }
        let (key, resolved) = parse_entry_or_block(lex, tok)?;
        if key.arg.is_some() {
            // meta block does not take key+arg pairs in the MVP — stash
            // the rendered key in extra so we don't lose data.
            let full = format!("{} {}", key.name, key.arg.unwrap_or_default());
            let value = match resolved {
                Resolved::Value(v) => v,
                Resolved::Nested => {
                    return Err(ParseError::UnexpectedToken {
                        expected: "a `:` (nested blocks not allowed inside `meta`)",
                        got: "`{`".to_string(),
                    });
                }
            };
            if seen_keys.insert(full.clone(), ()).is_some() {
                return Err(ParseError::DuplicateKey { key: full });
            }
            meta.extra.insert(full, value);
            continue;
        }
        let value = match resolved {
            Resolved::Value(v) => v,
            Resolved::Nested => {
                return Err(ParseError::UnexpectedToken {
                    expected: "a `:` (nested blocks not allowed inside `meta`)",
                    got: "`{`".to_string(),
                });
            }
        };
        if seen_keys.insert(key.name.clone(), ()).is_some() {
            return Err(ParseError::DuplicateKey { key: key.name });
        }
        match key.name.as_str() {
            "name" => meta.name = Some(value),
            "type" => meta.type_ = Some(value.to_ascii_lowercase()),
            "seq" => meta.seq = Some(value),
            _ => {
                meta.extra.insert(key.name, value);
            }
        }
    }
}

fn parse_iso8583(lex: &mut Lexer<'_>) -> Result<Iso8583Body, ParseError> {
    let mut body = Iso8583Body::default();
    let mut seen_keys: BTreeMap<String, ()> = BTreeMap::new();
    loop {
        let tok = next_non_newline(lex)?;
        if matches!(tok, Tok::RBrace) {
            return Ok(body);
        }
        if matches!(tok, Tok::Eof) {
            return Err(ParseError::UnclosedBlock {
                name: "iso8583".to_string(),
            });
        }
        let (key, resolved) = parse_entry_or_block(lex, tok)?;
        let value = match resolved {
            Resolved::Value(v) => v,
            Resolved::Nested => {
                return Err(ParseError::UnexpectedToken {
                    expected: "a `:` (nested blocks not allowed inside `iso8583`)",
                    got: "`{`".to_string(),
                });
            }
        };
        match (key.name.as_str(), key.arg.as_deref()) {
            ("field", Some(n)) => {
                let num: u8 = n.parse().map_err(|_| ParseError::InvalidFieldNumber {
                    value: n.to_string(),
                })?;
                if body.fields.insert(num, value).is_some() {
                    return Err(ParseError::DuplicateKey {
                        key: format!("field {num}"),
                    });
                }
            }
            ("mti", None) => {
                if body.mti.is_some() {
                    return Err(ParseError::DuplicateKey {
                        key: "mti".to_string(),
                    });
                }
                body.mti = Some(value);
            }
            (name, None) => {
                if seen_keys.insert(name.to_string(), ()).is_some() {
                    return Err(ParseError::DuplicateKey {
                        key: name.to_string(),
                    });
                }
                body.extra.insert(name.to_string(), value);
            }
            (name, Some(arg)) => {
                let full = format!("{name} {arg}");
                if seen_keys.insert(full.clone(), ()).is_some() {
                    return Err(ParseError::DuplicateKey { key: full });
                }
                body.extra.insert(full, value);
            }
        }
    }
}

fn parse_swift_mt(lex: &mut Lexer<'_>) -> Result<SwiftMtBody, ParseError> {
    let mut body = SwiftMtBody::default();
    let mut seen_keys: BTreeMap<String, ()> = BTreeMap::new();
    loop {
        let tok = next_non_newline(lex)?;
        if matches!(tok, Tok::RBrace) {
            return Ok(body);
        }
        if matches!(tok, Tok::Eof) {
            return Err(ParseError::UnclosedBlock {
                name: "swift-mt".to_string(),
            });
        }
        let (key, resolved) = parse_entry_or_block(lex, tok)?;
        match (key.name.as_str(), key.arg.as_deref(), &resolved) {
            ("block", Some(n), Resolved::Value(_)) => {
                let id: u8 = n.parse().map_err(|_| ParseError::InvalidBlockNumber {
                    value: n.to_string(),
                })?;
                if !(1..=5).contains(&id) {
                    return Err(ParseError::InvalidBlockNumber {
                        value: n.to_string(),
                    });
                }
                // The single-line `block 4: …` form and the nested
                // `block 4 { … }` form are mutually exclusive — a file
                // using both has two sources of truth for block 4, which
                // would silently drop one. Reject the second form here.
                if id == 4 && body.block_4.is_some() {
                    return Err(ParseError::DuplicateKey {
                        key: "block 4".to_string(),
                    });
                }
                if let Resolved::Value(v) = resolved {
                    if body.blocks.insert(id, v).is_some() {
                        return Err(ParseError::DuplicateKey {
                            key: format!("block {id}"),
                        });
                    }
                }
            }
            ("block", Some(n), Resolved::Nested) => {
                let id: u8 = n.parse().map_err(|_| ParseError::InvalidBlockNumber {
                    value: n.to_string(),
                })?;
                if !(1..=5).contains(&id) {
                    return Err(ParseError::InvalidBlockNumber {
                        value: n.to_string(),
                    });
                }
                if id != 4 {
                    return Err(ParseError::UnexpectedToken {
                        expected: "nested `block 4 { ... }`; other blocks must use `block N: ...`",
                        got: format!("`block {id} {{`"),
                    });
                }
                if body.block_4.is_some() {
                    return Err(ParseError::DuplicateKey {
                        key: "block 4".to_string(),
                    });
                }
                // Mutually exclusive with the single-line `block 4: …`
                // form captured in `blocks` — reject if both appear.
                if body.blocks.contains_key(&4) {
                    return Err(ParseError::DuplicateKey {
                        key: "block 4".to_string(),
                    });
                }
                body.block_4 = Some(parse_block_4(lex)?);
            }
            (name, arg, Resolved::Value(_)) => {
                let full = match arg {
                    Some(a) => format!("{name} {a}"),
                    None => name.to_string(),
                };
                if seen_keys.insert(full.clone(), ()).is_some() {
                    return Err(ParseError::DuplicateKey { key: full });
                }
                if let Resolved::Value(v) = resolved {
                    body.extra.insert(full, v);
                }
            }
            (_, _, Resolved::Nested) => {
                return Err(ParseError::UnexpectedToken {
                    expected: "a `:` (only `block 4 { ... }` may nest inside `swift-mt`)",
                    got: "`{`".to_string(),
                });
            }
        }
    }
}

fn parse_block_4(lex: &mut Lexer<'_>) -> Result<BTreeMap<String, String>, ParseError> {
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let tok = next_non_newline(lex)?;
        if matches!(tok, Tok::RBrace) {
            return Ok(fields);
        }
        if matches!(tok, Tok::Eof) {
            return Err(ParseError::UnclosedBlock {
                name: "block 4".to_string(),
            });
        }
        let (key, resolved) = parse_entry_or_block(lex, tok)?;
        let value = match resolved {
            Resolved::Value(v) => v,
            Resolved::Nested => {
                return Err(ParseError::UnexpectedToken {
                    expected: "a `:` (no further nesting inside `block 4`)",
                    got: "`{`".to_string(),
                });
            }
        };
        let full = match key.arg {
            Some(a) => format!("{} {}", key.name, a),
            None => key.name,
        };
        if fields.insert(full.clone(), value).is_some() {
            return Err(ParseError::DuplicateKey { key: full });
        }
    }
}

/// Parse an `mx { ... }` block into an opaque [`MxBody`].
///
/// Only the `xml:` key is meaningful — its value is the single-line
/// ISO 20022 envelope, carried verbatim (wf-format does not interpret
/// the XML). Any other keys are accepted and ignored so a future
/// extension never forces a parser change. If no `xml:` key appears,
/// `xml` defaults to the empty string.
///
/// Unlike the other blocks, the `xml:` value is read with
/// [`Lexer::read_value_raw_until_newline`] rather than the normal
/// value reader: an ISO 20022 envelope legitimately contains `//`
/// (e.g. an `http://...` namespace URI), and the normal reader would
/// treat that as a line comment and silently truncate the envelope.
fn parse_mx(lex: &mut Lexer<'_>) -> Result<MxBody, ParseError> {
    let mut mx = MxBody::default();
    let mut seen_xml = false;
    loop {
        let tok = next_non_newline(lex)?;
        match tok {
            Tok::RBrace => return Ok(mx),
            Tok::Eof => {
                return Err(ParseError::UnclosedBlock {
                    name: "mx".to_string(),
                });
            }
            Tok::Ident(key) => {
                // Each entry is `key: <value>`; only `xml` is interpreted.
                let colon = lex.next_token()?;
                if !matches!(colon, Tok::Colon) {
                    return Err(ParseError::UnexpectedToken {
                        expected: "`:` after an `mx` key",
                        got: format!("{colon:?}"),
                    });
                }
                if key == "xml" {
                    if seen_xml {
                        return Err(ParseError::DuplicateKey {
                            key: "xml".to_string(),
                        });
                    }
                    seen_xml = true;
                    // Opaque read: keeps `//` (e.g. `http://`) intact.
                    mx.xml = lex.read_value_raw_until_newline();
                } else {
                    // Any other key is intentionally ignored — the `mx`
                    // block is an opaque envelope carrier; only `xml:` is
                    // interpreted. Consume its value so the loop advances.
                    let _ = lex.read_value_until_newline();
                }
            }
            other => {
                return Err(ParseError::UnexpectedToken {
                    expected: "an `mx` key name or `}`",
                    got: format!("{other:?}"),
                });
            }
        }
    }
}

fn parse_raw(lex: &mut Lexer<'_>, block_name: &str) -> Result<RawBody, ParseError> {
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    loop {
        let tok = next_non_newline(lex)?;
        if matches!(tok, Tok::RBrace) {
            return Ok(RawBody {
                name: block_name.to_string(),
                entries,
            });
        }
        if matches!(tok, Tok::Eof) {
            return Err(ParseError::UnclosedBlock {
                name: block_name.to_string(),
            });
        }
        let (key, resolved) = parse_entry_or_block(lex, tok)?;
        let value = match resolved {
            Resolved::Value(v) => v,
            Resolved::Nested => {
                return Err(ParseError::UnexpectedToken {
                    expected: "a `:` (raw blocks cannot nest in MVP)",
                    got: "`{`".to_string(),
                });
            }
        };
        let full = match key.arg {
            Some(a) => format!("{} {}", key.name, a),
            None => key.name,
        };
        if entries.insert(full.clone(), value).is_some() {
            return Err(ParseError::DuplicateKey { key: full });
        }
    }
}

fn expect_lbrace(lex: &mut Lexer<'_>) -> Result<(), ParseError> {
    let tok = next_non_newline(lex)?;
    match tok {
        Tok::LBrace => Ok(()),
        other => Err(ParseError::UnexpectedToken {
            expected: "`{` to open the block body",
            got: format!("{other:?}"),
        }),
    }
}

fn next_non_newline(lex: &mut Lexer<'_>) -> Result<Tok, ParseError> {
    loop {
        let t = lex.next_token()?;
        if !matches!(t, Tok::Newline) {
            return Ok(t);
        }
    }
}
