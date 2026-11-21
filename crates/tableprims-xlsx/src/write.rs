// SPDX-License-Identifier: MIT OR Apache-2.0
//! The second read: writing the workbook.

use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use rust_xlsxwriter::{Format, Workbook, XlsxError};
use tableprims::table::LogicalType;

use crate::error::{ErrorKind, WriteError};
use crate::metadata;
use crate::plan::{pass, Outcome, Prepared, Representation, Scan};

/// Largest scale shown with a fixed number of decimal places.
pub(crate) const MAX_FORMAT_SCALE: u8 = 30;

fn backend(error: XlsxError) -> WriteError {
    match error {
        XlsxError::IoError(error) => WriteError::new(ErrorKind::Write(error)),
        XlsxError::TempFileError(_, error) => {
            WriteError::new(ErrorKind::Staging(error.kind().into()))
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

/// What the pinned writer says when it cannot write a temporary file.
const STAGING_PANIC: &str = "Couldn't write to xml file";
/// How a panic on an unwrapped error begins.
const UNWRAP_PANIC: &str = "called `Result::unwrap()` on an `Err` value: ";

/// Turns a panic from the workbook writer into an error.
///
/// The writer reports a failure to write its temporary files by panicking
/// with the operating system error in the message. Such a panic becomes a
/// staging failure that keeps the error's kind. Any other panic is reported
/// as such and is not taken for an I/O failure.
fn panic_error(payload: &(dyn std::any::Any + Send)) -> WriteError {
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&'static str>().copied())
        .unwrap_or_default();
    let os_error = |text: &str| text.starts_with("Os {") || text.starts_with("Custom {");
    let staged = match message.strip_prefix(STAGING_PANIC) {
        Some(_) => true,
        None => message.strip_prefix(UNWRAP_PANIC).is_some_and(os_error),
    };
    if !staged {
        return WriteError::new(ErrorKind::BackendPanic);
    }
    let kind = [
        ("kind: StorageFull", std::io::ErrorKind::StorageFull),
        ("kind: QuotaExceeded", std::io::ErrorKind::QuotaExceeded),
        ("kind: FileTooLarge", std::io::ErrorKind::FileTooLarge),
        ("kind: OutOfMemory", std::io::ErrorKind::OutOfMemory),
        (
            "kind: PermissionDenied",
            std::io::ErrorKind::PermissionDenied,
        ),
    ]
    .into_iter()
    .find(|(name, _)| message.contains(name))
    .map_or(std::io::ErrorKind::Other, |(_, kind)| kind);
    WriteError::new(ErrorKind::Staging(kind.into()))
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
    /// `input` must yield the bytes [`Plan::scan`](crate::Plan::scan) read,
    /// and the caller is responsible for that: by an input that cannot
    /// change, or by comparing digests of the bytes of both reads before
    /// publishing what `sink` received. As a safeguard the rows are
    /// validated again and a read with a different outcome or different
    /// cells is refused; the cells are compared by a 64-bit fingerprint,
    /// which is not a digest.
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
    /// the workbook cannot be written. A failure of the writer's temporary
    /// files is caught as a panic, so it is reported only in a build that
    /// unwinds on panic.
    pub fn write<R: BufRead, W: Write + Send>(
        &self,
        input: R,
        sink: W,
        staging: Option<&Path>,
        cancel: &AtomicBool,
    ) -> Result<Outcome, WriteError> {
        // The writer panics rather than returning some failures. Nothing it
        // holds is used after a panic.
        let work =
            std::panic::AssertUnwindSafe(|| self.write_workbook(input, sink, staging, cancel));
        match std::panic::catch_unwind(work) {
            Ok(result) => result,
            Err(payload) => Err(panic_error(payload.as_ref())),
        }
    }

    fn write_workbook<R: BufRead, W: Write + Send>(
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
            | ErrorKind::Staging(_)
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
        sheet.set_name(metadata::WORKSHEET).map_err(backend)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Category;

    fn classify(message: &str) -> (Category, &'static str, bool) {
        let error = panic_error(&message.to_owned());
        (error.category(), error.rule(), error.is_system_exhaustion())
    }

    #[test]
    fn a_staging_panic_keeps_the_kind_of_its_cause() {
        for (cause, category, rule, exhausted) in [
            (
                "Os { code: 28, kind: StorageFull, message: \"No space left on device\" }",
                Category::ResourceLimit,
                "storage-full",
                true,
            ),
            (
                "Os { code: 27, kind: FileTooLarge, message: \"File too large\" }",
                Category::ResourceLimit,
                "file-too-large",
                true,
            ),
            (
                "Os { code: 5, kind: Uncategorized, message: \"Input/output error\" }",
                Category::IoFailure,
                "staging-failed",
                false,
            ),
        ] {
            let expected = (category, rule, exhausted);
            assert_eq!(classify(&format!("{STAGING_PANIC}: {cause}")), expected);
            assert_eq!(classify(&format!("{UNWRAP_PANIC}{cause}")), expected);
        }
    }

    #[test]
    fn any_other_panic_is_not_an_io_failure() {
        for message in [
            "",
            "index out of bounds: the len is 3 but the index is 7",
            "called `Result::unwrap()` on an `Err` value: ParseIntError { kind: StorageFull }",
            "called `Option::unwrap()` on a `None` value",
            "RwLock poisoned",
        ] {
            assert_eq!(
                classify(message),
                (Category::Internal, "writer-panic", false)
            );
        }
        let error = panic_error(&7_u8);
        assert_eq!(error.rule(), "writer-panic");
    }
}
