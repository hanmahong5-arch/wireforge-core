//! Integration tests for the `wf layout check` entry points.
//!
//! The trace fixtures are SYNTHETIC `bcl_dump`-shaped text built in-test
//! (header `[buffer dump: … length=N]`, hex lines of ≤16 byte pairs with an
//! ASCII gutter, end marker). Every expectation is hand-computed from the
//! layout's field lengths versus the frame lengths — the exact-tiling anchor.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;

use wf_cli::{extract_bcl_frames, layout_check_frame, layout_check_trace, parse_fixed_layout_toml};

/// Render one frame as a bcl-style dump block: 16 hex pairs per line plus an
/// ASCII gutter (which deliberately contains hex-looking text, to prove the
/// extractor never reads past the hex region).
fn dump_block(frame: &[u8]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "[buffer dump: pid=1234 length={}]", frame.len());
    out.push('\n');
    for chunk in frame.chunks(16) {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        // Gutter starts with "37" — a valid hex pair — so a naive extractor
        // that keeps reading tokens past the hex region would corrupt the
        // frame and fail the byte-for-byte assertions below.
        let _ = writeln!(out, "{}       37 gutter text", hex.join(" "));
    }
    out.push('\n');
    out.push_str("[buffer dump end]\n");
    out
}

/// A layout tiling exactly 4 + 2 + 6 = 12 bytes.
const LAYOUT_12: &str = r#"
name = "t12"
[[field]]
name = "len"
len = 4
[[field]]
name = "code"
len = 2
[[field]]
name = "body"
len = 6
"#;

#[test]
fn extractor_recovers_frames_byte_for_byte() {
    // Two frames: 12 bytes and 20 bytes (the second spans two hex lines).
    let f1: Vec<u8> = (0u8..12).collect();
    let f2: Vec<u8> = (100u8..120).collect();
    let trace = format!(
        "RECV:begin\nsome log line\n{}noise between dumps\n{}",
        dump_block(&f1),
        dump_block(&f2)
    );
    let (frames, dropped) = extract_bcl_frames(trace.as_bytes());
    assert_eq!(dropped, 0);
    assert_eq!(frames, vec![f1, f2], "frames must round-trip byte-for-byte");
}

#[test]
fn extractor_counts_truncated_dump_as_dropped() {
    // Header claims 32 bytes but only one 16-byte line follows before the
    // end marker → the dump is incomplete and must be dropped, not silently
    // emitted short.
    let half: Vec<u8> = (0u8..16).collect();
    let hex: Vec<String> = half.iter().map(|b| format!("{b:02x}")).collect();
    let trace = format!(
        "[buffer dump: pid=1 length=32]\n\n{}\n[buffer dump end]\n",
        hex.join(" ")
    );
    let (frames, dropped) = extract_bcl_frames(trace.as_bytes());
    assert!(frames.is_empty());
    assert_eq!(dropped, 1);
}

#[test]
fn matching_frames_gate_zero_and_report_counts() {
    // 12-byte frames tile the 12-byte layout; the 9-byte frame cannot.
    let trace = format!(
        "{}{}{}",
        dump_block(b"0012OKABCDEF"),
        dump_block(b"0012OKZZZZZZ"),
        dump_block(b"short f__")
    );
    let (body, code) = layout_check_trace(LAYOUT_12, trace.as_bytes());
    assert_eq!(code, 0, "at least one frame tiled → exit 0: {body}");
    assert!(
        body.contains("matched: 2/3"),
        "hand-count is 2 of 3: {body}"
    );
    assert!(
        body.contains("structural check only"),
        "must state the honest scope: {body}"
    );
}

#[test]
fn no_matching_frame_gates_one() {
    // Only an 11-byte frame: cannot tile 12 → exit 1 (draft disagrees).
    let (body, code) = layout_check_trace(LAYOUT_12, dump_block(b"elevenbytes").as_bytes());
    assert_eq!(code, 1, "no frame tiled → exit 1: {body}");
    assert!(body.contains("matched: 0/1"), "{body}");
}

#[test]
fn frameless_trace_and_bad_toml_gate_two() {
    // No dump blocks at all → uncheckable (2), distinct from "checked, no
    // match" (1).
    let (_, code) = layout_check_trace(LAYOUT_12, b"just log text, no dumps");
    assert_eq!(code, 2);
    // Malformed layout TOML → uncheckable (2).
    let (body, code) = layout_check_trace("not = = toml", dump_block(b"0012OKABCDEF").as_bytes());
    assert_eq!(code, 2, "{body}");
    // A field with both len and rest is contradictory → uncheckable (2).
    let contradictory = "[[field]]\nname = \"x\"\nlen = 4\nrest = true\n";
    let (_, code) = layout_check_trace(contradictory, dump_block(b"0012OKABCDEF").as_bytes());
    assert_eq!(code, 2);
}

#[test]
fn single_frame_mode_and_rest_tail() {
    // 4-byte prefix + variable tail: any frame of >= 4 bytes matches.
    let layout = r#"
[[field]]
name = "len"
len = 4
[[field]]
name = "body"
rest = true
"#;
    let (_, code) = layout_check_frame(layout, b"0003abc");
    assert_eq!(code, 0);
    let (_, code) = layout_check_frame(layout, b"003");
    assert_eq!(code, 1, "3 bytes cannot cover the 4-byte fixed prefix");
}

#[test]
fn layout_toml_rejects_mid_layout_rest() {
    let doc = r#"
[[field]]
name = "tail"
rest = true
[[field]]
name = "after"
len = 2
"#;
    assert!(
        parse_fixed_layout_toml(doc).is_err(),
        "rest before the last field must be rejected"
    );
}
