//! Wireforge observability helpers — a small, local-first logging toolkit
//! modeled on the Starring platform's `bcl_*` logging API (`capi.h`):
//! severity levels, raw-buffer hex dumps at a chosen level, and call-site
//! slicing (component / file / line) via `tracing` spans.
//!
//! Nothing here phones home. Events go through the `tracing` facade and are a
//! no-op unless a subscriber is installed, so leaving dumps in hot paths is
//! free when the level is off. The binaries install stderr subscribers
//! ([`init_cli_subscriber`] / [`init_server_subscriber`]); stdout is always
//! left untouched so command results / JSON-RPC framing stay machine-clean.
//!
//! Level mapping vs. Starring's four explicit severities plus a quiet base:
//! `bcl_log` ≈ `INFO`, then `err`/`warn`/`debug`/`trace` line up with
//! `tracing`'s `ERROR`/`WARN`/`DEBUG`/`TRACE`.

use std::fmt::Write as _;

use tracing::Level;
use tracing_subscriber::{filter::LevelFilter, EnvFilter};

/// Maximum number of bytes [`hexdump`] will render. Inputs are bounded so a
/// multi-megabyte buffer can never blow up a single log record
/// (bounded-everything rule: 有界一切). The tail beyond this is summarized, not dropped
/// silently.
pub const MAX_DUMP_BYTES: usize = 4096;

/// Bytes shown per row, matching the classic `hexdump -C` layout.
const ROW: usize = 16;

/// Render `bytes` as a canonical `offset  hex…  |ascii|` dump — the layout of
/// `hexdump -C` and Starring's `bcl_dump_buffer`. At most [`MAX_DUMP_BYTES`]
/// are shown; any remainder is summarized on a final line. An empty input
/// yields an empty string.
pub fn hexdump(bytes: &[u8]) -> String {
    let shown = bytes.len().min(MAX_DUMP_BYTES);
    // ~80 chars per 16-byte row; pre-size generously to avoid reallocs.
    let mut out = String::with_capacity(shown / ROW * 80 + 64);
    let mut off = 0;
    while off < shown {
        let end = (off + ROW).min(shown);
        let row = &bytes[off..end];
        let _ = write!(out, "{off:08x}  ");
        for i in 0..ROW {
            if i == ROW / 2 {
                out.push(' '); // gap between the two 8-byte halves
            }
            match row.get(i) {
                Some(b) => {
                    let _ = write!(out, "{b:02x} ");
                }
                None => out.push_str("   "), // pad the short final row
            }
        }
        out.push_str(" |");
        for &b in row {
            out.push(if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
        off = end;
    }
    if bytes.len() > shown {
        let _ = writeln!(
            out,
            "… ({} more byte(s) not shown; total {})",
            bytes.len() - shown,
            bytes.len()
        );
    }
    out
}

/// Emit a hex dump of `bytes` at `level`, tagged with `label`. This is the
/// single, level-parameterized analog of Starring's
/// `bcl_dump_buffer_{err,warn,debug,trace}`.
///
/// The dump string ([`hexdump`]) is only built when `level` is enabled by the
/// active subscriber — the `tracing` macros evaluate field/message arguments
/// lazily — so a `TRACE` dump on a hot path costs nothing when trace is off.
pub fn dump_buffer(level: Level, label: &str, bytes: &[u8]) {
    macro_rules! emit {
        ($m:ident) => {
            tracing::$m!(buffer = label, len = bytes.len(), "\n{}", hexdump(bytes))
        };
    }
    match level {
        Level::ERROR => emit!(error),
        Level::WARN => emit!(warn),
        Level::INFO => emit!(info),
        Level::DEBUG => emit!(debug),
        Level::TRACE => emit!(trace),
    }
}

/// Map a `-v` repeat count to a maximum log level — a quiet base plus the
/// four explicit severities above it, mirroring Starring's logging tiers:
/// `0 → WARN`, `1 → INFO`, `2 → DEBUG`, `3+ → TRACE`.
pub fn cli_level(verbosity: u8) -> LevelFilter {
    match verbosity {
        0 => LevelFilter::WARN,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    }
}

/// Install a stderr `tracing` subscriber for a CLI process. The level comes
/// from `verbosity` (see [`cli_level`]) unless `RUST_LOG` is set, which wins.
/// stdout is left untouched so command output stays machine-clean. Records
/// carry target + file + line so logs can be sliced by call site. Idempotent:
/// a second call (or a clash with an already-installed global subscriber) is a
/// silent no-op.
pub fn init_cli_subscriber(verbosity: u8) {
    let filter = EnvFilter::builder()
        .with_default_directive(cli_level(verbosity).into())
        .from_env_lossy();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .try_init();
}

/// Install a stderr subscriber for the MCP server: default `INFO`,
/// `RUST_LOG`-overridable. stdout is reserved for JSON-RPC framing, so all
/// logs go to stderr. Idempotent.
pub fn init_server_subscriber() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hexdump_layout_has_offset_hex_and_ascii() {
        // b"0200" = 0x30 0x32 0x30 0x30
        let d = hexdump(b"0200");
        assert!(d.starts_with("00000000  "), "offset column missing: {d}");
        assert!(d.contains("30 32 30 30"), "hex bytes missing: {d}");
        assert!(d.contains("|0200|"), "ascii gutter missing: {d}");
    }

    #[test]
    fn hexdump_non_printable_becomes_dot() {
        let d = hexdump(&[0x00, 0xff, b'A']);
        assert!(d.contains("|..A|"), "non-printable not dotted: {d}");
    }

    #[test]
    fn hexdump_empty_is_empty() {
        assert_eq!(hexdump(&[]), "");
    }

    #[test]
    fn hexdump_bounds_large_input() {
        let big = vec![0u8; MAX_DUMP_BYTES + 100];
        let d = hexdump(&big);
        assert!(
            d.contains("100 more byte(s)"),
            "tail summary missing for over-cap input"
        );
        assert!(
            d.contains(&format!("total {}", MAX_DUMP_BYTES + 100)),
            "total count missing"
        );
    }

    #[test]
    fn cli_level_maps_verbosity_to_starring_tiers() {
        assert_eq!(cli_level(0), LevelFilter::WARN);
        assert_eq!(cli_level(1), LevelFilter::INFO);
        assert_eq!(cli_level(2), LevelFilter::DEBUG);
        assert_eq!(cli_level(3), LevelFilter::TRACE);
        assert_eq!(cli_level(9), LevelFilter::TRACE);
    }
}
