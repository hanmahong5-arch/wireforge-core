//! ISO 8583 wire-format dialects.
//!
//! ISO 8583 leaves the actual byte-level encoding under-specified, so deployed
//! systems pick one of several conventions for how the MTI, bitmap, and field
//! data are serialised. The three dialects we support today cover the
//! public OSS corpus (jpos packager fixtures, moov-io test vectors,
//! openiso8583 contributions): two ASCII variants plus the BCD-packed
//! mainframe variant observed in `ISO87BPackager` style fixtures.
//!
//! Per the 2026-05-20 D1 dialect findings under
//! `docs/sample-acquisition-2026-05-20.md`, this enum is the API entry point
//! callers use to round-trip messages without losing the source convention.
//!
//! # Dialect cheat sheet
//!
//! | dialect       | MTI                  | bitmap                       | length prefixes         | Numeric/Track data |
//! |---------------|----------------------|------------------------------|-------------------------|--------------------|
//! | `HybridAscii` | 4 ASCII digit chars  | 8 / 16 raw binary bytes      | ASCII digits            | ASCII chars        |
//! | `FullAscii`   | 4 ASCII digit chars  | 16 / 32 ASCII hex chars      | ASCII digits            | ASCII chars        |
//! | `FullBinary`  | 2 BCD bytes          | 8 / 16 raw binary bytes      | BCD (1 / 2 bytes)       | BCD (ceil(N/2) B)  |
//!
//! `FullBinary` stores Numeric / Track field data as BCD nibbles on the
//! wire but [`Iso8583Message`](super::parser::Iso8583Message) values are
//! kept in their decoded ASCII form regardless of dialect, so callers can
//! mix dialects on input and output without reasoning about nibble layout.
//! Non-numeric fields (Alpha, AlphaNumericSpecial, Binary, …) pass through
//! verbatim in every dialect.
//!
//! The sniffer in [`parse_any`](super::parser::parse_any) tries dialects in
//! declaration order; an unambiguous match wins, and ambiguous inputs
//! resolve to the earlier dialect (preserving historical behaviour).

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
    /// BCD-packed MTI + 8/16 raw-binary bitmap bytes + BCD-packed Numeric
    /// data and BCD length prefixes.
    ///
    /// The jpos `ISO87BPackager` and mainframe NDC dialect. Two decimal
    /// digits live in one byte (e.g. `"0800"` → `0x08 0x00`); a 16-digit
    /// PAN occupies 8 bytes; LLVAR prefix is one BCD byte, LLLVAR is two.
    /// Non-numeric field data (Alpha, AlphaNumericSpecial, Binary, …) is
    /// emitted verbatim as in the ASCII dialects.
    ///
    /// `Iso8583Message` field payloads are still kept in ASCII form after
    /// parsing (Numeric fields decode through [`super::bcd::decode_bcd`])
    /// so applications can mix dialects without reasoning about nibble
    /// layout.
    FullBinary,
}

impl Dialect {
    /// All dialects this build of `wf-codec` supports, in the priority order
    /// the sniffer tries them.
    ///
    /// `FullBinary` is placed last because its MTI byte range (`0x00..=0x99`
    /// per nibble) overlaps the printable ASCII space only above `0x30`, so
    /// any wire starting with an ASCII digit (HybridAscii / FullAscii) will
    /// be matched by an earlier dialect first; only the genuinely non-ASCII
    /// MTI bytes ever reach `FullBinary`.
    pub const ALL: &'static [Dialect] = &[
        Dialect::HybridAscii,
        Dialect::FullAscii,
        Dialect::FullBinary,
    ];
}
