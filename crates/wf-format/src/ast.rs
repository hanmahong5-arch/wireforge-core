//! `.wf` file AST.
//!
//! The AST is intentionally shallow: each top-level `meta { ... }`,
//! `iso8583 { ... }`, `swift-mt { ... }`, or `mx { ... }` block lifts
//! into a strongly typed variant of [`Body`], and everything else falls
//! into [`Body::Raw`] so the parser never has to reject a file just
//! because the protocol block is new. A file may hold several payload
//! bodies (e.g. a matched `swift-mt` + `mx` pair).
//!
//! Field maps use `BTreeMap` rather than insertion-ordered structures
//! because:
//!
//! - The semantic meaning of `.wf` files is set-based, not list-based:
//!   `mti: 0200` followed by `field 2: 4242...` would compare equal to
//!   the same two entries in opposite order. A diff layer can therefore
//!   compare two parsed files directly without first canonicalising the
//!   key order.
//! - `BTreeMap`'s `Debug` output is sorted, so test failures are
//!   stable across runs and hash-randomisation seeds.

use std::collections::BTreeMap;

/// One parsed `.wf` file. Top-level items are kept in source order so
/// callers can re-emit the file with stable layout — sub-fields within a
/// `Body` variant use sorted maps for the reasons listed at the module
/// level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WfFile {
    /// Required: every file must have exactly one `meta { ... }`
    /// block. Validation runs after parsing — the parser also rejects
    /// files with no `meta` block, so reaching this type guarantees
    /// presence.
    pub meta: Meta,
    /// Zero or more payload blocks — each an `iso8583`, `swift-mt`, `mx`,
    /// or unrecognised raw block, kept in source order. Empty for a
    /// meta-only file (a legitimate state for templates / skeletons); a
    /// matched MT + MX pair file holds two (a `swift-mt` and an `mx`).
    pub bodies: Vec<Body>,
}

/// `meta { ... }` block. Known keys (`name`, `type`, `seq`) are lifted
/// to typed fields; anything else lands in [`Meta::extra`] verbatim so
/// the parser never fails on a key it does not recognise — additive
/// extensibility is the whole point.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    /// Human-readable name (the `name:` key inside the block).
    pub name: Option<String>,
    /// Protocol selector (`type: iso8583`, `type: swift-mt`, …).
    /// Lower-cased on parse so a comparison against the spec set is
    /// case-insensitive.
    pub type_: Option<String>,
    /// Optional sequence number (e.g. ordering inside a multi-message
    /// scenario). Stored as a string so leading zeros are preserved.
    pub seq: Option<String>,
    /// All keys not covered by the typed fields above.
    pub extra: BTreeMap<String, String>,
}

/// Top-level payload block kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// `iso8583 { ... }` block.
    Iso8583(Iso8583Body),
    /// `swift-mt { ... }` block.
    SwiftMt(SwiftMtBody),
    /// `mx { ... }` block — an opaque ISO 20022 envelope.
    Mx(MxBody),
    /// Any unrecognised top-level block kind. Holds the original block
    /// name plus its raw `key: value` entries so a diff layer can still
    /// compare two files that disagree only on extension blocks.
    Raw(RawBody),
}

/// `iso8583 { ... }` block.
///
/// The block accepts:
/// - `mti: <value>` — message type indicator (4-digit string).
/// - `field <N>: <value>` — one entry per data field.
/// - any other `key: value` — stashed into `extra` verbatim.
///
/// Field-value validity (PAN charset, amount format, …) is the codec
/// layer's job — `.wf` only carries the strings; downstream consumers
/// re-parse them through `wf-codec::iso8583`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Iso8583Body {
    /// MTI, if a `mti:` line was present.
    pub mti: Option<String>,
    /// Data fields keyed by field number (`field 2:` → `2 → "..."`).
    pub fields: BTreeMap<u8, String>,
    /// All `key: value` lines not covered by `mti` or `field N`.
    pub extra: BTreeMap<String, String>,
}

/// `swift-mt { ... }` block. Mirrors the SWIFT wrapper structure: an
/// optional set of opaque block strings (blocks 1, 2, 3, and optionally a
/// single-line block 4 and block 5) plus an optional nested block 4 of
/// `tag: value` fields.
///
/// Block 4 has two mutually exclusive forms — a single-line
/// `block 4: <value>` and a nested `block 4 { ... }` — and a file may use
/// at most one of them. A file that supplies both is rejected at parse
/// time (`DuplicateKey { key: "block 4" }`), so block 4 always has a
/// single source of truth.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwiftMtBody {
    /// Opaque block strings keyed by block id (`block 1: ...` →
    /// `1 → "..."`). A single-line `block 4: ...` form is stored **here**
    /// (keyed by id `4`); only the nested `block 4 { ... }` form is routed
    /// to [`SwiftMtBody::block_4`]. The two block-4 forms are mutually
    /// exclusive — a file using both is rejected.
    pub blocks: BTreeMap<u8, String>,
    /// Block 4's nested `tag: value` entries (`field 32A: 240520...`),
    /// populated only by the nested `block 4 { ... }` form. `None` if no
    /// nested block 4 appeared (including when a single-line `block 4: ...`
    /// was used instead — that lands in [`SwiftMtBody::blocks`]).
    pub block_4: Option<BTreeMap<String, String>>,
    /// All entries not matched by `block N` or `block 4 { … }`.
    pub extra: BTreeMap<String, String>,
}

/// `mx { ... }` block — an opaque ISO 20022 (MX) envelope.
///
/// `.wf` does **not** parse the XML: the `xml` key carries the whole
/// `<AppHdr>` + `<Document>` envelope verbatim as a single string, and
/// a downstream consumer (e.g. `wf-mx`) is responsible for interpreting
/// it. Mirrors [`RawBody`]'s "carry, don't validate" stance.
///
/// # Single-line constraint
///
/// The value **must be a single line** with no `{` / `}` characters. The
/// `.wf` lexer reads a value as the rest of one line (breaking at a `}`
/// seen at brace depth 0), so an XML envelope — which uses `<` / `>`, not
/// braces — round-trips intact only when emitted on one line. Multi-line
/// XML or XML containing literal braces is out of scope for this MVP.
///
/// The value carries `//` (e.g. `http://` namespace URIs) verbatim, but it
/// must **not** contain the C-style block-comment delimiters `/*` or `*/`:
/// those are removed by the whole-source `strip_block_comments` pre-pass
/// that runs before lexing. ISO 20022 XML uses `<!-- -->`, never `/* */`,
/// so this is a documented non-issue in practice.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MxBody {
    /// The opaque, single-line ISO 20022 envelope XML (the `xml:` key's
    /// value). Empty if no `xml:` key was present.
    pub xml: String,
}

/// Fall-through container for unrecognised top-level block kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBody {
    /// The block name as written in the source (e.g. `"cnaps2"`).
    pub name: String,
    /// All `key: value` entries inside the block. Nested blocks are
    /// flattened: a key like `"block 4"` would still appear here
    /// verbatim if present, since the Raw container has no special
    /// nesting awareness.
    pub entries: BTreeMap<String, String>,
}
