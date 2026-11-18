// SPDX-License-Identifier: MIT OR Apache-2.0
//! Writes workbooks and reads them back with an independent reader and
//! from the raw worksheet XML.

use std::io::{Cursor, Read};
use std::sync::atomic::AtomicBool;

use calamine::{Data, Reader, SheetVisible, Xlsx};
use tableprims::binary64::same_decimal_value;
use tableprims::documents::{parse_policy, parse_schema};
use tableprims_xlsx::{Category, ErrorKind, Outcome, Plan, Representation, WriteError};

const SCHEMA: &str = r#"{"format":"tableprims.schema/0","columns":[
  {"id":"invoice","label":"Invoice","type":"string","nullable":true},
  {"id":"qty","label":"Qty","type":"integer","nullable":true},
  {"id":"amount","label":"Amount","type":"decimal","precision":12,"scale":4,"nullable":true},
  {"id":"paid","label":"","type":"boolean","nullable":true},
  {"id":"day","label":"Day","type":"date","nullable":true},
  {"id":"seen","label":"Seen","type":"local-timestamp","fraction":3,"nullable":true},
  {"id":"sent","label":"Sent","type":"offset-timestamp","fraction":0,"nullable":true}]}"#;

fn policy(exact: &str, extra: &str) -> String {
    format!(r#"{{"format":"tableprims.policy/0","exact_numbers":"{exact}","sheet":"Data"{extra}}}"#)
}

fn plan(schema: &str, policy: &str) -> Result<Plan, WriteError> {
    Plan::new(
        &parse_schema(schema).unwrap(),
        &parse_policy(policy).unwrap(),
    )
}

fn write(schema: &str, policy: &str, rows: &str) -> Result<(Vec<u8>, Outcome), WriteError> {
    let never = AtomicBool::new(false);
    let scan = plan(schema, policy)?.scan(rows.as_bytes(), &never)?;
    let mut bytes = Vec::new();
    let outcome = scan.write(rows.as_bytes(), &mut bytes, None, &never)?;
    Ok((bytes, outcome))
}

fn member(bytes: &[u8], name: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

/// The raw `<c …>…</c>` element of a cell, if the worksheet has one.
fn cell_xml<'a>(sheet: &'a str, reference: &str) -> Option<&'a str> {
    let start = sheet.find(&format!("<c r=\"{reference}\""))?;
    let rest = &sheet[start..];
    let close = rest.find('>')?;
    if rest[..close].ends_with('/') {
        return Some(&rest[..=close]);
    }
    Some(&rest[..rest.find("</c>")? + 4])
}

/// The numeric token of a cell that is neither a string nor a Boolean.
fn numeric_token<'a>(sheet: &'a str, reference: &str) -> Option<&'a str> {
    let cell = cell_xml(sheet, reference)?;
    if cell[..cell.find('>')?].contains(" t=") {
        return None;
    }
    Some(&cell[cell.find("<v>")? + 3..cell.find("</v>")?])
}

fn strings(bytes: &[u8], sheet: &str) -> Vec<Vec<Option<String>>> {
    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).unwrap();
    let range = workbook.worksheet_range(sheet).unwrap();
    range
        .rows()
        .map(|row| {
            row.iter()
                .map(|cell| match cell {
                    Data::Empty => None,
                    Data::String(text) => Some(text.clone()),
                    other => Some(format!("{other:?}")),
                })
                .collect()
        })
        .collect()
}

const ROWS: &str = concat!(
    r#"["INV-1","3","12.3400",true,"2026-11-18","2026-11-18T09:30:00.250","2026-11-18T09:30:00Z"]"#,
    "\n",
    r#"["","-7","123.4500",false,null,null,"2026-11-18T09:30:00+00:00"]"#,
    "\n",
    r#"[null,null,null,null,"0001-01-01","9999-12-31T23:59:59.999",null]"#,
    "\n",
    r#"["=1+1","0","0.0000",true,null,null,null]"#,
    "\n",
);

#[test]
fn a_table_of_every_type_reads_back() {
    let (bytes, outcome) = write(SCHEMA, &policy("require-exact", ""), ROWS).unwrap();
    assert_eq!(outcome.rows, 4);
    let kinds: Vec<_> = outcome.columns.iter().map(|c| c.representation).collect();
    use Representation::{Native, Text};
    assert_eq!(kinds, [Text, Native, Native, Native, Text, Text, Text]);
    let (invoice, qty, amount) = (
        &outcome.columns[0],
        &outcome.columns[1],
        &outcome.columns[2],
    );
    assert_eq!(
        (invoice.null, invoice.empty, invoice.exact_number),
        (1, 1, false)
    );
    assert_eq!(
        (qty.null, qty.numeric, qty.text_fallback, qty.text_forced),
        (1, 3, 0, 0)
    );
    assert_eq!(
        (amount.null, amount.numeric, amount.exact_number),
        (1, 3, true)
    );
    assert_eq!(
        (outcome.columns[3].null, outcome.columns[3].numeric),
        (1, 0)
    );

    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(&bytes[..])).unwrap();
    let sheets = workbook.sheets_metadata().to_vec();
    assert_eq!(sheets.len(), 2);
    assert_eq!(
        (sheets[0].name.as_str(), sheets[0].visible),
        ("Data", SheetVisible::Visible)
    );
    assert_eq!(
        (sheets[1].name.as_str(), sheets[1].visible),
        ("_tableprims", SheetVisible::Hidden)
    );
    let data = workbook.worksheet_range("Data").unwrap();
    assert_eq!(data.get((0, 0)), Some(&Data::String("Invoice".into())));
    assert_eq!(data.get((0, 3)), Some(&Data::Empty));
    assert_eq!(data.get((1, 0)), Some(&Data::String("INV-1".into())));
    assert_eq!(data.get((1, 3)), Some(&Data::Bool(true)));
    assert_eq!(data.get((1, 4)), Some(&Data::String("2026-11-18".into())));
    assert_eq!(
        data.get((1, 5)),
        Some(&Data::String("2026-11-18T09:30:00.250".into()))
    );
    assert_eq!(
        data.get((1, 6)),
        Some(&Data::String("2026-11-18T09:30:00Z".into()))
    );
    assert_eq!(data.get((2, 0)), Some(&Data::Empty));
    assert_eq!(
        data.get((2, 6)),
        Some(&Data::String("2026-11-18T09:30:00+00:00".into()))
    );
    assert_eq!(
        data.get((3, 5)),
        Some(&Data::String("9999-12-31T23:59:59.999".into()))
    );
    assert_eq!(data.get((4, 0)), Some(&Data::String("=1+1".into())));

    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    assert!(!sheet.contains("<f>") && !sheet.contains("<f "));
    for (reference, source) in [
        ("B2", "3"),
        ("B3", "-7"),
        ("B5", "0"),
        ("C2", "12.3400"),
        ("C3", "123.4500"),
        ("C5", "0.0000"),
    ] {
        let token = numeric_token(&sheet, reference).unwrap();
        assert!(
            same_decimal_value(token, source),
            "{reference}: {token} is not {source}"
        );
    }
    assert_eq!(numeric_token(&sheet, "C2"), Some("12.34"));
    assert!(cell_xml(&sheet, "B4").is_none() && cell_xml(&sheet, "A3").is_none());
    assert!(cell_xml(&sheet, "E2").unwrap().contains("t=\"inlineStr\""));
    assert!(cell_xml(&sheet, "A5").unwrap().contains("t=\"inlineStr\""));
    assert!(member(&bytes, "xl/styles.xml").contains("formatCode=\"0.0000\""));
}

#[test]
fn the_metadata_worksheet_holds_the_records() {
    let (bytes, _) = write(SCHEMA, &policy("require-exact", ""), ROWS).unwrap();
    let text = |fields: &[&str]| -> Vec<Option<String>> {
        let mut row: Vec<Option<String>> = fields
            .iter()
            .map(|field| (!field.is_empty()).then(|| (*field).to_owned()))
            .collect();
        row.resize(9, None);
        row
    };
    let expected = vec![
        text(&["format", "1", "tableprims", "xlsx-write/r1"]),
        text(&["sheet", "Data", "1", "4", "7"]),
        text(&[
            "column", "0", "invoice", "string", "", "", "true", "", "text",
        ]),
        text(&[
            "column", "1", "qty", "integer", "", "", "true", "", "native",
        ]),
        text(&[
            "column", "2", "amount", "decimal", "12", "4", "true", "", "native",
        ]),
        text(&[
            "column", "3", "paid", "boolean", "", "", "true", "", "native",
        ]),
        text(&["column", "4", "day", "date", "", "", "true", "", "text"]),
        text(&[
            "column",
            "5",
            "seen",
            "local-timestamp",
            "",
            "",
            "true",
            "3",
            "text",
        ]),
        text(&[
            "column",
            "6",
            "sent",
            "offset-timestamp",
            "",
            "",
            "true",
            "0",
            "text",
        ]),
        text(&["empty", "0", "1-1"]),
        text(&["end", "10"]),
    ];
    assert_eq!(strings(&bytes, "_tableprims"), expected);
    let sheet = member(&bytes, "xl/worksheets/sheet2.xml");
    assert!(!sheet.contains("<v>") && !sheet.contains("<f"));
}

const NUMBERS: &str = r#"{"format":"tableprims.schema/0","columns":[
  {"id":"n","label":"N","type":"integer","nullable":true},
  {"id":"d","label":"D","type":"decimal","precision":38,"scale":3,"nullable":true}]}"#;

fn refusal(result: Result<(Vec<u8>, Outcome), WriteError>) -> WriteError {
    result
        .map(|(_, outcome)| outcome)
        .expect_err("the write was accepted")
}

#[test]
fn require_exact_refuses_the_first_value_that_is_not_a_numeric_cell() {
    let exact = policy("require-exact", "");
    for (rows, row, column, rule) in [
        (
            "[\"1\",\"1.245\"]\n[\"9007199254740993\",null]\n",
            1,
            "n",
            "too-many-significant-digits",
        ),
        (
            "[\"9007199254740992\",null]\n",
            0,
            "n",
            "too-many-significant-digits",
        ),
        (
            "[\"9007199254740991\",null]\n",
            0,
            "n",
            "too-many-significant-digits",
        ),
        (
            "[\"10000000000000000000000000000000000000\",null]\n",
            0,
            "n",
            "not-exact-integer",
        ),
        (
            "[null,\"1234567890123.456\"]\n",
            0,
            "d",
            "too-many-significant-digits",
        ),
    ] {
        let error = refusal(write(NUMBERS, &exact, rows));
        assert_eq!(
            error.category(),
            Category::UnsupportedRepresentation,
            "{rows}"
        );
        assert_eq!(
            (error.row(), error.column(), error.rule()),
            (Some(row), Some(column), rule)
        );
        assert!(matches!(error.kind(), ErrorKind::NotNative(_)));
    }
    let rows = "[\"-999999999999999\",\"-12.500\"]\n[\"123456789012345000\",\"0.001\"]\n";
    let error = refusal(write(NUMBERS, &exact, rows));
    assert_eq!((error.row(), error.rule()), (Some(1), "not-exact-integer"));
    let rows = "[\"-999999999999999\",\"-12.500\"]\n[\"1000000000000000\",\"999999999999.999\"]\n";
    let (bytes, outcome) = write(NUMBERS, &exact, rows).unwrap();
    assert_eq!(
        (outcome.columns[0].numeric, outcome.columns[1].numeric),
        (2, 2)
    );
    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    for (reference, source) in [
        ("A2", "-999999999999999"),
        ("B2", "-12.500"),
        ("A3", "1000000000000000"),
        ("B3", "999999999999.999"),
    ] {
        let token = numeric_token(&sheet, reference).unwrap();
        assert!(
            same_decimal_value(token, source),
            "{reference}: {token} is not {source}"
        );
    }
}

#[test]
fn a_changed_digit_is_not_the_same_value() {
    let (bytes, _) = write(NUMBERS, &policy("require-exact", ""), "[null,\"1.245\"]\n").unwrap();
    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    let token = numeric_token(&sheet, "B2").unwrap();
    assert_eq!(token, "1.245");
    assert!(!same_decimal_value(&token.replace('5', "4"), "1.245"));
}

#[test]
fn text_if_needed_writes_the_whole_column_as_text() {
    let rows = "[\"1\",\"1.245\"]\n[\"9007199254740993\",\"2.000\"]\n[null,null]\n";
    let (bytes, outcome) = write(NUMBERS, &policy("text-if-needed", ""), rows).unwrap();
    let (n, d) = (&outcome.columns[0], &outcome.columns[1]);
    assert_eq!(n.representation, Representation::TextFallback);
    assert_eq!(
        (n.numeric, n.text_fallback, n.text_forced, n.null),
        (0, 2, 0, 1)
    );
    assert_eq!(d.representation, Representation::Native);
    assert_eq!(
        (d.numeric, d.text_fallback, d.text_forced, d.null),
        (2, 0, 0, 1)
    );
    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    assert_eq!(numeric_token(&sheet, "A2"), None);
    assert!(cell_xml(&sheet, "A2").unwrap().contains("<t>1</t>"));
    assert!(cell_xml(&sheet, "A3")
        .unwrap()
        .contains("<t>9007199254740993</t>"));
    assert_eq!(numeric_token(&sheet, "B3"), Some("2"));
    let meta = strings(&bytes, "_tableprims");
    assert_eq!(meta[2][8].as_deref(), Some("text-fallback"));
    assert_eq!(meta[3][8].as_deref(), Some("native"));
}

#[test]
fn text_always_writes_every_exact_number_as_text() {
    let rows = "[\"1\",\"1.245\"]\n[\"2\",null]\n";
    let (bytes, outcome) = write(NUMBERS, &policy("text-always", ""), rows).unwrap();
    let (n, d) = (&outcome.columns[0], &outcome.columns[1]);
    assert_eq!(n.representation, Representation::TextForced);
    assert_eq!((n.numeric, n.text_fallback, n.text_forced), (0, 0, 2));
    assert_eq!(
        (d.numeric, d.text_fallback, d.text_forced, d.null),
        (0, 0, 1, 1)
    );
    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    assert!(!sheet.contains("<v>"));
    assert!(cell_xml(&sheet, "B2").unwrap().contains("<t>1.245</t>"));
}

#[test]
fn quantization_runs_before_the_representation_is_decided() {
    let quantize = r#","quantize":[{"column":"d","scale":2,"rounding":"half-even"}]"#;
    let rows = "[null,\"1.245\"]\n[null,\"2.500\"]\n[null,null]\n[null,\"12345678901234.565\"]\n";
    let error = refusal(write(NUMBERS, &policy("require-exact", quantize), rows));
    assert_eq!(
        (error.row(), error.rule()),
        (Some(3), "too-many-significant-digits")
    );
    let rows = "[null,\"1.245\"]\n[null,\"2.500\"]\n[null,null]\n[null,\"123456789012.345\"]\n";
    let (bytes, outcome) = write(NUMBERS, &policy("require-exact", quantize), rows).unwrap();
    let d = &outcome.columns[1];
    assert_eq!(
        (d.numeric, d.quantized, d.scale_changed, d.null),
        (3, 2, 3, 1)
    );
    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    assert_eq!(numeric_token(&sheet, "B2"), Some("1.24"));
    assert_eq!(numeric_token(&sheet, "B3"), Some("2.5"));
    assert_eq!(numeric_token(&sheet, "B5"), Some("123456789012.34"));
    let meta = strings(&bytes, "_tableprims");
    let record: Vec<_> = meta[3]
        .iter()
        .map(|field| field.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        record,
        [
            "column",
            "1",
            "d",
            "decimal",
            "38",
            "2",
            "true",
            "",
            "native",
            "3",
            "half-even"
        ]
    );
    assert!(member(&bytes, "xl/styles.xml").contains("formatCode=\"0.00\""));
}

#[test]
fn a_quantized_value_that_no_longer_fits_is_refused() {
    let schema = r#"{"format":"tableprims.schema/0","columns":[
      {"id":"d","label":"D","type":"decimal","precision":3,"scale":1,"nullable":false}]}"#;
    let quantize = r#","quantize":[{"column":"d","scale":3,"rounding":"half-even"}]"#;
    let error = refusal(write(
        schema,
        &policy("text-always", quantize),
        "[\"1.5\"]\n",
    ));
    assert_eq!(
        (error.category(), error.rule()),
        (Category::SchemaMismatch, "quantize-overflow")
    );
    assert_eq!((error.row(), error.column()), (Some(0), Some("d")));
}

const TEXT: &str = r#"{"format":"tableprims.schema/0","columns":[
  {"id":"a","label":"A","type":"string","nullable":true},
  {"id":"b","label":"B","type":"string","nullable":false}]}"#;

#[test]
fn empty_strings_are_recorded_as_merged_ranges() {
    let rows = "[\"\",\"x\"]\n[\"\",\"\"]\n[null,\"y\"]\n[\"\",\"\"]\n[\"z\",\"\"]\n";
    let (bytes, outcome) = write(TEXT, &policy("require-exact", ""), rows).unwrap();
    assert_eq!((outcome.columns[0].empty, outcome.columns[0].null), (3, 1));
    assert_eq!((outcome.columns[1].empty, outcome.columns[1].null), (3, 0));
    let meta = strings(&bytes, "_tableprims");
    let empties: Vec<_> = meta
        .iter()
        .filter(|record| record[0].as_deref() == Some("empty"))
        .map(|record| (record[1].clone().unwrap(), record[2].clone().unwrap()))
        .collect();
    assert_eq!(
        empties,
        [
            ("0".into(), "0-1,3-3".into()),
            ("1".into(), "1-1,3-4".into())
        ]
    );
    let sheet = member(&bytes, "xl/worksheets/sheet1.xml");
    assert!(cell_xml(&sheet, "A2").is_none() && cell_xml(&sheet, "A4").is_none());
}

#[test]
fn many_ranges_continue_across_records_and_the_cap_refuses() {
    let mut rows = String::new();
    for _ in 0..600 {
        rows.push_str("[\"\",\"x\"]\n[\"y\",\"x\"]\n");
    }
    let (bytes, _) = write(TEXT, &policy("require-exact", ""), &rows).unwrap();
    let meta = strings(&bytes, "_tableprims");
    let empties: Vec<_> = meta
        .iter()
        .filter(|r| r[0].as_deref() == Some("empty"))
        .collect();
    assert_eq!(empties.len(), 2);
    assert_eq!(empties[0][2].as_ref().unwrap().split(',').count(), 512);
    assert!(empties[1][2].as_ref().unwrap().ends_with(",1198-1198"));
    assert_eq!(meta.last().unwrap()[1].as_deref(), Some("6"));

    let capped = policy("require-exact", r#","limits":{"max_empty_ranges":599}"#);
    let error = refusal(write(TEXT, &capped, &rows));
    assert_eq!(
        (error.category(), error.rule()),
        (Category::ResourceLimit, "too-many-empty-ranges")
    );
    assert_eq!((error.row(), error.column()), (Some(1198), Some("a")));
}

#[test]
fn limits_are_enforced_and_bounded_by_the_profile() {
    let rows = "[\"p\",\"x\"]\n[\"q\",\"x\"]\n[\"r\",\"x\"]\n";
    let capped = policy("require-exact", r#","limits":{"max_rows":2}"#);
    let error = refusal(write(TEXT, &capped, rows));
    assert_eq!(
        (error.category(), error.rule(), error.row()),
        (Category::ResourceLimit, "too-many-rows", Some(2))
    );
    let capped = policy("require-exact", r#","limits":{"max_line_bytes":8}"#);
    let error = refusal(write(TEXT, &capped, rows));
    assert_eq!(
        (error.category(), error.rule(), error.row()),
        (Category::ResourceLimit, "line-too-long", Some(0))
    );
    for limits in [
        r#"{"max_rows":1048576}"#,
        r#"{"max_line_bytes":16777217}"#,
        r#"{"max_empty_ranges":65537}"#,
    ] {
        let error = plan(
            TEXT,
            &policy("require-exact", &format!(r#","limits":{limits}"#)),
        )
        .unwrap_err();
        assert_eq!(
            (error.category(), error.rule()),
            (Category::ResourceLimit, "limit-above-profile")
        );
    }
    let limits = plan(TEXT, &policy("require-exact", "")).unwrap().limits();
    assert_eq!(
        (
            limits.max_rows,
            limits.max_line_bytes,
            limits.max_empty_ranges
        ),
        (1_048_575, 1_048_576, 65_536)
    );
}

#[test]
fn a_string_longer_than_a_cell_is_refused() {
    let fits = "é".repeat(32_767);
    let rows = format!("[\"{fits}\",\"x\"]\n");
    let (bytes, _) = write(TEXT, &policy("require-exact", ""), &rows).unwrap();
    assert_eq!(
        strings(&bytes, "Data")[1][0].as_deref(),
        Some(fits.as_str())
    );
    // One character outside the basic plane is two units.
    let rows = format!("[\"{}\u{1F600}\",\"x\"]\n", "a".repeat(32_766));
    let error = refusal(write(TEXT, &policy("require-exact", ""), &rows));
    assert_eq!(
        (error.category(), error.rule()),
        (Category::ResourceLimit, "cell-too-long")
    );
    assert_eq!((error.row(), error.column()), (Some(0), Some("a")));
}

#[test]
fn awkward_strings_read_back_unchanged() {
    let values = [
        "=1+1",
        "+123",
        "-456",
        "@SUM(A1)",
        "000123",
        "12345678901234567890",
        " lead",
        "trail ",
        "a\tb",
        "a\nb",
        "a\r\nb",
        "_x000D_",
        "_x005F_",
        "\u{1}\u{1f}",
        "<&>\"'",
        "é\u{1F600}\u{FFFD}",
        "\u{FFFE}\u{FFFF}",
        "true",
        "null",
        "1.0",
    ];
    let mut rows = String::new();
    for value in values {
        let escaped: String = value
            .chars()
            .map(|c| match c {
                '"' => "\\\"".to_owned(),
                '\\' => "\\\\".to_owned(),
                c if c < ' ' => format!("\\u{:04x}", c as u32),
                c => c.to_string(),
            })
            .collect();
        rows.push_str(&format!("[\"{escaped}\",\"x\"]\n"));
    }
    let (bytes, _) = write(TEXT, &policy("require-exact", ""), &rows).unwrap();
    let read = strings(&bytes, "Data");
    for (at, value) in values.iter().enumerate() {
        assert_eq!(read[at + 1][0].as_deref(), Some(*value), "value {at}");
    }
    assert!(!member(&bytes, "xl/worksheets/sheet1.xml").contains("<f"));
}

#[test]
fn rows_that_do_not_fit_are_refused_with_a_location() {
    let exact = policy("require-exact", "");
    for (rows, category, rule, row, column) in [
        (
            "[\"a\"]\n",
            Category::SchemaMismatch,
            "arity-mismatch",
            0,
            Some("b"),
        ),
        (
            "[\"a\",\"b\",\"c\"]\n",
            Category::SchemaMismatch,
            "arity-mismatch",
            0,
            None,
        ),
        (
            "[\"a\",null]\n",
            Category::SchemaMismatch,
            "null-not-allowed",
            0,
            Some("b"),
        ),
        (
            "[\"a\",\"b\"]\n[true,\"b\"]\n",
            Category::SchemaMismatch,
            "wrong-scalar-kind",
            1,
            Some("a"),
        ),
        (
            "[\"a\",1]\n",
            Category::InvalidInput,
            "numeric-literal",
            0,
            None,
        ),
        (
            "[\"a\",\"b\"]\n\n",
            Category::InvalidInput,
            "blank-line",
            1,
            None,
        ),
        (
            "{\"a\":1}\n",
            Category::InvalidInput,
            "not-an-array",
            0,
            None,
        ),
    ] {
        let error = refusal(write(TEXT, &exact, rows));
        assert_eq!(
            (error.category(), error.rule(), error.row(), error.column()),
            (category, rule, Some(row), column),
            "{rows}"
        );
    }
    for (rows, category, rule) in [
        (
            "[\"007\",null]\n",
            Category::InvalidInput,
            "exact-leading-zero",
        ),
        (
            "[\"-0\",null]\n",
            Category::InvalidInput,
            "exact-negative-zero",
        ),
        (
            "[\"1e3\",null]\n",
            Category::InvalidInput,
            "exact-invalid-character",
        ),
        (
            "[null,\"1.24\"]\n",
            Category::SchemaMismatch,
            "exact-scale-mismatch",
        ),
        (
            "[null,\"1\"]\n",
            Category::SchemaMismatch,
            "exact-point-mismatch",
        ),
    ] {
        let error = refusal(write(NUMBERS, &exact, rows));
        assert_eq!(
            (error.category(), error.rule(), error.row()),
            (category, rule, Some(0)),
            "{rows}"
        );
    }
    let error = refusal(write(TEXT, &exact, "\u{FEFF}[\"a\",\"b\"]\n"));
    assert_eq!(
        (error.category(), error.rule()),
        (Category::InvalidInput, "byte-order-mark")
    );
    let message = refusal(write(TEXT, &exact, "[\"secret\",null]\n")).to_string();
    assert_eq!(
        message,
        "schema-mismatch: null-not-allowed at row 0 in column b"
    );
}

#[test]
fn the_plan_refuses_what_a_workbook_cannot_hold() {
    for sheet in ["a/b", "x".repeat(32).as_str(), "'quoted'", "a[1]"] {
        let policy = format!(
            r#"{{"format":"tableprims.policy/0","exact_numbers":"require-exact","sheet":"{sheet}"}}"#
        );
        let error = plan(TEXT, &policy).unwrap_err();
        assert_eq!(
            (error.category(), error.rule()),
            (
                Category::UnsupportedRepresentation,
                "sheet-name-unsupported"
            ),
            "{sheet}"
        );
    }
    let long = "i".repeat(32_768);
    let schema = format!(
        r#"{{"format":"tableprims.schema/0","columns":[{{"id":"{long}","label":"L","type":"string","nullable":true}}]}}"#
    );
    let error = plan(&schema, &policy("require-exact", "")).unwrap_err();
    assert_eq!(
        (error.category(), error.rule()),
        (Category::ResourceLimit, "cell-too-long")
    );
    let quantize = r#","quantize":[{"column":"a","scale":2,"rounding":"half-even"}]"#;
    let error = plan(TEXT, &policy("require-exact", quantize)).unwrap_err();
    assert_eq!(
        (error.category(), error.rule()),
        (Category::SchemaMismatch, "quantize-not-decimal")
    );
}

#[test]
fn a_second_read_that_differs_is_refused() {
    let never = AtomicBool::new(false);
    let scan = |rows: &str, exact: &str| {
        plan(NUMBERS, &policy(exact, ""))
            .unwrap()
            .scan(rows.as_bytes(), &never)
            .unwrap()
    };
    let first = "[\"1\",\"1.245\"]\n[\"2\",null]\n";
    for (exact, second, row) in [
        ("require-exact", "[\"1\",\"1.245\"]\n", None),
        (
            "require-exact",
            "[\"1\",\"1.245\"]\n[\"2\",null]\n[\"3\",null]\n",
            Some(2),
        ),
        (
            "require-exact",
            "[\"1\",\"1.245\"]\n[null,\"2.000\"]\n",
            None,
        ),
        (
            "require-exact",
            "[\"1\",\"1.245\"]\n[\"9007199254740993\",null]\n",
            Some(1),
        ),
        (
            "require-exact",
            "[\"1\",\"1.245\"]\n[\"x\",null]\n",
            Some(1),
        ),
        (
            "text-if-needed",
            "[\"1\",\"1.245\"]\n[\"9007199254740993\",null]\n",
            Some(1),
        ),
    ] {
        let mut bytes = Vec::new();
        let error = scan(first, exact)
            .write(second.as_bytes(), &mut bytes, None, &never)
            .unwrap_err();
        assert!(matches!(error.kind(), ErrorKind::InputChanged), "{second}");
        assert_eq!(
            (error.category(), error.rule(), error.row()),
            (Category::IoFailure, "input-changed", row)
        );
    }
    // A column that fell back to text must still need text on the second read.
    let fallback = "[\"1\",null]\n[\"9007199254740993\",null]\n";
    let mut bytes = Vec::new();
    let error = scan(fallback, "text-if-needed")
        .write(
            "[\"1\",null]\n[\"2\",null]\n".as_bytes(),
            &mut bytes,
            None,
            &never,
        )
        .unwrap_err();
    assert!(matches!(error.kind(), ErrorKind::InputChanged));
    // A different input with the same outcome is the caller's to detect.
    let mut bytes = Vec::new();
    scan(first, "require-exact")
        .write(first.as_bytes(), &mut bytes, None, &never)
        .unwrap();
}

#[test]
fn cancellation_stops_both_reads() {
    let stop = AtomicBool::new(true);
    let never = AtomicBool::new(false);
    let rows = "[\"a\",\"b\"]\n";
    let error = plan(TEXT, &policy("require-exact", ""))
        .unwrap()
        .scan(rows.as_bytes(), &stop)
        .unwrap_err();
    assert_eq!(
        (error.category(), error.rule()),
        (Category::Cancelled, "cancelled")
    );
    let scan = plan(TEXT, &policy("require-exact", ""))
        .unwrap()
        .scan(rows.as_bytes(), &never)
        .unwrap();
    let mut bytes = Vec::new();
    let error = scan
        .write(rows.as_bytes(), &mut bytes, None, &stop)
        .unwrap_err();
    assert_eq!(error.category(), Category::Cancelled);
    assert!(bytes.is_empty());
}

#[test]
fn an_empty_input_writes_the_header_and_the_records() {
    let (bytes, outcome) = write(TEXT, &policy("text-if-needed", ""), "").unwrap();
    assert_eq!(outcome.rows, 0);
    assert_eq!(
        strings(&bytes, "Data"),
        [[Some("A".to_owned()), Some("B".to_owned())]]
    );
    assert_eq!(strings(&bytes, "_tableprims")[1][3].as_deref(), Some("0"));
}

#[test]
fn staging_uses_the_given_directory() {
    let never = AtomicBool::new(false);
    let scan = plan(TEXT, &policy("require-exact", ""))
        .unwrap()
        .scan("".as_bytes(), &never)
        .unwrap();
    let missing = std::env::temp_dir().join("tableprims-xlsx-no-such-directory");
    let mut bytes = Vec::new();
    let error = scan
        .write("".as_bytes(), &mut bytes, Some(&missing), &never)
        .unwrap_err();
    assert_eq!(error.category(), Category::IoFailure);
    scan.write(
        "".as_bytes(),
        &mut bytes,
        Some(&std::env::temp_dir()),
        &never,
    )
    .unwrap();
    assert!(!bytes.is_empty());
}

struct FailingSink;

impl std::io::Write for FailingSink {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::StorageFull.into())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn a_failing_sink_is_a_write_failure() {
    let never = AtomicBool::new(false);
    let rows = "[\"a\",\"b\"]\n";
    let scan = plan(TEXT, &policy("require-exact", ""))
        .unwrap()
        .scan(rows.as_bytes(), &never)
        .unwrap();
    let error = scan
        .write(rows.as_bytes(), FailingSink, None, &never)
        .unwrap_err();
    assert_eq!(
        (error.category(), error.rule()),
        (Category::IoFailure, "write-failed")
    );
}

#[test]
fn the_pinned_writer_is_the_one_in_the_lockfile() {
    let lock =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock")).unwrap();
    let pinned = format!(
        "name = \"{}\"\nversion = \"{}\"\n",
        tableprims_xlsx::BACKEND_NAME,
        tableprims_xlsx::BACKEND_VERSION
    );
    assert!(lock.replace("\r\n", "\n").contains(&pinned));
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml")).unwrap();
    let line = manifest
        .lines()
        .find(|line| line.starts_with("rust_xlsxwriter"))
        .unwrap();
    assert!(line.contains(&format!("\"={}\"", tableprims_xlsx::BACKEND_VERSION)));
    assert!(line.contains("default-features = false") && line.contains("[\"constant_memory\"]"));
}
