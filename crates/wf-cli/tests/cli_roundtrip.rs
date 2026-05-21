//! End-to-end MVP tests: spec-driven build + parse + tree + JSON.
//!
//! Hex bytes are reconstructed via the public `build_from_json` API and then
//! fed back into `parse_to_*` — this validates the binary is a round-trip
//! function, but does NOT prove correctness against an outside spec on its
//! own (see `wf-codec/tests/iso8583_message.rs` for hand-crafted spec
//! vectors that test the codec independently of itself).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wf_cli::{build_from_json, parse_to_json, parse_to_tree};

const TYPICAL_MSG_JSON: &str = r#"{
    "mti": "0200",
    "fields": {
        "3": "000000",
        "4": "000000010000",
        "7": "1130120000"
    }
}"#;

#[test]
fn build_then_parse_to_tree_shows_all_fields() {
    let hex = build_from_json(TYPICAL_MSG_JSON).expect("build");
    let tree = parse_to_tree(&hex).expect("parse");

    assert!(tree.contains("MTI: 0200"), "tree:\n{tree}");
    assert!(tree.contains("[  3]"), "tree:\n{tree}");
    assert!(tree.contains("[  4]"), "tree:\n{tree}");
    assert!(tree.contains("[  7]"), "tree:\n{tree}");
    assert!(tree.contains("Processing Code"), "tree:\n{tree}");
    assert!(tree.contains("\"000000\""), "tree:\n{tree}");
}

#[test]
fn build_then_parse_to_json_is_valid_and_complete() {
    let hex = build_from_json(TYPICAL_MSG_JSON).expect("build");
    let json = parse_to_json(&hex).expect("parse json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");

    assert_eq!(value["mti"], "0200");
    assert_eq!(value["has_secondary"], false);
    let fields = value["fields"].as_array().expect("fields array");
    assert_eq!(fields.len(), 3);
    let nums: Vec<u64> = fields
        .iter()
        .map(|f| f["number"].as_u64().unwrap())
        .collect();
    assert_eq!(nums, vec![3, 4, 7]);
    assert_eq!(fields[0]["name"], "Processing Code");
    assert_eq!(fields[0]["value_ascii"], "000000");
}

#[test]
fn empty_message_parses_to_no_fields() {
    let hex = build_from_json(r#"{"mti": "0800", "fields": {}}"#).expect("build");
    let tree = parse_to_tree(&hex).expect("parse");
    assert!(tree.contains("MTI: 0800"));
    assert!(tree.contains("(no fields)"));
}

#[test]
fn invalid_mti_in_build_input_is_rejected() {
    let err = build_from_json(r#"{"mti": "BAD", "fields": {}}"#).expect_err("must reject");
    assert!(err.contains("mti"), "err: {err}");
}

#[test]
fn invalid_hex_in_parse_input_is_rejected() {
    let err = parse_to_tree("not hex").expect_err("must reject");
    assert!(err.contains("non-hex") || err.contains("odd"), "err: {err}");
}

#[test]
fn whitespace_in_hex_input_is_tolerated() {
    let hex = build_from_json(TYPICAL_MSG_JSON).expect("build");
    let spaced = hex
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join(" \n ");
    let tree = parse_to_tree(&spaced).expect("parse with whitespace");
    assert!(tree.contains("MTI: 0200"));
}

#[test]
fn hex_encoded_field_value_round_trips() {
    let json = r#"{
        "mti": "0200",
        "fields": {
            "52": { "hex": "0123456789abcdef" }
        }
    }"#;
    let hex = build_from_json(json).expect("build");
    let parsed_json = parse_to_json(&hex).expect("parse");
    let value: serde_json::Value = serde_json::from_str(&parsed_json).unwrap();
    assert_eq!(value["fields"][0]["number"], 52);
    assert_eq!(value["fields"][0]["value_hex"], "0123456789abcdef");
    // 0x01..0xef contains non-printable bytes so value_ascii should be absent.
    assert!(value["fields"][0].get("value_ascii").is_none());
}

#[test]
fn binary_wf_parse_command_runs_end_to_end() {
    let hex = build_from_json(TYPICAL_MSG_JSON).expect("build");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wf"))
        .args(["parse", &hex])
        .output()
        .expect("run wf binary");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MTI: 0200"), "stdout:\n{stdout}");
}

#[test]
fn binary_wf_build_reads_stdin_and_emits_hex() {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(env!("CARGO_BIN_EXE_wf"))
        .arg("build")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn wf build");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(TYPICAL_MSG_JSON.as_bytes())
        .expect("write stdin");

    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let hex = stdout.trim();
    // Sanity: round-trip the binary output through parse_to_tree.
    let tree = parse_to_tree(hex).expect("parse output of binary build");
    assert!(tree.contains("MTI: 0200"));
}
