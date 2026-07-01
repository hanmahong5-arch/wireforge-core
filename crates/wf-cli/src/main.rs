use std::io::{self, Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use wf_cli::{
    build_from_json, ebcdic_decode_hex, ebcdic_encode_text, hex_to_bytes, layout_check_frame,
    layout_check_trace, mt_mx_truncation_diff, mt_mx_truncation_diff_from_wf, mx_address_report,
    oracle_report, oracle_report_from_wf, parse_to_json, parse_to_tree, render_address_scan,
    render_oracle_scan, select_xml, sm3_digest, swift_parse_to_json, swift_parse_to_tree,
    OracleEntry, ScanEntry,
};

/// Wireforge CLI for financial message codecs.
#[derive(Debug, Parser)]
#[command(name = "wf", version, about = "Wireforge — financial message codec CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Increase log verbosity (repeatable): -v info, -vv debug, -vvv trace.
    /// Logs go to stderr; stdout stays reserved for command output. The
    /// `RUST_LOG` env var, if set, overrides this.
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Parse an ISO 8583 hex string into a field tree.
    ///
    /// Pass the hex inline, or use `-` to read from stdin. Whitespace
    /// in the input is ignored.
    Parse {
        /// Hex string to parse, or `-` to read from stdin.
        hex: String,
        /// Emit JSON instead of a human tree.
        #[arg(long)]
        json: bool,
    },
    /// Build ISO 8583 wire bytes from a JSON message description read from stdin.
    ///
    /// Output is a hex string on stdout.
    Build,
    /// SWIFT MT operations.
    #[command(subcommand)]
    Swift(SwiftCommands),
    /// EBCDIC <-> Unicode conversion.
    #[command(subcommand)]
    Ebcdic(EbcdicCommands),
    /// MT <-> MX field truncation & loss detection.
    #[command(subcommand)]
    Xform(XformCommands),
    /// ISO 8583 regression-conformance EVIDENCE (Mode-A replay).
    #[command(subcommand)]
    Oracle(OracleCommands),
    /// Fixed-length record layout tools (spec-recovery verification).
    #[command(subcommand)]
    Layout(LayoutCommands),
    /// Compute the SM3 (GM/T 0004-2012) hash digest.
    ///
    /// By default the argument is a hex string (whitespace ignored), matching
    /// how `wf parse`/`wf build` take hex input. Pass `--text` to hash the raw
    /// UTF-8 bytes of the argument instead. The output is the lowercase 64-hex
    /// digest. This is a plain hash; no compliance claim is implied.
    Sm3 {
        /// Input to hash: a hex string by default, or raw text with `--text`.
        input: String,
        /// Interpret the argument as UTF-8 text rather than as hex.
        #[arg(long)]
        text: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SwiftCommands {
    /// Parse a SWIFT MT wire message into a block tree.
    ///
    /// Pass the message inline, or use `-` to read from stdin.
    Parse {
        /// SWIFT MT wire text, or `-` to read from stdin.
        wire: String,
        /// Emit JSON instead of a human tree.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum XformCommands {
    /// Detect field truncation / loss between a SWIFT MT103 and an ISO
    /// 20022 pacs.008.001.08.
    ///
    /// This is a DETECTOR, not a converter: it does not convert MT to MX or
    /// MX to MT, and makes no certification, conformance, or equivalence
    /// claim. Coverage is limited to pacs.008.001.08 vs MT103 across five
    /// roles only: debtor name, creditor name, remittance info, settlement
    /// amount, settlement currency.
    ///
    /// Reads the MT file and the MX file (a full <AppHdr>+<Document>
    /// envelope). At most one of the two paths may be `-` to read that side
    /// from stdin. Alternatively, pass `--wf <path>` to read a single `.wf`
    /// file that holds a matched `swift-mt` + `mx` pair, in which case the
    /// two positional paths must be omitted.
    Diff {
        /// SWIFT MT103 file (or `-` for stdin). Omit when using --wf.
        mt_file: Option<String>,
        /// ISO 20022 MX XML file (or `-` for stdin). Omit when using --wf.
        mx_file: Option<String>,
        /// A single `.wf` file holding a matched swift-mt + mx pair.
        #[arg(long)]
        wf: Option<String>,
    },
    /// Check debtor/creditor postal-address compliance of one-or-more
    /// pacs.008.001.08 / pacs.004.001.09 / pacs.003.001.08 / pain.001.001.09
    /// envelopes (message type auto-detected per file).
    ///
    /// Structural CBPR+ SR2026 presence check: verifies that TwnNm and Ctry
    /// appear in dedicated structured PstlAdr fields (mandatory 2026-11-14).
    /// This is a DETECTOR — NOT a full CBPR+ validation and NOT a
    /// certification.
    ///
    /// Input may be: one MX file (full <AppHdr>+<Document> envelope); several
    /// MX files; a single directory (its top-level *.xml files are scanned,
    /// sorted, one level only — no recursion); or `-` to read one envelope
    /// from stdin (`-` cannot be mixed with file paths). One
    /// unreadable/unparseable file does not abort the batch — it is reported
    /// and folded into the exit code.
    ///
    /// The exit code is diff-style, so the check gates CI:
    ///   0 = every input is compliant,
    ///   1 = ran cleanly but at least one input is non-compliant,
    ///   2 = at least one input could not be checked.
    /// Scan your outbound message store this way before the 2026-11-14
    /// deadline.
    AddressCheck {
        /// MX XML file(s), a directory to scan, or `-` for one stdin envelope.
        #[arg(required = true, num_args = 1..)]
        paths: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum OracleCommands {
    /// Produce deterministic ISO 8583 regression-conformance EVIDENCE by
    /// comparing a captured legacy response against a migrated response.
    ///
    /// This is EVIDENCE, NOT a proof, certification, or equivalence claim. It
    /// is a Mode-A replay: the two responses to the same request are diffed
    /// field-by-field under an operator-approved mask spec (STABLE / VOLATILE
    /// / CRYPTO / INTENDED-DELTA; unconsidered fields default to STABLE and
    /// fail closed). The output carries a coverage meter (value-bearing
    /// baseline fields only — VOLATILE/CRYPTO are excluded, never inflating
    /// the number) and a diff-style exit code:
    ///   0 = conformant (no drift),
    ///   1 = ran cleanly but found UNEXPLAINED drift,
    ///   2 = the comparison could not be performed (parse / spec error).
    ///
    /// Provide either four inputs — `--req`, `--legacy`, `--migrated` (each a
    /// file path, `hex:<bytes>`, or `-` for stdin; at most one `-`) and
    /// `--spec` (a mask spec TOML file) — OR a single `--wf <path>` holding a
    /// `req`/`legacy`/`migrated` iso8583 triple plus an `oracle-spec` block.
    /// `--wf` is mutually exclusive with the four flags.
    ///
    /// Fixtures are SYNTHETIC in this PoC. An MCP `wf_oracle_check` tool is
    /// intentionally deferred to keep the server's 12-tool surface stable —
    /// the engine is CLI-first.
    Check {
        /// A single `.wf` file holding a req/legacy/migrated triple +
        /// oracle-spec. Mutually exclusive with the four flags below.
        #[arg(long)]
        wf: Option<String>,
        /// Request bytes: a file path, `hex:<bytes>`, or `-` for stdin.
        #[arg(long)]
        req: Option<String>,
        /// Captured legacy response: a file path, `hex:<bytes>`, or `-`.
        #[arg(long)]
        legacy: Option<String>,
        /// Migrated system response: a file path, `hex:<bytes>`, or `-`.
        #[arg(long)]
        migrated: Option<String>,
        /// Mask specification TOML: a file path, or `-` for stdin.
        #[arg(long)]
        spec: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum LayoutCommands {
    /// Check a fixed-length layout draft against captured frames.
    ///
    /// The one-command verification step of the spec-recovery loop: a field
    /// table drafted from an interface spec (by hand or machine-assisted) is
    /// checked against real captured bytes BEFORE anyone trusts it. This is a
    /// STRUCTURAL check only — a layout "matches" a frame when its declared
    /// field lengths account for every byte (exact tiling, no truncation, no
    /// remainder); field values and semantics are NOT validated, and no
    /// certification claim is made.
    ///
    /// `--layout` is the layout TOML (`[[field]] name/len`, optional trailing
    /// `rest = true`). Give frames either via `--trace` (a `bcl_dump`-style
    /// trace file: `[buffer dump: … length=N]` blocks of hex lines; frames of
    /// all segments are extracted and grouped by length) or via `--frame`
    /// (one raw frame: file path, `hex:<bytes>`, or `-` for stdin). Exactly
    /// one of the two.
    ///
    /// The exit code is diff-style, so a draft gate can run in CI:
    ///   0 = the layout explains at least one captured frame,
    ///   1 = it explains none (the draft disagrees with the bytes),
    ///   2 = the input could not be checked (bad TOML / no frames).
    Check {
        /// Layout TOML: a file path, or `-` for stdin.
        #[arg(long)]
        layout: String,
        /// Trace file with `[buffer dump: …]` blocks (or `-` for stdin).
        #[arg(long)]
        trace: Option<String>,
        /// One raw frame: file path, `hex:<bytes>`, or `-` for stdin.
        #[arg(long)]
        frame: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum EbcdicCommands {
    /// Decode EBCDIC bytes (given as hex) into Unicode text.
    ///
    /// Whitespace in the hex input is ignored. Default code page is CP037.
    Decode {
        /// EBCDIC bytes as a hex string.
        hex: String,
        /// Code page: 037 (default) or 500.
        #[arg(long, default_value = "037")]
        cp: String,
    },
    /// Encode Unicode text into EBCDIC, printed as hex.
    ///
    /// Fails on a character with no EBCDIC representation in the code page.
    /// Default code page is CP037.
    Encode {
        /// Text to encode.
        text: String,
        /// Code page: 037 (default) or 500.
        #[arg(long, default_value = "037")]
        cp: String,
    },
}

/// Worker-thread stack size for command execution.
///
/// The `xform diff` path deserializes a deeply nested ISO 20022 envelope,
/// whose recursive-descent parse can exceed the platform default main-
/// thread stack (1 MiB on Windows). Running command work on a thread with
/// an explicit, generous stack keeps every subcommand robust without
/// changing any parsing logic.
const WORKER_STACK_BYTES: usize = 16 * 1024 * 1024;

/// One command's rendered stdout plus the process exit code it implies.
///
/// Most commands either succeed (printing output, exit 0) or error (exit 1
/// via `main`'s `Err` arm), so [`CmdOutcome::pass`] builds the success case.
/// The address-check gate is the one command that prints normally yet may
/// still carry a non-zero, diff-style code (0 compliant / 1 non-compliant /
/// 2 errored), which is why dispatch threads an explicit exit code through.
struct CmdOutcome {
    stdout: String,
    code: ExitCode,
}

impl CmdOutcome {
    /// A successful outcome: print `stdout` and exit 0.
    fn pass(stdout: String) -> Self {
        CmdOutcome {
            stdout,
            code: ExitCode::SUCCESS,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    // Install the stderr observability subscriber before any work runs; the
    // worker thread below inherits this global subscriber.
    wf_obs::init_cli_subscriber(cli.verbose);
    // Run the command on a worker thread with a large stack so deeply
    // nested inputs (e.g. an ISO 20022 envelope in `xform diff`) cannot
    // overflow the platform's smaller default main-thread stack.
    let worker = std::thread::Builder::new()
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || dispatch(cli.command));
    let result = match worker {
        Ok(handle) => match handle.join() {
            Ok(r) => r,
            Err(_) => Err("internal error: worker thread terminated unexpectedly".to_string()),
        },
        Err(e) => Err(format!("failed to start worker thread: {e}")),
    };
    match result {
        Ok(out) => {
            // println! also flushes on newline; emit and exit with the
            // command's own code (the address-check gate may be non-zero).
            println!("{}", out.stdout);
            out.code
        }
        Err(e) => {
            let _ = writeln!(io::stderr(), "wf: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Execute one parsed subcommand, returning its rendered output or a
/// human-readable error. Pure dispatch — no process exit, so it is easy
/// to run on a worker thread.
fn dispatch(command: Commands) -> Result<CmdOutcome, String> {
    // Per-command span carries the subcommand name (the call-site "component"
    // slice); the outcome is logged below so each invocation is observable end
    // to end without touching stdout.
    let _span = tracing::info_span!("cmd", cmd = command_name(&command)).entered();
    tracing::debug!("dispatch");
    // Every command but `address-check` succeeds-or-errors with exit 0/1, so
    // it maps through `CmdOutcome::pass`; `address-check` carries its own
    // diff-style exit code and returns a `CmdOutcome` directly.
    let result = match command {
        Commands::Parse { hex, json } => run_parse(&hex, json).map(CmdOutcome::pass),
        Commands::Build => run_build().map(CmdOutcome::pass),
        Commands::Swift(SwiftCommands::Parse { wire, json }) => {
            run_swift_parse(&wire, json).map(CmdOutcome::pass)
        }
        Commands::Ebcdic(EbcdicCommands::Decode { hex, cp }) => {
            ebcdic_decode_hex(&hex, &cp).map(CmdOutcome::pass)
        }
        Commands::Ebcdic(EbcdicCommands::Encode { text, cp }) => {
            ebcdic_encode_text(&text, &cp).map(CmdOutcome::pass)
        }
        Commands::Xform(XformCommands::Diff {
            mt_file,
            mx_file,
            wf,
        }) => run_xform_diff(mt_file.as_deref(), mx_file.as_deref(), wf.as_deref())
            .map(CmdOutcome::pass),
        Commands::Xform(XformCommands::AddressCheck { paths }) => run_address_check(&paths),
        Commands::Oracle(OracleCommands::Check {
            wf,
            req,
            legacy,
            migrated,
            spec,
        }) => run_oracle_check(
            wf.as_deref(),
            req.as_deref(),
            legacy.as_deref(),
            migrated.as_deref(),
            spec.as_deref(),
        ),
        Commands::Layout(LayoutCommands::Check {
            layout,
            trace,
            frame,
        }) => run_layout_check(&layout, trace.as_deref(), frame.as_deref()),
        Commands::Sm3 { input, text } => sm3_digest(&input, text).map(CmdOutcome::pass),
    };
    match &result {
        Ok(out) => tracing::debug!(bytes = out.stdout.len(), "ok"),
        Err(e) => tracing::warn!(error = %e, "error"),
    }
    result
}

/// Stable component label for a subcommand, used as the `cmd` span field.
fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Parse { .. } => "parse",
        Commands::Build => "build",
        Commands::Swift(SwiftCommands::Parse { .. }) => "swift.parse",
        Commands::Ebcdic(EbcdicCommands::Decode { .. }) => "ebcdic.decode",
        Commands::Ebcdic(EbcdicCommands::Encode { .. }) => "ebcdic.encode",
        Commands::Xform(XformCommands::Diff { .. }) => "xform.diff",
        Commands::Xform(XformCommands::AddressCheck { .. }) => "xform.address-check",
        Commands::Oracle(OracleCommands::Check { .. }) => "oracle.check",
        Commands::Layout(LayoutCommands::Check { .. }) => "layout.check",
        Commands::Sm3 { .. } => "sm3",
    }
}

fn run_parse(hex_arg: &str, as_json: bool) -> Result<String, String> {
    let hex_text = if hex_arg == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        buf
    } else {
        hex_arg.to_string()
    };
    wf_obs::dump_buffer(tracing::Level::TRACE, "parse.input", hex_text.as_bytes());
    if as_json {
        parse_to_json(&hex_text)
    } else {
        parse_to_tree(&hex_text)
    }
}

fn run_swift_parse(wire_arg: &str, as_json: bool) -> Result<String, String> {
    let wire_text = if wire_arg == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        buf
    } else {
        wire_arg.to_string()
    };
    wf_obs::dump_buffer(tracing::Level::TRACE, "swift.input", wire_text.as_bytes());
    if as_json {
        swift_parse_to_json(&wire_text)
    } else {
        swift_parse_to_tree(&wire_text)
    }
}

fn run_xform_diff(
    mt_arg: Option<&str>,
    mx_arg: Option<&str>,
    wf_arg: Option<&str>,
) -> Result<String, String> {
    match wf_arg {
        Some(wf_path) => {
            // `.wf` pair mode: the two positional paths must be absent so
            // the source of truth is unambiguous.
            if mt_arg.is_some() || mx_arg.is_some() {
                return Err(
                    "`--wf` cannot be combined with the positional mt_file / mx_file arguments; \
                     pass either a single `.wf` pair file via --wf, or the two MT/MX files, not both"
                        .to_string(),
                );
            }
            let wf_src = read_file_or_stdin(wf_path)?;
            mt_mx_truncation_diff_from_wf(&wf_src)
        }
        None => {
            // Two-file mode: both positional paths are required.
            let (mt_arg, mx_arg) = match (mt_arg, mx_arg) {
                (Some(mt), Some(mx)) => (mt, mx),
                _ => {
                    return Err(
                        "expected two arguments (mt_file and mx_file), or `--wf <path>` for a \
                         matched `.wf` pair; got neither complete pair nor --wf; supply both \
                         MT/MX file paths or use --wf"
                            .to_string(),
                    );
                }
            };
            if mt_arg == "-" && mx_arg == "-" {
                return Err(
                    "only one of the two inputs may be `-` (stdin); give a file path for the other"
                        .to_string(),
                );
            }
            let mt = read_file_or_stdin(mt_arg)?;
            let mx = read_file_or_stdin(mx_arg)?;
            mt_mx_truncation_diff(&mt, &mx)
        }
    }
}

/// Run the SR2026 address-compliance gate over one-or-more inputs.
///
/// Input mode is decided from the argument shape: a lone `-` reads one
/// envelope from stdin; a lone directory is scanned one level for sorted
/// `*.xml` files (empty → fail-loud `Err`, never a silent pass); otherwise
/// every argument is a file path. A per-file read/parse failure is captured
/// into its [`ScanEntry`] (so one bad file does not abort the batch), then
/// [`render_address_scan`] folds the verdicts into the diff-style exit code.
fn run_address_check(paths: &[String]) -> Result<CmdOutcome, String> {
    let entries: Vec<ScanEntry> = if paths.len() == 1 && paths[0] == "-" {
        // Single stdin envelope. A stdin read failure aborts (exit 1); a
        // parse failure is captured below and surfaces as the gate's exit 2.
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        wf_obs::dump_buffer(tracing::Level::TRACE, "file.input", buf.as_bytes());
        vec![ScanEntry {
            label: "-".to_string(),
            result: mx_address_report(&buf),
        }]
    } else if paths.len() == 1 && std::path::Path::new(&paths[0]).is_dir() {
        // One-level directory scan: top-level *.xml only, sorted.
        let dir = &paths[0];
        let mut names = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(|e| format!("read dir {dir:?}: {e}"))? {
            let entry = entry.map_err(|e| format!("read dir {dir:?}: {e}"))?;
            let path = entry.path();
            if path.is_file() {
                names.push(path.to_string_lossy().into_owned());
            }
        }
        let xml = select_xml(names);
        if xml.is_empty() {
            return Err(format!("no .xml files found in {dir}"));
        }
        xml.into_iter()
            .map(|p| {
                let result = read_mx_report(&p);
                ScanEntry { label: p, result }
            })
            .collect()
    } else {
        // One-or-more explicit file paths. `-` (stdin) cannot be mixed in.
        if paths.iter().any(|p| p == "-") {
            return Err(
                "`-` (stdin) cannot be combined with file paths; pass `-` alone, \
                 a single directory, or one-or-more file paths"
                    .to_string(),
            );
        }
        paths
            .iter()
            .map(|p| ScanEntry {
                label: p.clone(),
                result: read_mx_report(p),
            })
            .collect()
    };

    let (body, gate) = render_address_scan(&entries);
    Ok(CmdOutcome {
        stdout: body,
        code: ExitCode::from(gate.code()),
    })
}

/// Read one MX file and run the structural address check, capturing any
/// read / parse / unsupported-type failure as the entry's `Err` so a single
/// bad file does not abort a batch scan.
fn read_mx_report(path: &str) -> Result<wf_xform::AddressComplianceReport, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read file {path:?}: {e}"))?;
    wf_obs::dump_buffer(tracing::Level::TRACE, "file.input", content.as_bytes());
    mx_address_report(&content)
}

/// Read a path, or all of stdin when the argument is `-`.
fn read_file_or_stdin(arg: &str) -> Result<String, String> {
    let content = if arg == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        buf
    } else {
        std::fs::read_to_string(arg).map_err(|e| format!("read file {arg:?}: {e}"))?
    };
    wf_obs::dump_buffer(tracing::Level::TRACE, "file.input", content.as_bytes());
    Ok(content)
}

/// Run the ISO 8583 conformance EVIDENCE engine over one capture, returning a
/// [`CmdOutcome`] whose exit code is the diff-style gate (0 / 1 / 2).
///
/// Either `--wf <path>` (a single file holding the triple + spec) or the four
/// flags (`--req` / `--legacy` / `--migrated` / `--spec`); the two forms are
/// mutually exclusive. A parse / spec failure is captured into the single
/// [`OracleEntry`] (→ gate 2) rather than aborting, so `wf oracle check`
/// always emits an artifact and a meaningful exit code.
fn run_oracle_check(
    wf_arg: Option<&str>,
    req_arg: Option<&str>,
    legacy_arg: Option<&str>,
    migrated_arg: Option<&str>,
    spec_arg: Option<&str>,
) -> Result<CmdOutcome, String> {
    let entry = match wf_arg {
        Some(wf_path) => {
            // `.wf` mode: the four flags must be absent so the source of truth
            // is unambiguous.
            if req_arg.is_some()
                || legacy_arg.is_some()
                || migrated_arg.is_some()
                || spec_arg.is_some()
            {
                return Err(
                    "`--wf` cannot be combined with --req/--legacy/--migrated/--spec; \
                     pass either a single `.wf` file via --wf, or the four flags, not both"
                        .to_string(),
                );
            }
            let wf_src = read_file_or_stdin(wf_path)?;
            OracleEntry {
                label: wf_path.to_string(),
                result: oracle_report_from_wf(&wf_src),
            }
        }
        None => {
            // Four-flag mode: all four are required.
            let (req, legacy, migrated, spec) = match (req_arg, legacy_arg, migrated_arg, spec_arg)
            {
                (Some(r), Some(l), Some(m), Some(s)) => (r, l, m, s),
                _ => {
                    return Err(
                        "expected --req, --legacy, --migrated and --spec (or --wf <path> for a \
                         single triple file); supply all four flags or use --wf"
                            .to_string(),
                    );
                }
            };
            // At most one input may read stdin.
            let stdin_count = [req, legacy, migrated, spec]
                .iter()
                .filter(|a| **a == "-")
                .count();
            if stdin_count > 1 {
                return Err(
                    "at most one of --req/--legacy/--migrated/--spec may be `-` (stdin); \
                     give a file path or hex:<…> for the others"
                        .to_string(),
                );
            }
            let req_bytes = read_bytes_arg(req)?;
            let legacy_bytes = read_bytes_arg(legacy)?;
            let migrated_bytes = read_bytes_arg(migrated)?;
            let spec_toml = read_file_or_stdin(spec)?;
            OracleEntry {
                label: "oracle".to_string(),
                result: oracle_report(&req_bytes, &legacy_bytes, &migrated_bytes, &spec_toml),
            }
        }
    };

    let (body, gate) = render_oracle_scan(&[entry]);
    Ok(CmdOutcome {
        stdout: body,
        code: ExitCode::from(gate.code()),
    })
}

/// Resolve one byte-input argument: `hex:<…>` decodes inline hex (whitespace
/// ignored); `-` reads raw bytes from stdin; anything else is a file path read
/// as raw bytes. ISO 8583 captures can be binary, so this never round-trips
/// through UTF-8 (unlike [`read_file_or_stdin`]).
fn read_bytes_arg(arg: &str) -> Result<Vec<u8>, String> {
    if let Some(hex) = arg.strip_prefix("hex:") {
        hex_to_bytes(hex)
    } else if arg == "-" {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("read stdin: {e}"))?;
        wf_obs::dump_buffer(tracing::Level::TRACE, "oracle.input", &buf);
        Ok(buf)
    } else {
        let buf = std::fs::read(arg).map_err(|e| format!("read file {arg:?}: {e}"))?;
        wf_obs::dump_buffer(tracing::Level::TRACE, "oracle.input", &buf);
        Ok(buf)
    }
}

/// Run the fixed-layout structural check, returning a [`CmdOutcome`] whose
/// exit code is the diff-style gate (0 / 1 / 2).
///
/// `--trace` and `--frame` are mutually exclusive and exactly one is
/// required; the layout TOML always comes via `--layout`. At most one of the
/// inputs may be `-` (stdin). A layout-parse failure or a frameless trace is
/// rendered by the lib entry point as an uncheckable body with exit 2.
fn run_layout_check(
    layout_arg: &str,
    trace_arg: Option<&str>,
    frame_arg: Option<&str>,
) -> Result<CmdOutcome, String> {
    let stdin_count = [Some(layout_arg), trace_arg, frame_arg]
        .iter()
        .filter(|a| **a == Some("-"))
        .count();
    if stdin_count > 1 {
        return Err(
            "at most one of --layout/--trace/--frame may be `-` (stdin); \
             give file paths for the others"
                .to_string(),
        );
    }
    let layout_toml = read_file_or_stdin(layout_arg)?;
    let (body, code) = match (trace_arg, frame_arg) {
        (Some(trace), None) => {
            // A trace may contain non-UTF-8 (GBK) log text — read raw bytes.
            let trace_bytes = read_bytes_arg(trace)?;
            layout_check_trace(&layout_toml, &trace_bytes)
        }
        (None, Some(frame)) => {
            let frame_bytes = read_bytes_arg(frame)?;
            layout_check_frame(&layout_toml, &frame_bytes)
        }
        _ => {
            return Err(
                "expected exactly one of --trace <file> (bcl-dump trace) or \
                 --frame <file|hex:…|-> (one raw frame)"
                    .to_string(),
            );
        }
    };
    Ok(CmdOutcome {
        stdout: body,
        code: ExitCode::from(code),
    })
}

fn run_build() -> Result<String, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("read stdin: {e}"))?;
    if buf.trim().is_empty() {
        return Err("empty stdin — pipe a JSON message description in (see --help)".to_string());
    }
    wf_obs::dump_buffer(tracing::Level::TRACE, "build.input", buf.as_bytes());
    build_from_json(&buf)
}
