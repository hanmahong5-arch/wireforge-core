//! CLI front-end for `sample-sanitize`. Reads a hex-encoded ISO 8583 wire
//! capture, runs it through [`sample_sanitize::sanitize::sanitize`], and
//! emits the redacted hex plus a `.meta.toml` audit record next to it.
//!
//! Usage:
//! ```text
//! sample-sanitize <input.hex.raw> \
//!     --source <source-slug> \
//!     --source-url <url> \
//!     --license <license-string> \
//!     [--source-commit <sha>] \
//!     [--fetched-at <iso8601-utc>] \
//!     [--sanitized-at <iso8601-utc>] \
//!     [--notes <free-text>] \
//!     --out <output.hex>
//! ```
//!
//! Timestamps are taken from CLI flags rather than the system clock so the
//! sanitizer is fully deterministic — the fetch step records `fetched_at`
//! once in `candidates/<source>/SOURCE.txt`, and the daily wave-report
//! script supplies the same `sanitized_at` to every invocation in a batch.

use sample_sanitize::meta::{SampleMeta, MIN_ANONYMITY_SET_WARN, SANITIZE_VERSION};
use sample_sanitize::sanitize::sanitize;
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
usage: sample-sanitize <input.hex.raw> --source <slug> --source-url <url> \
--license <license> --out <output.hex> [--source-commit <sha>] \
[--fetched-at <iso8601>] [--sanitized-at <iso8601>] [--notes <free-text>]";

fn main() -> ExitCode {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sample-sanitize: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &[OsString]) -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse(args)?;

    // 1. Read the raw hex blob — whitespace tolerated for human-pasted input.
    let raw_text =
        fs::read_to_string(&cli.input).map_err(|e| format!("read {}: {e}", cli.input.display()))?;
    let cleaned: String = raw_text.chars().filter(|c| !c.is_whitespace()).collect();
    let wire = hex_decode(&cleaned).map_err(|e| format!("hex decode failed: {e}"))?;

    // 2. Run the parse → redact → rebuild → round-trip pipeline.
    let out = sanitize(&wire)?;

    // 3. Compute audit hash and write redacted hex.
    let sha256 = sha256_hex(&out.redacted_bytes);
    let hex_out = hex_encode(&out.redacted_bytes);
    fs::write(&cli.out, format!("{hex_out}\n"))
        .map_err(|e| format!("write {}: {e}", cli.out.display()))?;

    // 4. Write the meta.toml sidecar.
    let meta_path = sidecar(&cli.out, "meta.toml");
    let notes = if out.fields_redacted.contains(&48) {
        format_notes(
            &cli.notes,
            "field 48 auto-masked as all-X — manually review for residual PII",
        )
    } else {
        cli.notes.clone()
    };
    let meta = SampleMeta {
        source: cli.source,
        source_url: cli.source_url,
        source_commit: cli.source_commit,
        license: cli.license,
        fetched_at: cli.fetched_at,
        sanitize_version: SANITIZE_VERSION,
        sanitized_at: cli.sanitized_at,
        round_trip_verified: true,
        byte_length: out.redacted_bytes.len(),
        sha256_redacted: sha256,
        fields_redacted: out.fields_redacted.clone(),
        anonymity_set_size: out.anonymity_set_size,
        dialect: format!("{:?}", out.dialect),
        notes,
    };
    fs::write(&meta_path, meta.to_toml())
        .map_err(|e| format!("write {}: {e}", meta_path.display()))?;

    // 5. Summary + low-anonymity warning. Exit-code stays 0 on warning so
    //    batch runs don't abort mid-pipeline; the operator reviews flagged
    //    samples from the meta files at end-of-day.
    eprintln!(
        "ok: {} -> {} ({} bytes, dialect {:?}, fields {:?}, anonymity ~10^{} = {})",
        cli.input.display(),
        cli.out.display(),
        out.redacted_bytes.len(),
        out.dialect,
        out.fields_redacted,
        log10_floor(out.anonymity_set_size),
        out.anonymity_set_size,
    );
    if meta.low_anonymity() && !meta.fields_redacted.is_empty() {
        eprintln!(
            "WARN: anonymity set {} < threshold {} — review {} before sharing",
            meta.anonymity_set_size,
            MIN_ANONYMITY_SET_WARN,
            meta_path.display()
        );
    }
    Ok(())
}

#[derive(Debug)]
struct Cli {
    input: PathBuf,
    out: PathBuf,
    source: String,
    source_url: String,
    license: String,
    source_commit: Option<String>,
    fetched_at: String,
    sanitized_at: String,
    notes: String,
}

impl Cli {
    fn parse(args: &[OsString]) -> Result<Cli, String> {
        let mut input: Option<PathBuf> = None;
        let mut out: Option<PathBuf> = None;
        let mut source: Option<String> = None;
        let mut source_url: Option<String> = None;
        let mut license: Option<String> = None;
        let mut source_commit: Option<String> = None;
        let mut fetched_at: Option<String> = None;
        let mut sanitized_at: Option<String> = None;
        let mut notes: Option<String> = None;

        let mut i = 0;
        while i < args.len() {
            let arg = args[i].to_string_lossy().into_owned();
            match arg.as_str() {
                "--help" | "-h" => return Err(USAGE.into()),
                "--source" => source = Some(take_value(args, &mut i, "--source")?),
                "--source-url" => source_url = Some(take_value(args, &mut i, "--source-url")?),
                "--license" => license = Some(take_value(args, &mut i, "--license")?),
                "--source-commit" => {
                    source_commit = Some(take_value(args, &mut i, "--source-commit")?)
                }
                "--fetched-at" => fetched_at = Some(take_value(args, &mut i, "--fetched-at")?),
                "--sanitized-at" => {
                    sanitized_at = Some(take_value(args, &mut i, "--sanitized-at")?)
                }
                "--notes" => notes = Some(take_value(args, &mut i, "--notes")?),
                "--out" => out = Some(PathBuf::from(take_value(args, &mut i, "--out")?)),
                other if other.starts_with("--") => {
                    return Err(format!("unknown flag {other}\n{USAGE}"));
                }
                _ => {
                    if input.is_some() {
                        return Err(format!("unexpected positional {arg}\n{USAGE}"));
                    }
                    input = Some(PathBuf::from(&arg));
                    i += 1;
                }
            }
        }

        Ok(Cli {
            input: input.ok_or_else(|| format!("missing input path\n{USAGE}"))?,
            out: out.ok_or_else(|| format!("missing --out\n{USAGE}"))?,
            source: source.ok_or_else(|| format!("missing --source\n{USAGE}"))?,
            source_url: source_url.ok_or_else(|| format!("missing --source-url\n{USAGE}"))?,
            license: license.ok_or_else(|| format!("missing --license\n{USAGE}"))?,
            source_commit,
            fetched_at: fetched_at.unwrap_or_else(|| "unknown".into()),
            sanitized_at: sanitized_at.unwrap_or_else(|| "unknown".into()),
            notes: notes.unwrap_or_default(),
        })
    }
}

fn take_value(args: &[OsString], i: &mut usize, flag: &str) -> Result<String, String> {
    let next = args
        .get(*i + 1)
        .ok_or_else(|| format!("{flag} requires a value"))?;
    *i += 2;
    Ok(next.to_string_lossy().into_owned())
}

fn sidecar(out: &Path, ext: &str) -> PathBuf {
    let mut p = out.to_path_buf();
    p.set_extension(ext);
    p
}

fn format_notes(user_notes: &str, extra: &str) -> String {
    if user_notes.is_empty() {
        extra.to_string()
    } else {
        format!("{user_notes} | {extra}")
    }
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) {
        return Err("odd-length hex".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = nibble(bytes[i])?;
        let lo = nibble(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(format!("non-hex byte: {b:#x}")),
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex_encode(&h.finalize())
}

fn log10_floor(n: u128) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut n = n;
    let mut k = 0u32;
    while n >= 10 {
        n /= 10;
        k += 1;
    }
    k
}
