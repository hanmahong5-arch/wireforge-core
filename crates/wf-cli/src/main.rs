use std::io::{self, Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use wf_cli::{
    build_from_json, parse_to_json, parse_to_tree, swift_parse_to_json, swift_parse_to_tree,
};

/// Wireforge CLI for financial message codecs.
#[derive(Debug, Parser)]
#[command(name = "wf", version, about = "Wireforge — financial message codec CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Parse { hex, json } => run_parse(&hex, json),
        Commands::Build => run_build(),
        Commands::Swift(SwiftCommands::Parse { wire, json }) => run_swift_parse(&wire, json),
    };
    match result {
        Ok(output) => {
            // println! also flushes on newline; emit and exit.
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            let _ = writeln!(io::stderr(), "wf: {e}");
            ExitCode::FAILURE
        }
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
    if as_json {
        swift_parse_to_json(&wire_text)
    } else {
        swift_parse_to_tree(&wire_text)
    }
}

fn run_build() -> Result<String, String> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("read stdin: {e}"))?;
    if buf.trim().is_empty() {
        return Err("empty stdin — pipe a JSON message description in (see --help)".to_string());
    }
    build_from_json(&buf)
}
