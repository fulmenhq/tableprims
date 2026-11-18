// SPDX-License-Identifier: MIT OR Apache-2.0
//! The second read: writing the workbook.

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use rust_xlsxwriter::{Format, Workbook, XlsxError};
use tableprims::documents::RESERVED_SHEET;
use tableprims::table::LogicalType;

use crate::error::{ErrorKind, WriteError};
use crate::metadata;
use crate::plan::{pass, Outcome, Prepared, Representation, Scan};

/// Largest scale shown with a fixed number of decimal places.
const MAX_FORMAT_SCALE: u8 = 30;

fn backend(error: XlsxError) -> WriteError {
    match error {
        XlsxError::IoError(error) | XlsxError::TempFileError(_, error) => {
            WriteError::new(ErrorKind::Write(error))
        }
        other => {
            // An I/O failure wrapped by the archive writer is still an I/O
            // failure. Only its kind is kept.
            let mut source = std::error::Error::source(&other);
            while let Some(inner) = source {
                if let Some(io) = inner.downcast_ref::<std::io::Error>() {
                    return WriteError::new(ErrorKind::Write(io.kind().into()));
                }
                source = inner.source();
            }
            WriteError::new(ErrorKind::Backend(other.to_string()))
        }
    }
}

/// The display format of a native exact-number column. The scale shown is
/// a courtesy; the column record is the authority.
fn number_format(scan: &Scan, at: usize) -> Option<Format> {
    if scan.representations[at] != Representation::Native {
        return None;
    }
    let scale = match scan.plan.schema.columns()[at].logical_type() {
        LogicalType::Integer => 0,
        LogicalType::Decimal(declared) => {
            scan.plan.quantize[at].map_or(declared.scale(), |(target, _)| target.scale())
        }
        _ => return None,
    };
    if scale > MAX_FORMAT_SCALE {
        return None;
    }
    let pattern = if scale == 0 {
        String::from("0")
    } else {
        format!("0.{}", "0".repeat(usize::from(scale)))
    };
    Some(Format::new().set_num_format(pattern))
}

impl Scan {
    /// Reads the input a second time and writes the workbook to `sink`.
    ///
    /// `input` must yield the bytes [`Plan::scan`](crate::Plan::scan) read.
    /// The rows are validated again; a read that leads to a different
    /// outcome than the first is refused. A caller that needs byte
    /// identity compares digests of the two reads as well.
    ///
    /// Row data is staged in unnamed temporary files, in `staging` when it
    /// is given and in the system temporary directory otherwise. `sink`
    /// receives the workbook only after every row has been validated; on an
    /// error it may hold a partial workbook and must be discarded.
    ///
    /// `cancel` is checked before each row; setting it stops the write.
    ///
    /// # Errors
    ///
    /// Fails when the input cannot be read, differs from the first read, or
    /// the workbook cannot be written.
    pub fn write<R: BufRead, W: Write + Send>(
        &self,
        input: R,
        sink: W,
        staging: Option<&Path>,
        cancel: &AtomicBool,
    ) -> Result<Outcome, WriteError> {
        let columns = self.plan.schema.columns();
        let formats: Vec<Option<Format>> = (0..columns.len())
            .map(|at| number_format(self, at))
            .collect();
        let mut workbook = Workbook::new();
        if let Some(directory) = staging {
            workbook.set_tempdir(directory).map_err(backend)?;
        }
        let data = workbook
            .add_worksheet_with_constant_memory()
            .map_err(backend)?;
        data.set_name(&self.plan.sheet).map_err(backend)?;
        for (at, column) in columns.iter().enumerate() {
            if !column.label().is_empty() {
                data.write_string(0, at as u16, column.label())
                    .map_err(backend)?;
            }
        }
        let tally = pass(
            &self.plan,
            input,
            cancel,
            Some(&self.representations),
            |row, cells| {
                if row >= self.tally.rows {
                    return Err(WriteError::at_row(ErrorKind::InputChanged, row));
                }
                // The scan bounded the row count by the worksheet's capacity.
                let line = row as u32 + metadata::HEADER_ROWS;
                for (at, cell) in cells.into_iter().enumerate() {
                    let at16 = at as u16;
                    match cell {
                        Prepared::Blank => {}
                        Prepared::Boolean(flag) => {
                            data.write_boolean(line, at16, flag).map_err(backend)?;
                        }
                        Prepared::Text(text) => {
                            data.write_string(line, at16, text).map_err(backend)?;
                        }
                        Prepared::Number(number) => match &formats[at] {
                            Some(format) => {
                                data.write_number_with_format(line, at16, number, format)
                                    .map_err(backend)?;
                            }
                            None => {
                                data.write_number(line, at16, number).map_err(backend)?;
                            }
                        },
                    }
                }
                Ok(())
            },
        )
        .map_err(|error| match error.kind() {
            // The first read accepted these rows, so a refusal now means the
            // input is not the one that was validated.
            ErrorKind::Read(_)
            | ErrorKind::Write(_)
            | ErrorKind::Backend(_)
            | ErrorKind::Cancelled
            | ErrorKind::InputChanged => error,
            _ => match error.row() {
                Some(row) => WriteError::at_row(ErrorKind::InputChanged, row),
                None => WriteError::new(ErrorKind::InputChanged),
            },
        })?;
        if tally != self.tally {
            return Err(WriteError::new(ErrorKind::InputChanged));
        }
        let sheet = workbook
            .add_worksheet_with_constant_memory()
            .map_err(backend)?;
        sheet.set_name(RESERVED_SHEET).map_err(backend)?;
        sheet.set_hidden(true);
        for (line, record) in metadata::records(self).iter().enumerate() {
            for (at, field) in record.iter().enumerate() {
                if !field.is_empty() {
                    sheet
                        .write_string(line as u32, at as u16, field)
                        .map_err(backend)?;
                }
            }
        }
        workbook.save_to_writer(sink).map_err(backend)?;
        Ok(self.outcome())
    }
}
