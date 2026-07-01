//! Runtime [`FieldSpec`] tests.
//!
//! Two jobs:
//! 1. **Zero regression** — the default entry points (`parse_with` /
//!    `build_with`) must be byte-for-byte identical to passing
//!    [`FieldSpec::builtin`] explicitly, across every dialect. This is the
//!    contract that lets the spec abstraction land without touching any
//!    existing caller's behaviour.
//! 2. **Override behaviour** — a runtime spec actually changes how fields are
//!    parsed/built (redefined length, brand-new field, closed rejection).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;

use wf_codec::iso8583::field::{DataType, LengthSpec};
use wf_codec::iso8583::{
    build, build_with, build_with_spec, parse_with, parse_with_spec, BuildError, Dialect,
    FieldSpec, Iso8583Message, SpecError, SpecField,
};

fn msg(mti: &[u8; 4], fields: &[(u8, &[u8])]) -> Iso8583Message {
    let mut m = BTreeMap::new();
    for (n, v) in fields {
        m.insert(*n, v.to_vec());
    }
    Iso8583Message {
        mti: *mti,
        fields: m,
    }
}

/// A spread of representative messages: fixed numeric, LLVAR PAN, ans
/// pass-through, and a secondary-bitmap field.
fn corpus() -> Vec<Iso8583Message> {
    vec![
        msg(
            b"0200",
            &[
                (2, b"4242424242424242"),
                (3, b"000000"),
                (4, b"000000012345"),
                (11, b"000001"),
                (41, b"WF000001"),
            ],
        ),
        msg(
            b"0800",
            &[(7, b"1130120500"), (11, b"000002"), (70, b"301")],
        ),
        msg(b"0210", &[(39, b"00")]),
    ]
}

#[test]
fn default_entrypoints_equal_builtin_spec_across_dialects() {
    let builtin = FieldSpec::builtin();
    for m in corpus() {
        for &dialect in Dialect::ALL {
            let via_default = build_with(&m, dialect).expect("build_with");
            let via_spec = build_with_spec(&m, dialect, builtin).expect("build_with_spec");
            assert_eq!(
                via_default, via_spec,
                "build differs for MTI {:?} dialect {dialect:?}",
                m.mti
            );

            let parsed_default = parse_with(&via_default, dialect).expect("parse_with");
            let parsed_spec =
                parse_with_spec(&via_default, dialect, builtin).expect("parse_with_spec");
            assert_eq!(
                parsed_default, parsed_spec,
                "parse differs for MTI {:?} dialect {dialect:?}",
                m.mti
            );
            // And the whole thing round-trips back to the original.
            assert_eq!(parsed_default, m, "round-trip lost data for {:?}", m.mti);
        }
    }
}

#[test]
fn extending_builtin_redefines_field_length() {
    // Field 3 is Fixed(6) in the built-in table; pin it to Fixed(4).
    let spec = FieldSpec::extending_builtin([SpecField::new(
        3,
        DataType::Numeric,
        LengthSpec::Fixed(4),
        "Processing Code (4-digit variant)",
    )])
    .unwrap();

    let m = msg(b"0200", &[(3, b"1234")]);

    // The built-in path rejects the 4-byte payload (expects 6).
    assert!(
        matches!(
            build(&m),
            Err(BuildError::FixedLengthMismatch {
                field: 3,
                expected: 6,
                actual: 4
            })
        ),
        "built-in spec should reject a 4-byte field 3"
    );

    // The custom spec accepts it and round-trips.
    let wire = build_with_spec(&m, Dialect::HybridAscii, &spec).unwrap();
    let back = parse_with_spec(&wire, Dialect::HybridAscii, &spec).unwrap();
    assert_eq!(back, m);
}

#[test]
fn extending_builtin_adds_a_new_field_and_changes_interpretation() {
    // Field 105 is "Reserved" (Binary LLLVAR) in the built-in table. Define
    // it as a 3-digit fixed numeric field instead.
    let spec = FieldSpec::extending_builtin([SpecField::new(
        105,
        DataType::Numeric,
        LengthSpec::Fixed(3),
        "Custom national field 105",
    )])
    .unwrap();

    let m = msg(b"0210", &[(105, b"123")]);

    let wire = build_with_spec(&m, Dialect::HybridAscii, &spec).unwrap();
    let back = parse_with_spec(&wire, Dialect::HybridAscii, &spec).unwrap();
    assert_eq!(back, m);

    // The built-in spec reads those same 3 bytes as an LLLVAR length prefix
    // ("123" → expect 123 more bytes), so it interprets the wire differently
    // and fails — proving the spec genuinely drives interpretation.
    assert!(
        parse_with(&wire, Dialect::HybridAscii).is_err(),
        "built-in spec should not parse a Fixed(3) field 105 wire"
    );
}

#[test]
fn closed_spec_rejects_unlisted_field_on_build() {
    let spec = FieldSpec::closed(
        "PAN-only",
        [SpecField::new(
            2,
            DataType::Numeric,
            LengthSpec::LLVAR { max: 19 },
            "Primary Account Number",
        )],
    )
    .unwrap();

    // Field 3 is valid under the built-in table but absent from this closed
    // spec, so the build must refuse it rather than silently borrow the ISO
    // default.
    let m = msg(b"0200", &[(2, b"4242424242424242"), (3, b"000000")]);
    assert!(matches!(
        build_with_spec(&m, Dialect::HybridAscii, &spec),
        Err(BuildError::UnknownField(3))
    ));
}

// ---------------------------------------------------------------------------
// FIX E: LengthSpec max validation against wire prefix capacity
// ---------------------------------------------------------------------------

/// A spec with LLVAR max > 99 must be rejected at load time. A 2-digit wire
/// prefix can represent at most 99; silently accepting 5000 would allow
/// building messages that no compliant receiver could decode.
#[test]
fn extending_builtin_rejects_llvar_max_too_large() {
    let result = FieldSpec::extending_builtin([SpecField::new(
        48,
        DataType::AlphaNumericSpecial,
        LengthSpec::LLVAR { max: 5000 },
        "Private (oversized LLVAR)",
    )]);
    assert!(
        matches!(
            result,
            Err(SpecError::LengthMaxTooLarge {
                field: 48,
                prefix_digits: 2,
                max: 5000,
            })
        ),
        "expected LengthMaxTooLarge for LLVAR max:5000, got {result:?}",
    );
}

/// LLLVAR max > 999 is similarly rejected (3-digit prefix caps at 999).
#[test]
fn extending_builtin_rejects_lllvar_max_too_large() {
    let result = FieldSpec::extending_builtin([SpecField::new(
        48,
        DataType::AlphaNumericSpecial,
        LengthSpec::LLLVAR { max: 100_000 },
        "Private (oversized LLLVAR)",
    )]);
    assert!(
        matches!(
            result,
            Err(SpecError::LengthMaxTooLarge {
                field: 48,
                prefix_digits: 3,
                max: 100_000,
            })
        ),
        "expected LengthMaxTooLarge for LLLVAR max:100000, got {result:?}",
    );
}

/// LLVAR max == 99 is exactly at the boundary and must be accepted.
#[test]
fn extending_builtin_accepts_llvar_max_exactly_99() {
    let result = FieldSpec::extending_builtin([SpecField::new(
        48,
        DataType::AlphaNumericSpecial,
        LengthSpec::LLVAR { max: 99 },
        "Private (boundary LLVAR)",
    )]);
    assert!(
        result.is_ok(),
        "LLVAR max:99 must be accepted, got {result:?}"
    );
}

/// LLLVAR max == 999 is exactly at the boundary and must be accepted.
#[test]
fn extending_builtin_accepts_lllvar_max_exactly_999() {
    let result = FieldSpec::extending_builtin([SpecField::new(
        48,
        DataType::AlphaNumericSpecial,
        LengthSpec::LLLVAR { max: 999 },
        "Private (boundary LLLVAR)",
    )]);
    assert!(
        result.is_ok(),
        "LLLVAR max:999 must be accepted, got {result:?}"
    );
}
