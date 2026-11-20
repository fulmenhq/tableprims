// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fault controls for the read-back check.
//!
//! [`read_back`] compares a workbook with what a reader expects of it: the
//! column labels in order, every data cell, and the records of the hidden
//! `_tableprims` worksheet. Each test here makes a workbook with one fault
//! and shows that the check reports it: a dropped row, reordered columns,
//! missing metadata, a truncated file. A dropped row and reordered columns
//! are made by the writer itself, from input that differs in that way.
//! Missing metadata and truncation happen to a workbook after it is
//! written, so they are applied to the bytes the writer produced.
//!
//! The expectations below are written by hand from the input rows.

use std::collections::BTreeSet;
use std::io::{Cursor, Read, Write};
use std::sync::atomic::AtomicBool;

use calamine::{Data, Reader, SheetVisible, Xlsx};
use tableprims::binary64::same_decimal_value;
use tableprims::documents::{parse_policy, parse_schema};
use tableprims_xlsx::Plan;

/// One column as a reader expects it: identifier, label and logical type.
type Column = (&'static str, &'static str, &'static str);

const COLUMNS: [Column; 4] = [
    ("invoice", "Invoice", "string"),
    ("qty", "Qty", "integer"),
    ("amount", "Amount", "decimal"),
    ("paid", "Paid", "boolean"),
];

/// The schema for [`COLUMNS`] in the order given.
fn schema(columns: &[Column]) -> String {
    let declared: Vec<String> = columns
        .iter()
        .map(|(id, label, kind)| {
            let extra = if *kind == "decimal" {
                r#","precision":12,"scale":2"#
            } else {
                ""
            };
            format!(r#"{{"id":"{id}","label":"{label}","type":"{kind}"{extra},"nullable":true}}"#)
        })
        .collect();
    format!(
        r#"{{"format":"tabular-exchange.schema/0","columns":[{}]}}"#,
        declared.join(",")
    )
}

const POLICY: &str = r#"{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact","extensions":{"tableprims":{"sheet":"Data"}}}"#;

/// The input rows as JSON element tokens, in [`COLUMNS`] order. Each row is
/// written as one line `[a,b,c,d]`.
const LINES: [[&str; 4]; 3] = [
    [r#""INV-1""#, r#""3""#, r#""12.34""#, "true"],
    [r#""""#, r#""7""#, r#""0.50""#, "null"],
    [r#""INV-3""#, r#""-2""#, "null", "false"],
];

/// What a reader expects of each data cell, in [`COLUMNS`] order. `None` is
/// a null. Numbers are decimal text, compared by value.
const EXPECTED: [[Option<&str>; 4]; 3] = [
    [Some("INV-1"), Some("3"), Some("12.34"), Some("true")],
    [Some(""), Some("7"), Some("0.50"), None],
    [Some("INV-3"), Some("-2"), None, Some("false")],
];

fn write(schema: &str, rows: &str) -> Vec<u8> {
    let never = AtomicBool::new(false);
    let plan = Plan::new(
        &parse_schema(schema).unwrap(),
        &parse_policy(POLICY).unwrap(),
    )
    .unwrap();
    let scan = plan.scan(rows.as_bytes(), &never).unwrap();
    let mut bytes = Vec::new();
    scan.write(rows.as_bytes(), &mut bytes, None, &never)
        .unwrap();
    bytes
}

fn lines(rows: &[[&str; 4]]) -> String {
    rows.iter()
        .map(|fields| format!("[{}]\n", fields.join(",")))
        .collect()
}

/// A data cell as stored.
#[derive(Debug)]
enum Cell {
    Blank,
    Text(String),
    Bool(bool),
    /// The numeric token exactly as written in the worksheet XML.
    Number(String),
    /// Anything else, described for the report.
    Other(#[allow(dead_code)] String),
}

/// The numeric token of the cell at `reference` in the worksheet XML, when
/// the cell is neither a string nor a Boolean.
fn numeric_token(sheet: &str, reference: &str) -> Option<String> {
    let start = sheet.find(&format!("<c r=\"{reference}\""))?;
    let cell = &sheet[start..];
    let open = &cell[..cell.find('>')?];
    if open.ends_with('/') || open.contains(" t=") {
        return None;
    }
    let cell = &cell[..cell.find("</c>")?];
    Some(cell[cell.find("<v>")? + 3..cell.find("</v>")?].to_owned())
}

fn reference(row: u32, column: u32) -> String {
    let letter = char::from(b'A' + u8::try_from(column).unwrap());
    format!("{letter}{}", row + 1)
}

fn read_member(bytes: &[u8], name: &str) -> Option<String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut text = String::new();
    archive.by_name(name).ok()?.read_to_string(&mut text).ok()?;
    Some(text)
}

/// Text of a metadata or label cell; blank is `""`.
fn text(cell: Option<&Data>) -> String {
    match cell {
        None | Some(Data::Empty) => String::new(),
        Some(Data::String(text)) => text.clone(),
        Some(other) => format!("{other:?}"),
    }
}

/// Compares a workbook with what a reader expects of it and lists every
/// difference. An empty list means the workbook holds `expected` under
/// `columns`, in that order, with metadata that agrees.
fn read_back(bytes: &[u8], columns: &[Column], expected: &[[Option<&str>; 4]]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut workbook: Xlsx<_> = match Xlsx::new(Cursor::new(bytes)) {
        Ok(workbook) => workbook,
        Err(error) => return vec![format!("unreadable: {error}")],
    };
    let sheets = workbook.sheets_metadata().to_vec();
    let names: Vec<_> = sheets.iter().map(|s| s.name.as_str()).collect();
    if names.first() != Some(&"Data") || sheets[0].visible != SheetVisible::Visible {
        problems.push(format!("worksheets: {names:?}"));
    }
    match sheets.iter().find(|s| s.name == "_tableprims") {
        None => return [problems, vec!["no _tableprims worksheet".to_owned()]].concat(),
        Some(meta) if meta.visible != SheetVisible::Hidden => {
            problems.push("_tableprims is not hidden".to_owned());
        }
        Some(_) => {}
    }

    // The records of the hidden worksheet.
    let meta = match workbook.worksheet_range("_tableprims") {
        Ok(range) => range,
        Err(error) => return [problems, vec![format!("_tableprims: {error}")]].concat(),
    };
    let records: Vec<Vec<String>> = meta
        .rows()
        .map(|row| {
            let mut fields: Vec<String> = row.iter().map(|cell| text(Some(cell))).collect();
            while fields.last().is_some_and(String::is_empty) {
                fields.pop();
            }
            fields
        })
        .collect();
    let (rows, width) = (expected.len().to_string(), columns.len().to_string());
    let want: [Vec<&str>; 2] = [
        vec!["format", "1", "tableprims", "xlsx-write/r1"],
        vec!["sheet", "Data", "1", &rows, &width],
    ];
    for (index, want) in want.iter().enumerate() {
        if records
            .get(index)
            .map(|r| r.iter().map(String::as_str).collect::<Vec<_>>())
            .as_ref()
            != Some(want)
        {
            problems.push(format!(
                "record {index}: {:?}, expected {want:?}",
                records.get(index)
            ));
        }
    }
    for (index, (id, _, kind)) in columns.iter().enumerate() {
        let record = records.get(2 + index).cloned().unwrap_or_default();
        let at = |field: usize| record.get(field).map_or("", String::as_str);
        if (at(0), at(1), at(2), at(3)) != ("column", index.to_string().as_str(), *id, *kind) {
            problems.push(format!(
                "column record {index}: {record:?}, expected {id} {kind}"
            ));
        }
    }
    let mut empty = BTreeSet::new();
    let mut seen = 2 + columns.len();
    while records
        .get(seen)
        .is_some_and(|r| r.first().is_some_and(|kind| kind == "empty"))
    {
        let column: usize = records[seen][1].parse().unwrap();
        for range in records[seen][2..].join(",").split(',') {
            let (first, last) = range.split_once('-').unwrap();
            for row in first.parse::<usize>().unwrap()..=last.parse().unwrap() {
                empty.insert((row, column));
            }
        }
        seen += 1;
    }
    let end = vec!["end".to_owned(), seen.to_string()];
    if records.get(seen) != Some(&end) || records.len() != seen + 1 {
        problems.push(format!(
            "records after {seen}: {:?}, expected {end:?}",
            &records[seen.min(records.len())..]
        ));
    }

    // The labels and the data cells.
    let data = match workbook.worksheet_range("Data") {
        Ok(range) => range,
        Err(error) => return [problems, vec![format!("Data: {error}")]].concat(),
    };
    let Some(xml) = read_member(bytes, "xl/worksheets/sheet1.xml") else {
        return [problems, vec!["no worksheet XML for Data".to_owned()]].concat();
    };
    for (index, (_, label, _)) in columns.iter().enumerate() {
        let got = text(data.get_value((0, u32::try_from(index).unwrap())));
        if got != *label {
            problems.push(format!("label {index}: {got:?}, expected {label:?}"));
        }
    }
    if let Some((last, _)) = data.end() {
        if usize::try_from(last).unwrap() > expected.len() {
            problems.push(format!(
                "rows beyond {}: last row is {last}",
                expected.len()
            ));
        }
    }
    for (row, cells) in expected.iter().enumerate() {
        for (column, want) in cells.iter().enumerate() {
            let kind = columns[column].2;
            let at = (
                u32::try_from(row).unwrap() + 1,
                u32::try_from(column).unwrap(),
            );
            let got = match data.get_value(at) {
                None | Some(Data::Empty) => Cell::Blank,
                Some(Data::String(text)) => Cell::Text(text.clone()),
                Some(Data::Bool(value)) => Cell::Bool(*value),
                Some(Data::Float(_) | Data::Int(_)) => numeric_token(&xml, &reference(at.0, at.1))
                    .map_or_else(|| Cell::Other("no numeric token".to_owned()), Cell::Number),
                Some(other) => Cell::Other(format!("{other:?}")),
            };
            let agrees = match (&got, want) {
                (Cell::Blank, None) => !empty.contains(&(row, column)),
                (Cell::Blank, Some("")) => empty.contains(&(row, column)),
                (Cell::Text(text), Some(want)) => kind == "string" && text == want,
                (Cell::Bool(value), Some(want)) => kind == "boolean" && value.to_string() == *want,
                (Cell::Number(token), Some(want)) => {
                    matches!(kind, "integer" | "decimal") && same_decimal_value(token, want)
                }
                _ => false,
            };
            if !agrees {
                problems.push(format!(
                    "row {row} column {column}: {got:?}, expected {want:?}"
                ));
            }
        }
    }
    problems
}

fn expected_rows() -> Vec<[Option<&'static str>; 4]> {
    EXPECTED.to_vec()
}

fn all_lines() -> Vec<[&'static str; 4]> {
    LINES.to_vec()
}

/// Control: the workbook written from the input agrees, so a reported
/// difference below comes from the fault.
#[test]
fn the_workbook_as_written_reads_back() {
    let bytes = write(&schema(&COLUMNS), &lines(&all_lines()));
    assert_eq!(
        read_back(&bytes, &COLUMNS, &expected_rows()),
        Vec::<String>::new()
    );
}

#[test]
fn a_changed_digit_is_reported() {
    let mut input = all_lines();
    input[0][2] = r#""12.35""#;
    let bytes = write(&schema(&COLUMNS), &lines(&input));
    assert_eq!(
        read_back(&bytes, &COLUMNS, &expected_rows()),
        ["row 0 column 2: Number(\"12.35\"), expected Some(\"12.34\")"]
    );
}

#[test]
fn a_dropped_row_is_reported() {
    for dropped in 0..LINES.len() {
        let mut input = all_lines();
        input.remove(dropped);
        let bytes = write(&schema(&COLUMNS), &lines(&input));
        let problems = read_back(&bytes, &COLUMNS, &expected_rows());
        assert!(
            problems.iter().any(|p| p.starts_with("record 1:")),
            "row {dropped}: {problems:?}"
        );
        // The data cells alone also show it.
        assert!(
            problems.iter().any(|p| p.starts_with("row ")),
            "row {dropped}: {problems:?}"
        );
    }
}

#[test]
fn reordered_columns_are_reported() {
    for (a, b) in [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)] {
        let mut columns = COLUMNS;
        columns.swap(a, b);
        let mut input = all_lines();
        for fields in &mut input {
            fields.swap(a, b);
        }
        let bytes = write(&schema(&columns), &lines(&input));
        // The workbook is a faithful copy of the swapped input.
        let mut swapped = expected_rows();
        for row in &mut swapped {
            row.swap(a, b);
        }
        assert_eq!(read_back(&bytes, &columns, &swapped), Vec::<String>::new());
        let problems = read_back(&bytes, &COLUMNS, &expected_rows());
        for kind in ["label ", "column record ", "row "] {
            assert!(
                problems.iter().any(|p| p.starts_with(kind)),
                "{a}<->{b} {kind}: {problems:?}"
            );
        }
    }
}

/// The workbook with the `_tableprims` worksheet no longer listed, as a
/// tool that keeps only visible worksheets would leave it. Every other
/// member is copied unchanged.
fn without_metadata(bytes: &[u8]) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let name = archive
            .by_index_raw(index)
            .unwrap()
            .name()
            .unwrap()
            .into_owned();
        if name == "xl/workbook.xml" {
            let mut xml = String::new();
            archive
                .by_index(index)
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            let start = xml.find("<sheet name=\"_tableprims\"").unwrap();
            let end = start + xml[start..].find("/>").unwrap() + 2;
            xml.replace_range(start..end, "");
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            out.raw_copy_file(archive.by_index_raw(index).unwrap())
                .unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

#[test]
fn missing_metadata_is_reported() {
    let bytes = write(&schema(&COLUMNS), &lines(&all_lines()));
    let stripped = without_metadata(&bytes);
    // The data worksheet still reads, and on its own looks complete.
    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(&stripped[..])).unwrap();
    assert_eq!(workbook.sheet_names(), ["Data"]);
    assert!(workbook.worksheet_range("Data").is_ok());
    assert_eq!(
        read_back(&stripped, &COLUMNS, &expected_rows()),
        ["no _tableprims worksheet"]
    );
}

#[test]
fn a_truncated_workbook_is_reported() {
    let bytes = write(&schema(&COLUMNS), &lines(&all_lines()));
    for keep in [0, 1, bytes.len() / 2, bytes.len() - 22, bytes.len() - 1] {
        let problems = read_back(&bytes[..keep], &COLUMNS, &expected_rows());
        assert!(
            problems.len() == 1 && problems[0].starts_with("unreadable: "),
            "{keep} of {}: {problems:?}",
            bytes.len()
        );
    }
}
