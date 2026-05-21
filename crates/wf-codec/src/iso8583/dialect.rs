//! ISO 8583 wire-format dialects.
//!
//! ISO 8583 leaves the actual byte-level encoding under-specified, so deployed
//! systems pick one of several conventions for how the MTI, bitmap, and field
//! data are serialised. The two dialects we support today are the two seen in
//! the public OSS corpus (jpos packager fixtures, moov-io test vectors,
//! openiso8583 contributions). A third dialect (full binary with BCD-packed
//! numerics) is observed in mainframe-only fixtures and is not yet supported.
//!
//! Per the 2026-05-20 D1 dialect findings under
//! `docs/sample-acquisition-2026-05-20.md`, this enum is the API entry point
//! callers use to round-trip messages without losing the source convention.
//!
//! # Dialect cheat sheet
//!
//! | dialect       | MTI                       | bitmap                       | length prefixes | field bytes |
//! |---------------|---------------------------|------------------------------|-----------------|-------------|
//! | `HybridAscii` | 4 ASCII digit chars       | 8 / 16 raw binary bytes      | ASCII digits    | ASCII       |
//! | `FullAscii`   | 4 ASCII digit chars       | 16 / 32 ASCII hex chars      | ASCII digits    | ASCII       |
//!
//! Both dialects keep length prefixes and field payloads in ASCII; the only
//! axis of variation is the bitmap encoding. That is also why a sniffer can
//! disambiguate them by looking at the eight bytes immediately after the MTI.

/// Wire-level encoding flavour for an ISO 8583 message.
///
/// The variant order is deliberate: [`crate::iso8583::parse_any`] tries
/// dialects in this declaration order, so `HybridAscii` (our historical
/// dialect, exercised by every test vector in `tests/iso8583_message.rs`)
/// is tried first and existing callers see no behavioural change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// ASCII MTI + 8/16 raw-binary bitmap bytes + ASCII field data.
    ///
    /// Field 2's length prefix is `b"16"` (two ASCII digit bytes), the
    /// bitmap byte at offset 4 directly carries the secondary-present bit
    /// in its high bit, etc. This is the dialect that `wf-codec` has
    /// implemented since Sprint 1.
    HybridAscii,
    /// ASCII MTI + 16/32 ASCII hex bitmap chars + ASCII field data.
    ///
    /// The mainstream "text on the wire" flavour. jpos serialises this as
    /// the `ISO87A` / `ISO93A` packagers; moov-io's Go fixtures use it
    /// uniformly. The bitmap is rendered as uppercase hex by convention,
    /// but the parser accepts lowercase as well.
    FullAscii,
}

impl Dialect {
    /// All dialects this build of `wf-codec` supports, in the priority order
    /// the sniffer tries them.
    pub const ALL: &'static [Dialect] = &[Dialect::HybridAscii, Dialect::FullAscii];
}
