#![allow(clippy::unwrap_used, clippy::panic)]

use wf_codec::iso8583::field::{field_def, DataType, LengthSpec};

#[test]
fn out_of_range_returns_none() {
    assert!(field_def(0).is_none());
    assert!(field_def(129).is_none());
    assert!(field_def(255).is_none());
}

#[test]
fn field_2_pan_llvar_19() {
    let f = field_def(2).unwrap();
    assert_eq!(f.number, 2);
    assert_eq!(f.data_type, DataType::Numeric);
    assert_eq!(f.length, LengthSpec::LLVAR { max: 19 });
    assert_eq!(f.name, "Primary Account Number");
}

#[test]
fn field_3_processing_code_n6() {
    let f = field_def(3).unwrap();
    assert_eq!(f.number, 3);
    assert_eq!(f.data_type, DataType::Numeric);
    assert_eq!(f.length, LengthSpec::Fixed(6));
    assert_eq!(f.name, "Processing Code");
}

#[test]
fn field_4_amount_transaction_n12() {
    let f = field_def(4).unwrap();
    assert_eq!(f.number, 4);
    assert_eq!(f.data_type, DataType::Numeric);
    assert_eq!(f.length, LengthSpec::Fixed(12));
    assert_eq!(f.name, "Amount, Transaction");
}

#[test]
fn field_7_transmission_date_time_n10() {
    let f = field_def(7).unwrap();
    assert_eq!(f.number, 7);
    assert_eq!(f.data_type, DataType::Numeric);
    assert_eq!(f.length, LengthSpec::Fixed(10));
    assert_eq!(f.name, "Transmission Date & Time");
}

#[test]
fn field_11_stan_n6() {
    let f = field_def(11).unwrap();
    assert_eq!(f.number, 11);
    assert_eq!(f.data_type, DataType::Numeric);
    assert_eq!(f.length, LengthSpec::Fixed(6));
    assert_eq!(f.name, "Systems Trace Audit Number");
}

#[test]
fn field_35_track2_z_llvar_37() {
    let f = field_def(35).unwrap();
    assert_eq!(f.number, 35);
    assert_eq!(f.data_type, DataType::Track);
    assert_eq!(f.length, LengthSpec::LLVAR { max: 37 });
    assert_eq!(f.name, "Track 2 Data");
}

#[test]
fn field_39_response_code_an2() {
    let f = field_def(39).unwrap();
    assert_eq!(f.number, 39);
    assert_eq!(f.data_type, DataType::AlphaNumeric);
    assert_eq!(f.length, LengthSpec::Fixed(2));
    assert_eq!(f.name, "Response Code");
}

#[test]
fn field_41_terminal_id_ans8() {
    let f = field_def(41).unwrap();
    assert_eq!(f.number, 41);
    assert_eq!(f.data_type, DataType::AlphaNumericSpecial);
    assert_eq!(f.length, LengthSpec::Fixed(8));
    assert_eq!(f.name, "Card Acceptor Terminal Identification");
}

#[test]
fn field_48_additional_data_private_an_lllvar_999() {
    let f = field_def(48).unwrap();
    assert_eq!(f.number, 48);
    assert_eq!(f.data_type, DataType::AlphaNumeric);
    assert_eq!(f.length, LengthSpec::LLLVAR { max: 999 });
    assert_eq!(f.name, "Additional Data - Private");
}

#[test]
fn field_52_pin_b8_bytes() {
    let f = field_def(52).unwrap();
    assert_eq!(f.number, 52);
    assert_eq!(f.data_type, DataType::Binary);
    assert_eq!(f.length, LengthSpec::Fixed(8));
    assert_eq!(f.name, "Personal Identification Number Data");
}

#[test]
fn field_70_network_management_n3() {
    let f = field_def(70).unwrap();
    assert_eq!(f.number, 70);
    assert_eq!(f.data_type, DataType::Numeric);
    assert_eq!(f.length, LengthSpec::Fixed(3));
    assert_eq!(f.name, "Network Management Information Code");
}

#[test]
fn reserved_fields_have_reserved_name() {
    for n in 105u8..=127u8 {
        let f = field_def(n).unwrap_or_else(|| panic!("field {n} should be Some"));
        assert_eq!(f.number, n);
        assert_eq!(f.name, "Reserved", "field {n} name");
    }
}

#[test]
fn field_1_is_secondary_bitmap() {
    let f = field_def(1).unwrap();
    assert_eq!(f.number, 1);
    assert_eq!(f.data_type, DataType::Binary);
    assert_eq!(f.length, LengthSpec::Fixed(8));
}

#[test]
fn field_128_mac2_b8() {
    let f = field_def(128).unwrap();
    assert_eq!(f.number, 128);
    assert_eq!(f.data_type, DataType::Binary);
    assert_eq!(f.length, LengthSpec::Fixed(8));
}

#[test]
fn every_defined_field_has_matching_number() {
    for n in 1u8..=128u8 {
        let f = field_def(n).unwrap_or_else(|| panic!("field {n} missing"));
        assert_eq!(f.number, n, "field {n} self-number mismatch");
    }
}
