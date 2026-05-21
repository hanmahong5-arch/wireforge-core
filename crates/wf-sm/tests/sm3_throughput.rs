//! Manual-run SM3 throughput bench.
//!
//! Why an integration test, not a `#[bench]` / `criterion` harness:
//! `#[bench]` requires nightly, and bringing `criterion` in as a
//! dev-dependency would inflate `cargo test --all-targets` for every
//! workspace contributor. The numbers needed for
//! `docs/sm-crypto-research-2026-05.md` are collected once when SM3
//! lands and again when the upstream is swapped — a manual-run
//! `#[ignore]` integration test is the lowest-friction way to make
//! that workflow repeatable without taxing CI.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo test -p wf-sm --release --test sm3_throughput -- --ignored --nocapture
//! ```
//!
//! Output is one line per input size: total bytes hashed, wall-clock
//! duration, and the derived MB/s figure. Numbers are noisy on a
//! laptop — record the median of three runs for the report.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::Instant;
use wf_sm::sm3;

/// Pick `iters` so each size hashes at least this many bytes total. A
/// small input would otherwise round to zero seconds and yield bogus
/// MB/s figures.
const TARGET_TOTAL_BYTES: usize = 64 * 1024 * 1024;

fn run_size(label: &str, bytes_per_iter: usize) {
    let buf = vec![0xA5u8; bytes_per_iter];
    let iters = (TARGET_TOTAL_BYTES / bytes_per_iter).max(1);

    // Warm-up — first call after process start can be slow on Windows
    // due to lazy DLL resolution / TLS table init.
    let _ = sm3(&buf);

    let start = Instant::now();
    for _ in 0..iters {
        let _ = sm3(&buf);
    }
    let elapsed = start.elapsed();

    let total_bytes = (bytes_per_iter as u128) * (iters as u128);
    let secs = elapsed.as_secs_f64();
    let mb_per_sec = if secs > 0.0 {
        (total_bytes as f64) / secs / (1024.0 * 1024.0)
    } else {
        f64::NAN
    };
    println!(
        "wf-sm/sm3 throughput: size={label:>6}  iters={iters:>6}  total={total_bytes:>10} B  elapsed={elapsed:?}  rate={mb_per_sec:>7.2} MB/s"
    );
}

#[test]
#[ignore = "manual: cargo test --release --test sm3_throughput -- --ignored --nocapture"]
fn throughput_1kb() {
    run_size("1 KB", 1024);
}

#[test]
#[ignore = "manual: cargo test --release --test sm3_throughput -- --ignored --nocapture"]
fn throughput_10kb() {
    run_size("10 KB", 10 * 1024);
}

#[test]
#[ignore = "manual: cargo test --release --test sm3_throughput -- --ignored --nocapture"]
fn throughput_100kb() {
    run_size("100 KB", 100 * 1024);
}

#[test]
#[ignore = "manual: cargo test --release --test sm3_throughput -- --ignored --nocapture"]
fn throughput_1mb() {
    run_size("1 MB", 1024 * 1024);
}
