// SPDX-License-Identifier: MIT OR Apache-2.0
//! Fault controls for the read-back check.
//!
//! [`read_back`] compares a workbook with what a reader expects of it: the
//! worksheets, the column labels in order, every data cell by type and exact
//! value, and every record of the hidden `_tableprims` worksheet. Each test
//! here makes a workbook with one fault and shows that the check reports it:
//! a changed digit, a dropped or added row, reordered or added columns, a
//! changed representation, missing or altered metadata, a truncated file.
//! Faults in the data are made by the writer itself, from input, schema or
//! policy that differs in that way. Missing or altered metadata and
//! truncation happen to a workbook after it is written, so they are applied
//! to the bytes the writer produced.
//!
//! The expectations below are written by hand from the input rows. Numbers
//! are compared with a decimal reader written here, not the library's.

use std::collections::BTreeSet;
use std::io::{Cursor, Read, Write};
use std::sync::atomic::AtomicBool;

use calamine::{Data, Reader, SheetVisible, Xlsx};
use tableprims::documents::{parse_policy, parse_schema};
use tableprims_xlsx::Plan;

/// One column as a reader expects it: identifier, label, logical type and
/// representation.
type Column = (&'static str, &'static str, &'static str, &'static str);

const COLUMNS: [Column; 4] = [
    ("invoice", "Invoice", "string", "text"),
    ("qty", "Qty", "integer", "native"),
    ("amount", "Amount", "decimal", "native"),
    ("paid", "Paid", "boolean", "native"),
];

/// The schema for `columns` in the order given.
fn schema(columns: &[Column]) -> String {
    let declared: Vec<String> = columns
        .iter()
        .map(|(id, label, kind, _)| {
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

fn policy(exact: &str) -> String {
    format!(
        r#"{{"format":"tabular-exchange.policy/0","exact_numbers":"{exact}","extensions":{{"tableprims":{{"sheet":"Data"}}}}}}"#
    )
}

/// The input rows as JSON element tokens, in [`COLUMNS`] order. Each row is
/// written as one line `[a,b,c,d]`.
const LINES: [[&str; 4]; 3] = [
    [r#""INV-1""#, r#""3""#, r#""12.34""#, "true"],
    [r#""""#, r#""7""#, r#""0.50""#, "null"],
    [r#""INV-3""#, r#""-2""#, "null", "false"],
];

/// One expected row: a reader's value for each cell, in column order.
/// `None` is a null. Numbers are decimal text, compared by value.
type Row = Vec<Option<&'static str>>;

/// What a reader expects of each data cell, in [`COLUMNS`] order.
fn expected_rows() -> Vec<Row> {
    vec![
        vec![Some("INV-1"), Some("3"), Some("12.34"), Some("true")],
        vec![Some(""), Some("7"), Some("0.50"), None],
        vec![Some("INV-3"), Some("-2"), None, Some("false")],
    ]
}

fn all_lines() -> Vec<Vec<&'static str>> {
    LINES.iter().map(|line| line.to_vec()).collect()
}

fn write_with(schema: &str, policy: &str, rows: &str) -> Vec<u8> {
    let never = AtomicBool::new(false);
    let plan = Plan::new(
        &parse_schema(schema).unwrap(),
        &parse_policy(policy).unwrap(),
    )
    .unwrap();
    let scan = plan.scan(rows.as_bytes(), &never).unwrap();
    let mut bytes = Vec::new();
    scan.write(rows.as_bytes(), &mut bytes, None, &never)
        .unwrap();
    bytes
}

fn write(schema: &str, rows: &str) -> Vec<u8> {
    write_with(schema, &policy("require-exact"), rows)
}

fn lines(rows: &[Vec<&str>]) -> String {
    rows.iter()
        .map(|fields| format!("[{}]\n", fields.join(",")))
        .collect()
}

fn as_written() -> Vec<u8> {
    write(&schema(&COLUMNS), &lines(&all_lines()))
}

/// A decimal number as sign, significant digits and power of ten, with no
/// leading or trailing zero digit; zero is `(false, "", 0)`. Accepts an
/// optional minus sign, digits, an optional fraction and an optional
/// exponent, which is what a worksheet's numeric token can hold.
fn decimal(text: &str) -> Option<(bool, String, i64)> {
    let (negative, text) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mantissa, exponent) = match text.find(['e', 'E']) {
        Some(at) => (&text[..at], text[at + 1..].parse::<i64>().ok()?),
        None => (text, 0),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((_, "")) => return None,
        Some(parts) => parts,
        None => (mantissa, ""),
    };
    if whole.is_empty() || !(whole.chars().chain(fraction.chars())).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let mut power = exponent - i64::try_from(fraction.len()).ok()?;
    let trimmed = digits.trim_start_matches('0');
    let significant = trimmed.trim_end_matches('0');
    power += i64::try_from(trimmed.len() - significant.len()).ok()?;
    if significant.is_empty() {
        return Some((false, String::new(), 0));
    }
    Some((negative, significant.to_owned(), power))
}

fn same_value(token: &str, expected: &str) -> bool {
    decimal(token).is_some() && decimal(token) == decimal(expected)
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

/// The `column` record a reader expects for column `index`.
fn column_record(index: usize, (id, _, kind, representation): &Column) -> Vec<String> {
    let (precision, scale) = if *kind == "decimal" {
        ("12", "2")
    } else {
        ("", "")
    };
    let fields = [
        "column",
        &index.to_string(),
        id,
        kind,
        precision,
        scale,
        "true",
        "",
        representation,
    ];
    fields.iter().map(|field| (*field).to_owned()).collect()
}

/// The (row, column) cells that the `empty` records from `records[first..]`
/// list, and the index of the first record after them.
fn empty_cells(
    records: &[Vec<String>],
    first: usize,
    width: usize,
    problems: &mut Vec<String>,
) -> (BTreeSet<(usize, usize)>, usize) {
    let mut cells = BTreeSet::new();
    let mut at = first;
    while records
        .get(at)
        .is_some_and(|r| r.first().is_some_and(|kind| kind == "empty"))
    {
        let record = &records[at];
        let column = record.get(1).and_then(|c| c.parse::<usize>().ok());
        let ranges: Option<Vec<(usize, usize)>> = record
            .get(2..)
            .filter(|fields| !fields.is_empty())
            .map(|fields| fields.join(","))
            .and_then(|joined| {
                joined
                    .split(',')
                    .map(|range| {
                        let (a, b) = range.split_once('-')?;
                        Some((a.parse().ok()?, b.parse().ok()?))
                    })
                    .collect()
            });
        match (column, ranges) {
            (Some(column), Some(ranges))
                if column < width && ranges.iter().all(|(a, b)| a <= b) =>
            {
                for (a, b) in ranges {
                    cells.extend((a..=b).map(|row| (row, column)));
                }
            }
            _ => problems.push(format!("record {at}: malformed empty record {record:?}")),
        }
        at += 1;
    }
    (cells, at)
}

/// Compares a workbook with what a reader expects of it and lists every
/// difference. An empty list means the workbook holds `expected` under
/// `columns`, in that order, with metadata that agrees.
#[allow(clippy::too_many_lines)]
fn read_back(bytes: &[u8], columns: &[Column], expected: &[Row]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut workbook: Xlsx<_> = match Xlsx::new(Cursor::new(bytes)) {
        Ok(workbook) => workbook,
        Err(error) => return vec![format!("unreadable: {error}")],
    };
    let sheets: Vec<_> = workbook
        .sheets_metadata()
        .iter()
        .map(|s| (s.name.clone(), s.visible))
        .collect();
    if !sheets.iter().any(|(name, _)| name == "_tableprims") {
        return vec!["no _tableprims worksheet".to_owned()];
    }
    let want = [
        ("Data".to_owned(), SheetVisible::Visible),
        ("_tableprims".to_owned(), SheetVisible::Hidden),
    ];
    if sheets != want {
        problems.push(format!("worksheets: {sheets:?}"));
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
    let mut want: Vec<Vec<String>> = [
        vec!["format", "1", "tableprims", "xlsx-write/r1"],
        vec!["sheet", "Data", "1", &rows, &width],
    ]
    .iter()
    .map(|record| record.iter().map(|field| (*field).to_owned()).collect())
    .collect();
    want.extend(columns.iter().enumerate().map(|(i, c)| column_record(i, c)));
    for (index, want) in want.iter().enumerate() {
        let got = records.get(index);
        if got != Some(want) {
            let what = if index < 2 { "record" } else { "column record" };
            problems.push(format!("{what} {index}: {got:?}, expected {want:?}"));
        }
    }
    let (empty, end_at) = empty_cells(&records, want.len(), columns.len(), &mut problems);
    let end = vec!["end".to_owned(), end_at.to_string()];
    if records.get(end_at) != Some(&end) || records.len() != end_at + 1 {
        problems.push(format!(
            "end record: {:?}, expected {end:?} and nothing after",
            &records[end_at.min(records.len())..]
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
    for (index, (_, label, _, _)) in columns.iter().enumerate() {
        let got = text(data.get_value((0, u32::try_from(index).unwrap())));
        if got != *label {
            problems.push(format!("label {index}: {got:?}, expected {label:?}"));
        }
    }
    if let Some((last_row, last_column)) = data.end() {
        if usize::try_from(last_row).unwrap() > expected.len() {
            problems.push(format!(
                "rows beyond {}: last is {last_row}",
                expected.len()
            ));
        }
        if usize::try_from(last_column).unwrap() >= columns.len() {
            problems.push(format!(
                "columns beyond {}: last is {last_column}",
                columns.len()
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
            let listed = empty.contains(&(row, column));
            let agrees = match (&got, want) {
                (Cell::Blank, None) => !listed,
                (Cell::Blank, Some("")) => listed,
                (_, _) if listed => false,
                (Cell::Text(text), Some(want)) => kind == "string" && text == want,
                (Cell::Bool(value), Some(want)) => kind == "boolean" && value.to_string() == *want,
                (Cell::Number(token), Some(want)) => {
                    matches!(kind, "integer" | "decimal") && same_value(token, want)
                }
                _ => false,
            };
            if !agrees {
                let note = if listed { " (listed empty)" } else { "" };
                problems.push(format!(
                    "row {row} column {column}: {got:?}{note}, expected {want:?}"
                ));
            }
        }
    }
    problems
}

/// Requires each prefix in `kinds` to start at least one of `problems`.
fn assert_reports(problems: &[String], kinds: &[&str], what: &str) {
    for kind in kinds {
        assert!(
            problems.iter().any(|p| p.starts_with(kind)),
            "{what}: no {kind:?} in {problems:?}"
        );
    }
}

/// The workbook with `edit` applied to the text of one member. Every other
/// member is copied unchanged.
fn rewrite(bytes: &[u8], member: &str, edit: impl FnOnce(&mut String)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut edit = Some(edit);
    for index in 0..archive.len() {
        let name = archive
            .by_index_raw(index)
            .unwrap()
            .name()
            .unwrap()
            .into_owned();
        if name == member {
            let mut xml = String::new();
            archive
                .by_index(index)
                .unwrap()
                .read_to_string(&mut xml)
                .unwrap();
            let before = xml.clone();
            (edit.take().unwrap())(&mut xml);
            assert_ne!(xml, before, "the edit changed nothing");
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            out.raw_copy_file(archive.by_index_raw(index).unwrap())
                .unwrap();
        }
    }
    assert!(edit.is_none(), "no member {member}");
    out.finish().unwrap().into_inner()
}

/// Removes the element that starts with `open` and ends with the first
/// `close` after it.
fn remove_element(xml: &mut String, open: &str, close: &str) {
    let start = xml.find(open).unwrap();
    let end = start + xml[start..].find(close).unwrap() + close.len();
    xml.replace_range(start..end, "");
}

/// Control: the workbook written from the input agrees, so a reported
/// difference below comes from the fault.
#[test]
fn the_workbook_as_written_reads_back() {
    assert_eq!(
        read_back(&as_written(), &COLUMNS, &expected_rows()),
        Vec::<String>::new()
    );
}

/// Control for the decimal reader used by the read-back.
#[test]
fn the_decimal_reader_compares_values_not_text() {
    for (a, b) in [
        ("12.34", "12.340"),
        ("0.50", "0.5"),
        ("-2", "-2.00"),
        ("0", "-0.000"),
        ("1.5E2", "150"),
        ("1e-3", "0.001"),
        ("007", "7"),
    ] {
        assert!(same_value(a, b), "{a} {b}");
    }
    for (a, b) in [
        ("12.34", "12.35"),
        ("12.34", "1.234"),
        ("-2", "2"),
        ("1e3", "100"),
        ("", "0"),
        ("1.", "1"),
        ("x", "x"),
    ] {
        assert!(!same_value(a, b), "{a} {b}");
    }
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
        // The sheet record's count, and the data cells on their own.
        assert_reports(&problems, &["record 1:", "row "], &format!("row {dropped}"));
    }
}

#[test]
fn an_added_row_is_reported() {
    let mut input = all_lines();
    input.push(LINES[0].to_vec());
    let bytes = write(&schema(&COLUMNS), &lines(&input));
    assert_eq!(
        read_back(&bytes, &COLUMNS, &expected_rows()),
        [
            "record 1: Some([\"sheet\", \"Data\", \"1\", \"4\", \"4\"]), expected [\"sheet\", \"Data\", \"1\", \"3\", \"4\"]",
            "rows beyond 3: last is 4"
        ]
    );
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
        assert_reports(
            &problems,
            &["label ", "column record ", "row "],
            &format!("{a}<->{b}"),
        );
    }
}

#[test]
fn an_added_column_is_reported() {
    let mut columns = COLUMNS.to_vec();
    columns.push(("note", "Note", "string", "text"));
    let input: Vec<Vec<&str>> = all_lines()
        .into_iter()
        .map(|mut fields| {
            fields.push(r#""n""#);
            fields
        })
        .collect();
    let bytes = write(&schema(&columns), &lines(&input));
    let problems = read_back(&bytes, &COLUMNS, &expected_rows());
    assert_reports(
        &problems,
        &["record 1:", "end record:", "columns beyond 4"],
        "added column",
    );
}

/// The policy `text-always` stores the integer and decimal columns as text.
#[test]
fn a_changed_representation_is_reported() {
    let bytes = write_with(
        &schema(&COLUMNS),
        &policy("text-always"),
        &lines(&all_lines()),
    );
    let problems = read_back(&bytes, &COLUMNS, &expected_rows());
    assert_reports(
        &problems,
        &[
            "column record 3:",
            "column record 4:",
            "row 0 column 1: Text",
        ],
        "text-always",
    );
}

#[test]
fn missing_metadata_is_reported() {
    let bytes = as_written();
    // A tool that keeps only visible worksheets drops `_tableprims`.
    let stripped = rewrite(&bytes, "xl/workbook.xml", |xml| {
        remove_element(xml, "<sheet name=\"_tableprims\"", "/>");
    });
    // The data worksheet still reads, and on its own looks complete.
    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(&stripped[..])).unwrap();
    assert_eq!(workbook.sheet_names(), ["Data"]);
    assert!(workbook.worksheet_range("Data").is_ok());
    assert_eq!(
        read_back(&stripped, &COLUMNS, &expected_rows()),
        ["no _tableprims worksheet"]
    );
}

/// Without its `empty` record, the empty string in row 1 would read as a
/// null.
#[test]
fn a_missing_empty_record_is_reported() {
    let bytes = as_written();
    let sheet = read_member(&bytes, "xl/worksheets/sheet2.xml").unwrap();
    let at = sheet.find("<t>empty</t>").unwrap();
    let start = sheet[..at].rfind("<row ").unwrap();
    let open = sheet[start..].split('>').next().unwrap().to_owned();
    let stripped = rewrite(&bytes, "xl/worksheets/sheet2.xml", |xml| {
        remove_element(xml, &open, "</row>");
    });
    let problems = read_back(&stripped, &COLUMNS, &expected_rows());
    assert_reports(
        &problems,
        &["end record:", "row 1 column 0: Blank"],
        "empty record",
    );
}

/// An `empty` record that names a cell holding text.
#[test]
fn an_empty_record_for_a_written_cell_is_reported() {
    let bytes = as_written();
    let altered = rewrite(&bytes, "xl/worksheets/sheet2.xml", |xml| {
        *xml = xml.replacen("<t>1-1</t>", "<t>0-1</t>", 1);
    });
    assert_eq!(
        read_back(&altered, &COLUMNS, &expected_rows()),
        ["row 0 column 0: Text(\"INV-1\") (listed empty), expected Some(\"INV-1\")"]
    );
    for bad in ["1-0", "x", "1-"] {
        let altered = rewrite(&bytes, "xl/worksheets/sheet2.xml", |xml| {
            *xml = xml.replacen("<t>1-1</t>", &format!("<t>{bad}</t>"), 1);
        });
        let problems = read_back(&altered, &COLUMNS, &expected_rows());
        assert_reports(&problems, &["record 6: malformed empty record"], bad);
    }
    // A column index beyond the last column.
    let altered = rewrite(&bytes, "xl/worksheets/sheet2.xml", |xml| {
        let kind = xml.find("<t>empty</t>").unwrap();
        let index = kind + xml[kind..].find("<t>0</t>").unwrap();
        xml.replace_range(index..index + 8, "<t>4</t>");
    });
    let problems = read_back(&altered, &COLUMNS, &expected_rows());
    assert_reports(&problems, &["record 6: malformed empty record"], "column 4");
}

#[test]
fn visible_metadata_is_reported() {
    let altered = rewrite(&as_written(), "xl/workbook.xml", |xml| {
        *xml = xml.replacen(" state=\"hidden\"", "", 1);
    });
    assert_eq!(
        read_back(&altered, &COLUMNS, &expected_rows()),
        ["worksheets: [(\"Data\", Visible), (\"_tableprims\", Visible)]"]
    );
}

#[test]
fn a_truncated_workbook_is_reported() {
    let bytes = as_written();
    for keep in [0, 1, bytes.len() / 2, bytes.len() - 22, bytes.len() - 1] {
        let problems = read_back(&bytes[..keep], &COLUMNS, &expected_rows());
        assert!(
            problems.len() == 1 && problems[0].starts_with("unreadable: "),
            "{keep} of {}: {problems:?}",
            bytes.len()
        );
    }
}
