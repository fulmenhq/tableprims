// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs the `quantize` vectors of the Tabular Exchange Standard, contract
//! revision `tabular-exchange/0.1.0`, through the workbook writer and reads
//! the quantized text back from the workbook with an independent reader.
//!
//! The vectors are vendored unedited under `third_party/crucible/`; the test
//! target `tabular_exchange` of `tableprims-cli` checks their digests and
//! runs the same vectors through the `tablebolt` binary for the verdict, the
//! rule and the two counters. That target has no workbook reader, so the
//! text a quantized value is written as is compared here.

#[path = "../../tableprims-cli/tests/support/vector_json.rs"]
mod vector_json;

use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use calamine::{Data, Reader, Xlsx};
use tableprims::documents::{parse_policy, parse_schema};
use tableprims_xlsx::Plan;
use vector_json::Json;

fn quantize_vectors() -> Vec<Json> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../third_party/crucible/v0.5.1/config/data/tabular-exchange/v0.1.0/value-vectors.json",
    );
    let vectors = vector_json::parse(&std::fs::read(path).unwrap()).unwrap();
    vectors.at("quantize").items().to_vec()
}

/// What the writer did with one vector, in the members a vector states.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The text of the only data cell, and whether its value and its scale
    /// were counted as changed.
    Accept {
        result: String,
        value_changed: bool,
        scale_changed: bool,
    },
    Refuse {
        category: String,
        rule: String,
    },
}

fn text(entry: &Json, name: &str) -> String {
    entry.at(name).text().to_owned()
}

fn flag(entry: &Json, name: &str) -> bool {
    match entry.at(name) {
        Json::Bool(flag) => *flag,
        other => panic!("{name}: {}", other.compact()),
    }
}

fn expected(entry: &Json) -> Outcome {
    match entry.at("verdict").text() {
        "accept" => Outcome::Accept {
            result: text(entry, "result"),
            value_changed: flag(entry, "value_changed"),
            scale_changed: flag(entry, "scale_changed"),
        },
        "refuse" => Outcome::Refuse {
            category: text(entry, "category"),
            rule: text(entry, "rule"),
        },
        other => panic!("verdict {other}"),
    }
}

/// Writes the value as the only cell of a decimal column that the policy
/// quantizes and stores as text, and reads the cell back.
fn actual(entry: &Json) -> Outcome {
    let schema = format!(
        r#"{{"format":"tabular-exchange.schema/0","columns":[{{"id":"v","label":"V","type":"decimal","precision":{},"scale":{},"nullable":false}}]}}"#,
        entry.at("precision").compact(),
        entry.at("scale").compact()
    );
    let policy = format!(
        r#"{{"format":"tabular-exchange.policy/0","exact_numbers":"text-always","quantize":[{{"column":"v","scale":{},"rounding":{}}}],"extensions":{{"tableprims":{{"sheet":"Data"}}}}}}"#,
        entry.at("target_scale").compact(),
        entry.at("rounding").compact()
    );
    let rows = format!("[{}]\n", entry.at("text").compact());
    let never = AtomicBool::new(false);
    let mut bytes = Vec::new();
    let written = Plan::new(
        &parse_schema(&schema).unwrap(),
        &parse_policy(&policy).unwrap(),
    )
    .and_then(|plan| plan.scan(rows.as_bytes(), &never))
    .and_then(|scan| scan.write(rows.as_bytes(), &mut bytes, None, &never));
    let outcome = match written {
        Ok(outcome) => outcome,
        Err(error) => {
            return Outcome::Refuse {
                category: error.category().as_str().to_owned(),
                rule: error.rule().to_owned(),
            }
        }
    };
    let mut workbook: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).unwrap();
    let range = workbook.worksheet_range("Data").unwrap();
    let cells: Vec<&Data> = range.rows().nth(1).unwrap().iter().collect();
    let [Data::String(result)] = cells.as_slice() else {
        panic!("the data row is not one text cell: {cells:?}");
    };
    let column = &outcome.columns[0];
    assert!(column.quantized <= 1 && column.scale_changed <= 1);
    Outcome::Accept {
        result: result.clone(),
        value_changed: column.quantized == 1,
        scale_changed: column.scale_changed == 1,
    }
}

/// Returns the vectors whose outcome differs from the one stated.
fn disagreements(entries: &[Json]) -> Vec<String> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let (expected, actual) = (expected(entry), actual(entry));
            (expected != actual).then(|| {
                format!(
                    "quantize[{index}] {}\n    expected: {expected:?}\n    actual:   {actual:?}",
                    entry.compact()
                )
            })
        })
        .collect()
}

#[test]
fn quantized_text_agrees_with_the_vectors() {
    let entries = quantize_vectors();
    let accepted = entries
        .iter()
        .filter(|entry| entry.at("verdict").text() == "accept")
        .count();
    assert_eq!((entries.len(), accepted), (77, 75));
    let disagree = disagreements(&entries);
    assert!(disagree.is_empty(), "{}", disagree.join("\n"));
}

/// Fault control: a stated result that differs by one digit, by its scale
/// alone or by its sign disagrees, and so do a flipped counter flag and a
/// refusal stated under another rule.
#[test]
fn an_altered_quantize_result_disagrees() {
    let entries = quantize_vectors();
    let accepted = entries
        .iter()
        .find(|entry| entry.at("verdict").text() == "accept" && entry.at("result").text() == "2")
        .unwrap();
    let refused = entries
        .iter()
        .find(|entry| entry.at("verdict").text() == "refuse")
        .unwrap();
    assert!(disagreements(&[accepted.clone(), refused.clone()]).is_empty());

    let mut faults = Vec::new();
    for result in ["3", "2.0", "-2"] {
        let mut fault = accepted.clone();
        fault.set("result", Json::string(result));
        faults.push(fault);
    }
    let mut flipped = accepted.clone();
    flipped.set(
        "value_changed",
        Json::Bool(!flag(accepted, "value_changed")),
    );
    faults.push(flipped);
    let mut rule = refused.clone();
    rule.set("rule", Json::string("quantize-scale-too-large"));
    faults.push(rule);
    // The rounding mode altered under an unaltered expectation: 2.5 to
    // scale 0 is 2 only when a tie goes to the even neighbour.
    let mut mode = accepted.clone();
    assert_eq!(mode.at("rounding").text(), "half-even");
    mode.set("rounding", Json::string("half-away-from-zero"));
    faults.push(mode);
    assert_eq!(disagreements(&faults).len(), faults.len());
}
