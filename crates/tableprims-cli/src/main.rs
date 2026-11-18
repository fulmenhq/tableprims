// SPDX-License-Identifier: MIT OR Apache-2.0

//! `tablebolt`: thin command-line consumer of the `tableprims` library.
//!
//! Exit codes follow the Fulmen exit-code catalog:
//!
//! | Code | Meaning |
//! | ---- | ------- |
//! | 0    | success |
//! | 54   | output could not be written |
//! | 64   | command-line usage error |
//!
//! A broken pipe while printing is treated as success: the reader chose to stop.
//! Any other failure to write requested output, including help text, exits 54.

use std::io::{self, Write};
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use serde_json::json;

mod identity;

/// Success.
const EXIT_SUCCESS: u8 = 0;
/// Output could not be written (`EXIT_FILE_WRITE_ERROR`).
const EXIT_FILE_WRITE_ERROR: u8 = 54;
/// Command-line usage error (`EXIT_USAGE`).
const EXIT_USAGE: u8 = 64;

#[derive(Debug, Parser)]
#[command(
    name = identity::BINARY_NAME,
    about = identity::DESCRIPTION,
    disable_version_flag = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print the tablebolt and tableprims versions.
    Version {
        /// Emit a JSON object instead of text.
        #[arg(long)]
        json: bool,
    },
    /// List the conversions this build implements.
    Capabilities {
        /// Emit a JSON object instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => return report_parse_error(&err),
    };

    let rendered = match cli.command {
        Command::Version { json } => version(json),
        Command::Capabilities { json } => capabilities(json),
    };

    finish(&rendered)
}

/// Help requests are output and succeed; every other parse failure is a
/// usage error reported on stderr.
fn report_parse_error(err: &clap::Error) -> ExitCode {
    let rendered = err.render().to_string();
    if matches!(err.kind(), ErrorKind::DisplayHelp) {
        return finish(&rendered);
    }
    diagnostic(&rendered);
    ExitCode::from(EXIT_USAGE)
}

/// Writes a diagnostic to stderr on a best-effort basis. The exit code carries
/// the outcome, so an unwritable stderr must never change it.
fn diagnostic(text: &str) {
    let _ = io::stderr().lock().write_all(text.as_bytes());
}

/// Writes requested output to stdout and maps the outcome to an exit code.
fn finish(rendered: &str) -> ExitCode {
    match write_stdout(rendered) {
        Ok(()) => ExitCode::from(EXIT_SUCCESS),
        Err(err) => {
            diagnostic(&format!(
                "{}: cannot write output: {err}\n",
                identity::BINARY_NAME
            ));
            ExitCode::from(EXIT_FILE_WRITE_ERROR)
        }
    }
}

fn version(as_json: bool) -> String {
    if as_json {
        let value = json!({
            "name": identity::BINARY_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "tableprims_version": tableprims::VERSION,
            "vendor": identity::VENDOR,
            "env_prefix": identity::ENV_PREFIX,
        });
        format!("{value}\n")
    } else {
        format!(
            "{} {} (tableprims {})\n",
            identity::BINARY_NAME,
            env!("CARGO_PKG_VERSION"),
            tableprims::VERSION
        )
    }
}

fn capabilities(as_json: bool) -> String {
    let conversions = tableprims::conversions();
    if as_json {
        let list: Vec<_> = conversions
            .iter()
            .map(|c| json!({ "source": c.source, "destination": c.destination }))
            .collect();
        let value = json!({
            "name": identity::BINARY_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "conversions": list,
        });
        format!("{value}\n")
    } else if conversions.is_empty() {
        "No conversions are implemented in this build.\n".to_owned()
    } else {
        conversions
            .iter()
            .map(|c| format!("{} -> {}\n", c.source, c.destination))
            .collect()
    }
}

fn write_stdout(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}
