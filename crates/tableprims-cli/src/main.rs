// SPDX-License-Identifier: MIT OR Apache-2.0

//! `tablebolt`: thin command-line consumer of the `tableprims` library.
//!
//! Exit codes follow the Fulmen exit-code catalog:
//!
//! | Code | Meaning |
//! | ---- | ------- |
//! | 0    | success |
//! | 1    | internal failure |
//! | 33   | the system ran out of storage or memory |
//! | 50   | permission denied |
//! | 51   | an input file does not exist |
//! | 53   | an input could not be read, or changed while it was read |
//! | 54   | output could not be written, or its name already exists |
//! | 60   | a value does not fit the schema, or a configured limit was exceeded |
//! | 61   | malformed input |
//! | 62   | a type or value the destination cannot represent under the policy |
//! | 64   | command-line usage error |
//!
//! A broken pipe while printing is treated as success: the reader chose to stop.
//! Any other failure to write requested output, including help text, exits 54.
//!
//! A conversion prints nothing to stdout. Its outcome is the exit code and
//! the result file.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};
use serde_json::json;

mod digest;
mod identity;
mod publish;
mod xlsx_write;

/// Success.
const EXIT_SUCCESS: u8 = 0;
/// Output could not be written (`EXIT_FILE_WRITE_ERROR`).
const EXIT_FILE_WRITE_ERROR: u8 = xlsx_write::EXIT_FILE_WRITE_ERROR;
/// Command-line usage error (`EXIT_USAGE`).
const EXIT_USAGE: u8 = xlsx_write::EXIT_USAGE;

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
    /// Convert to and from XLSX workbooks.
    Xlsx {
        #[command(subcommand)]
        command: XlsxCommand,
    },
}

#[derive(Debug, Subcommand)]
enum XlsxCommand {
    /// Write rows to a new workbook and record the outcome in a result file.
    Write {
        /// Rows: one JSON array of null, booleans and strings per line.
        #[arg(long, value_name = "FILE")]
        input: PathBuf,
        /// Schema document.
        #[arg(long, value_name = "FILE")]
        schema: PathBuf,
        /// Policy document.
        #[arg(long, value_name = "FILE")]
        policy: PathBuf,
        /// Workbook to create. An existing file is never replaced.
        #[arg(long, value_name = "FILE")]
        output: PathBuf,
        /// Result file to create. An existing file is never replaced.
        #[arg(long, value_name = "FILE")]
        result: PathBuf,
        /// Identifier copied into the result file. Generated when absent.
        #[arg(long, value_name = "ID", value_parser = xlsx_write::parse_run_id)]
        run_id: Option<String>,
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
        Command::Xlsx {
            command:
                XlsxCommand::Write {
                    input,
                    schema,
                    policy,
                    output,
                    result,
                    run_id,
                },
        } => {
            let files = xlsx_write::Files {
                input,
                schema,
                policy,
                output,
                result,
            };
            let (code, line) = xlsx_write::run(&files, run_id);
            if let Some(line) = line {
                diagnostic(&format!("{}: {line}\n", identity::BINARY_NAME));
            }
            return ExitCode::from(code);
        }
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

/// The conversions this build offers: the library's and the adapters'.
fn conversions() -> Vec<(&'static str, &'static str)> {
    let mut list: Vec<_> = tableprims::conversions()
        .iter()
        .map(|c| (c.source, c.destination))
        .collect();
    list.push(("ndjson", "xlsx"));
    list
}

fn capabilities(as_json: bool) -> String {
    let conversions = conversions();
    if as_json {
        let list: Vec<_> = conversions
            .iter()
            .map(|(source, destination)| json!({ "source": source, "destination": destination }))
            .collect();
        let value = json!({
            "name": identity::BINARY_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "conversions": list,
        });
        format!("{value}\n")
    } else {
        conversions
            .iter()
            .map(|(source, destination)| format!("{source} -> {destination}\n"))
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
