//! `.wf` file AST.
//!
//! The AST is intentionally shallow: each top-level `meta { ... }`,
//! `iso8583 { ... }`, or `swift-mt { ... }` block lifts into a strongly
//! typed variant of [`Body`], and everything else falls into
//! [`Body::Raw`] so the parser never has to reject a file just because
//! the protocol block is new.
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
    /// Optional payload block — `iso8583`, `swift-mt`, or an
    /// unrecognised raw block. `None` if the file is meta-only (a
    /// legitimate state for templates / skeletons).
    pub body: Option<Body>,
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
/// optional set of opaque block strings (blocks 1, 2, 3, 5) plus a
/// nested block 4 of `tag: value` fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SwiftMtBody {
    /// Opaque block strings keyed by block id (`block 1: ...` →
    /// `1 → "..."`). Block 4 is **not** stored here even if a
    /// `block 4: ...` single-line form is used — the parser routes it
    /// to [`SwiftMtBody::block_4`] instead.
    pub blocks: BTreeMap<u8, String>,
    /// Block 4's nested `tag: value` entries (`field 32A: 240520...`).
    /// `None` if no `block 4 { ... }` nested block appeared.
    pub block_4: Option<BTreeMap<String, String>>,
    /// All entries not matched by `block N` or `block 4 { … }`.
    pub extra: BTreeMap<String, String>,
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
