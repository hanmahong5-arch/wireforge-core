//! ISO 8583-1987 data field type table (fields 1..=128).
//!
//! # Source
//!
//! Field definitions follow the ISO 8583-1987 specification as published in
//! the standard and reflected in widely-used public references (Wikipedia
//! `ISO_8583`, openiso8583, vendor implementation guides). The 1987 revision
//! reserves a large portion of the 56..=128 range for ISO / national / private
//! use without prescribing a concrete `(type, length)` triple; per the global
//! honesty constraint we do NOT fabricate definitions for those slots.
//!
//! # Coverage
//!
//! - **105 fields** (1..=104 plus 128) have a concrete definition taken from
//!   the spec. Note: fields 55..=63 are labelled by the spec as "Reserved for
//!   ISO / National / Private use" but ship with a defined envelope of
//!   `ans...999` (LLLVAR alpha-numeric-special); they are listed under their
//!   spec label rather than as opaque placeholders.
//! - **23 fields** (105..=127) are marked as [`name = "Reserved"`](FieldDef::name)
//!   with a neutral `Binary` + `LLLVAR { max: 999 }` placeholder. Callers MUST
//!   treat these as opaque envelopes (typically LLLVAR-prefixed binary blobs
//!   in practice) and refuse to encode without operator-supplied schema
//!   override.
//!
//! Total = 128 entries in the public table (index 1..=128).
//! `field_def(0)` and `field_def(n)` for `n > 128` return [`None`].

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    /// Numeric, digits `0..=9` only.
    Numeric,
    /// Alphabetic, `a-z` / `A-Z` only.
    Alpha,
    /// Special characters (punctuation, control).
    Special,
    /// Alpha + numeric.
    AlphaNumeric,
    /// Alpha + special.
    AlphaSpecial,
    /// Numeric + special.
    NumericSpecial,
    /// Alpha + numeric + special.
    AlphaNumericSpecial,
    /// Raw binary bytes. Lengths counted in BYTES (not bits) in this table.
    Binary,
    /// Track 2 / Track 3 character set (digits + `=`, `D`).
    Track,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthSpec {
    /// Fixed length in characters (or bytes for `Binary`).
    Fixed(usize),
    /// LLVAR: 2-digit length prefix, up to `max` data chars/bytes.
    LLVAR { max: usize },
    /// LLLVAR: 3-digit length prefix, up to `max` data chars/bytes.
    LLLVAR { max: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldDef {
    pub number: u8,
    pub data_type: DataType,
    pub length: LengthSpec,
    pub name: &'static str,
}

pub fn field_def(n: u8) -> Option<&'static FieldDef> {
    if n == 0 || (n as usize) >= FIELD_DEFS.len() {
        return None;
    }
    FIELD_DEFS[n as usize].as_ref()
}

const fn def(
    number: u8,
    data_type: DataType,
    length: LengthSpec,
    name: &'static str,
) -> Option<FieldDef> {
    Some(FieldDef {
        number,
        data_type,
        length,
        name,
    })
}

const fn reserved(number: u8) -> Option<FieldDef> {
    Some(FieldDef {
        number,
        data_type: DataType::Binary,
        length: LengthSpec::LLLVAR { max: 999 },
        name: "Reserved",
    })
}

use DataType::*;
use LengthSpec::{Fixed, LLLVAR, LLVAR};

static FIELD_DEFS: [Option<FieldDef>; 129] = [
    None,
    def(1, Binary, Fixed(8), "Secondary Bitmap"),
    def(2, Numeric, LLVAR { max: 19 }, "Primary Account Number"),
    def(3, Numeric, Fixed(6), "Processing Code"),
    def(4, Numeric, Fixed(12), "Amount, Transaction"),
    def(5, Numeric, Fixed(12), "Amount, Settlement"),
    def(6, Numeric, Fixed(12), "Amount, Cardholder Billing"),
    def(7, Numeric, Fixed(10), "Transmission Date & Time"),
    def(8, Numeric, Fixed(8), "Amount, Cardholder Billing Fee"),
    def(9, Numeric, Fixed(8), "Conversion Rate, Settlement"),
    def(10, Numeric, Fixed(8), "Conversion Rate, Cardholder Billing"),
    def(11, Numeric, Fixed(6), "Systems Trace Audit Number"),
    def(12, Numeric, Fixed(6), "Time, Local Transaction"),
    def(13, Numeric, Fixed(4), "Date, Local Transaction"),
    def(14, Numeric, Fixed(4), "Date, Expiration"),
    def(15, Numeric, Fixed(4), "Date, Settlement"),
    def(16, Numeric, Fixed(4), "Date, Conversion"),
    def(17, Numeric, Fixed(4), "Date, Capture"),
    def(18, Numeric, Fixed(4), "Merchant Type"),
    def(19, Numeric, Fixed(3), "Acquiring Institution Country Code"),
    def(20, Numeric, Fixed(3), "PAN Extended Country Code"),
    def(21, Numeric, Fixed(3), "Forwarding Institution Country Code"),
    def(22, Numeric, Fixed(3), "Point-of-Service Entry Mode"),
    def(23, Numeric, Fixed(3), "Card Sequence Number"),
    def(24, Numeric, Fixed(3), "Network International Identifier"),
    def(25, Numeric, Fixed(2), "Point-of-Service Condition Code"),
    def(26, Numeric, Fixed(2), "Point-of-Service Capture Code"),
    def(
        27,
        Numeric,
        Fixed(1),
        "Authorizing Identification Response Length",
    ),
    def(28, AlphaNumericSpecial, Fixed(9), "Amount, Transaction Fee"),
    def(29, AlphaNumericSpecial, Fixed(9), "Amount, Settlement Fee"),
    def(
        30,
        AlphaNumericSpecial,
        Fixed(9),
        "Amount, Transaction Processing Fee",
    ),
    def(
        31,
        AlphaNumericSpecial,
        Fixed(9),
        "Amount, Settlement Processing Fee",
    ),
    def(
        32,
        Numeric,
        LLVAR { max: 11 },
        "Acquiring Institution Identification Code",
    ),
    def(
        33,
        Numeric,
        LLVAR { max: 11 },
        "Forwarding Institution Identification Code",
    ),
    def(
        34,
        Numeric,
        LLVAR { max: 28 },
        "Primary Account Number, Extended",
    ),
    def(35, Track, LLVAR { max: 37 }, "Track 2 Data"),
    def(36, Numeric, LLLVAR { max: 104 }, "Track 3 Data"),
    def(
        37,
        AlphaNumericSpecial,
        Fixed(12),
        "Retrieval Reference Number",
    ),
    def(
        38,
        AlphaNumericSpecial,
        Fixed(6),
        "Authorization Identification Response",
    ),
    def(39, AlphaNumeric, Fixed(2), "Response Code"),
    def(
        40,
        AlphaNumericSpecial,
        Fixed(3),
        "Service Restriction Code",
    ),
    def(
        41,
        AlphaNumericSpecial,
        Fixed(8),
        "Card Acceptor Terminal Identification",
    ),
    def(
        42,
        AlphaNumericSpecial,
        Fixed(15),
        "Card Acceptor Identification Code",
    ),
    def(
        43,
        AlphaNumericSpecial,
        Fixed(40),
        "Card Acceptor Name/Location",
    ),
    def(
        44,
        AlphaNumeric,
        LLVAR { max: 25 },
        "Additional Response Data",
    ),
    def(45, AlphaNumeric, LLVAR { max: 76 }, "Track 1 Data"),
    def(
        46,
        AlphaNumeric,
        LLLVAR { max: 999 },
        "Additional Data - ISO",
    ),
    def(
        47,
        AlphaNumeric,
        LLLVAR { max: 999 },
        "Additional Data - National",
    ),
    def(
        48,
        AlphaNumeric,
        LLLVAR { max: 999 },
        "Additional Data - Private",
    ),
    def(49, AlphaNumeric, Fixed(3), "Currency Code, Transaction"),
    def(50, AlphaNumeric, Fixed(3), "Currency Code, Settlement"),
    def(
        51,
        AlphaNumeric,
        Fixed(3),
        "Currency Code, Cardholder Billing",
    ),
    def(52, Binary, Fixed(8), "Personal Identification Number Data"),
    def(
        53,
        Numeric,
        Fixed(16),
        "Security Related Control Information",
    ),
    def(54, AlphaNumeric, LLLVAR { max: 120 }, "Additional Amounts"),
    def(55, AlphaNumericSpecial, LLLVAR { max: 999 }, "Reserved ISO"),
    def(56, AlphaNumericSpecial, LLLVAR { max: 999 }, "Reserved ISO"),
    def(
        57,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved National",
    ),
    def(
        58,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved National",
    ),
    def(
        59,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved National",
    ),
    def(
        60,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved National",
    ),
    def(
        61,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved Private",
    ),
    def(
        62,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved Private",
    ),
    def(
        63,
        AlphaNumericSpecial,
        LLLVAR { max: 999 },
        "Reserved Private",
    ),
    def(64, Binary, Fixed(8), "Message Authentication Code"),
    def(65, Binary, Fixed(8), "Bitmap, Tertiary"),
    def(66, Numeric, Fixed(1), "Settlement Code"),
    def(67, Numeric, Fixed(2), "Extended Payment Code"),
    def(68, Numeric, Fixed(3), "Receiving Institution Country Code"),
    def(69, Numeric, Fixed(3), "Settlement Institution Country Code"),
    def(70, Numeric, Fixed(3), "Network Management Information Code"),
    def(71, Numeric, Fixed(4), "Message Number"),
    def(72, Numeric, Fixed(4), "Message Number, Last"),
    def(73, Numeric, Fixed(6), "Date, Action"),
    def(74, Numeric, Fixed(10), "Credits, Number"),
    def(75, Numeric, Fixed(10), "Credits, Reversal Number"),
    def(76, Numeric, Fixed(10), "Debits, Number"),
    def(77, Numeric, Fixed(10), "Debits, Reversal Number"),
    def(78, Numeric, Fixed(10), "Transfer, Number"),
    def(79, Numeric, Fixed(10), "Transfer, Reversal Number"),
    def(80, Numeric, Fixed(10), "Inquiries, Number"),
    def(81, Numeric, Fixed(10), "Authorizations, Number"),
    def(82, Numeric, Fixed(12), "Credits, Processing Fee Amount"),
    def(83, Numeric, Fixed(12), "Credits, Transaction Fee Amount"),
    def(84, Numeric, Fixed(12), "Debits, Processing Fee Amount"),
    def(85, Numeric, Fixed(12), "Debits, Transaction Fee Amount"),
    def(86, Numeric, Fixed(16), "Credits, Amount"),
    def(87, Numeric, Fixed(16), "Credits, Reversal Amount"),
    def(88, Numeric, Fixed(16), "Debits, Amount"),
    def(89, Numeric, Fixed(16), "Debits, Reversal Amount"),
    def(90, Numeric, Fixed(42), "Original Data Elements"),
    def(91, AlphaNumeric, Fixed(1), "File Update Code"),
    def(92, AlphaNumeric, Fixed(2), "File Security Code"),
    def(93, AlphaNumeric, Fixed(5), "Response Indicator"),
    def(94, AlphaNumeric, Fixed(7), "Service Indicator"),
    def(95, AlphaNumeric, Fixed(42), "Replacement Amounts"),
    def(96, Binary, Fixed(8), "Message Security Code"),
    def(97, AlphaNumericSpecial, Fixed(17), "Amount, Net Settlement"),
    def(98, AlphaNumericSpecial, Fixed(25), "Payee"),
    def(
        99,
        Numeric,
        LLVAR { max: 11 },
        "Settlement Institution Identification Code",
    ),
    def(
        100,
        Numeric,
        LLVAR { max: 11 },
        "Receiving Institution Identification Code",
    ),
    def(101, AlphaNumericSpecial, LLVAR { max: 17 }, "File Name"),
    def(
        102,
        AlphaNumericSpecial,
        LLVAR { max: 28 },
        "Account Identification 1",
    ),
    def(
        103,
        AlphaNumericSpecial,
        LLVAR { max: 28 },
        "Account Identification 2",
    ),
    def(
        104,
        AlphaNumericSpecial,
        LLLVAR { max: 100 },
        "Transaction Description",
    ),
    reserved(105),
    reserved(106),
    reserved(107),
    reserved(108),
    reserved(109),
    reserved(110),
    reserved(111),
    reserved(112),
    reserved(113),
    reserved(114),
    reserved(115),
    reserved(116),
    reserved(117),
    reserved(118),
    reserved(119),
    reserved(120),
    reserved(121),
    reserved(122),
    reserved(123),
    reserved(124),
    reserved(125),
    reserved(126),
    reserved(127),
    def(128, Binary, Fixed(8), "Message Authentication Code 2"),
];
