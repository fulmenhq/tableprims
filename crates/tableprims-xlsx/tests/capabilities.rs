// SPDX-License-Identifier: MIT OR Apache-2.0
//! Runs the repository's capability suite against the two Rust surfaces:
//! the core crate and this adapter. The suite is owned by this repository
//! and kept apart from the vendored vectors of the standard.

use std::io::{BufReader, Cursor};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use calamine::{Data, Reader, Xlsx};
use tableprims::binary64::Binary64Error;
use tableprims::capability::{self, Disposition, BINARY64_DECISION, QUANTIZE, XLSX_WRITE};
use tableprims::documents::{parse_policy, parse_schema};
use tableprims::exact::{Decimal, DecimalType, RoundingMode};
use tableprims::json::{parse_document, Json};
use tableprims::ndjson::RowReader;
use tableprims::table::Value;
use tableprims_xlsx::Plan;

const CORE: &str = "rust/tableprims";
const ADAPTER: &str = "rust/tableprims-xlsx";

#[derive(Clone)]
struct Case {
    surface: String,
    capability: String,
    disposition: String,
}

#[derive(Clone)]
struct Suite {
    capabilities: Vec<String>,
    reports: Vec<(String, Vec<String>)>,
    cases: Vec<Case>,
    unknown: Vec<String>,
}

fn member<'a>(object: &'a Json, name: &str) -> &'a Json {
    match object {
        Json::Object(members) => {
            let found: Vec<_> = members.iter().filter(|(key, _)| key == name).collect();
            assert_eq!(found.len(), 1, "member {name}");
            &found[0].1
        }
        other => panic!("not an object: {other:?}"),
    }
}

fn text(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

fn texts(value: &Json) -> Vec<String> {
    match value {
        Json::Array(items) => items.iter().map(text).collect(),
        other => panic!("not an array: {other:?}"),
    }
}

fn items(value: &Json) -> &[Json] {
    match value {
        Json::Array(items) => items,
        other => panic!("not an array: {other:?}"),
    }
}

fn load() -> Suite {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/capabilities/v0/capabilities.json");
    let document = parse_document(&std::fs::read_to_string(path).unwrap(), 4).unwrap();
    let Json::Object(members) = &document else {
        panic!("suite is not an object")
    };
    let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "suite",
            "description",
            "capabilities",
            "surfaces",
            "cases",
            "unknown"
        ]
    );
    assert_eq!(
        text(member(&document, "suite")),
        "tableprims-capabilities/0"
    );
    Suite {
        capabilities: texts(member(&document, "capabilities")),
        reports: items(member(&document, "surfaces"))
            .iter()
            .map(|s| (text(member(s, "surface")), texts(member(s, "report"))))
            .collect(),
        cases: items(member(&document, "cases"))
            .iter()
            .map(|c| Case {
                surface: text(member(c, "surface")),
                capability: text(member(c, "capability")),
                disposition: text(member(c, "disposition")),
            })
            .collect(),
        unknown: texts(member(&document, "unknown")),
    }
}

const SCHEMA: &str = r#"{"format":"tabular-exchange.schema/0","columns":[
  {"id":"amount","label":"Amount","type":"decimal","precision":3,"scale":2,"nullable":false}]}"#;
const POLICY: &str = r#"{"format":"tabular-exchange.policy/0","exact_numbers":"require-exact",
  "quantize":[{"column":"amount","scale":1,"rounding":"half-even"}],
  "extensions":{"tableprims":{"sheet":"Data"}}}"#;

/// Runs the operation a capability names on a surface, through the public
/// API, and checks its result. A capability with no operation here fails.
fn exercise(surface: &str, capability: &str) -> Result<(), String> {
    match (surface, capability) {
        (CORE, capability::READ_SCHEMA) => {
            let schema = parse_schema(SCHEMA).map_err(|e| format!("{e:?}"))?;
            (schema.columns().len() == 1 && schema.columns()[0].id() == "amount")
                .then_some(())
                .ok_or_else(|| "schema columns".to_owned())
        }
        (CORE, capability::READ_POLICY) => {
            let policy = parse_policy(POLICY).map_err(|e| format!("{e:?}"))?;
            let quantize = policy.quantize();
            (quantize.len() == 1 && quantize[0].column() == "amount" && quantize[0].scale() == 1)
                .then_some(())
                .ok_or_else(|| "policy quantize".to_owned())
        }
        (CORE, capability::READ_ROWS) => {
            let schema = parse_schema(SCHEMA).map_err(|e| format!("{e:?}"))?;
            let mut rows = RowReader::new(BufReader::new(&b"[\"1.25\"]\n"[..]), 1 << 10);
            let (index, cells) = rows.next().ok_or("no row")?.map_err(|e| format!("{e:?}"))?;
            let scalars: Vec<_> = cells.iter().map(|c| c.as_scalar()).collect();
            let values = schema
                .check_row(index, &scalars)
                .map_err(|e| format!("{e:?}"))?;
            let expected = DecimalType::new(3, 2)
                .and_then(|ty| Decimal::parse("1.25", ty))
                .map_err(|e| format!("{e:?}"))?;
            if values != [Value::Decimal(expected)] || rows.next().is_some() {
                return Err(format!("rows {values:?}"));
            }
            Ok(())
        }
        (CORE, QUANTIZE) => {
            let from = DecimalType::new(3, 2).map_err(|e| format!("{e:?}"))?;
            let to = DecimalType::new(3, 1).map_err(|e| format!("{e:?}"))?;
            let quantized = Decimal::parse("1.35", from)
                .and_then(|d| d.quantize(to, RoundingMode::HalfEven))
                .map_err(|e| format!("{e:?}"))?;
            (quantized.value.to_string() == "1.4"
                && quantized.value_changed
                && quantized.scale_changed)
                .then_some(())
                .ok_or_else(|| format!("quantized {quantized:?}"))
        }
        (CORE, BINARY64_DECISION) => {
            let ty = DecimalType::new(3, 2).map_err(|e| format!("{e:?}"))?;
            let stored = Decimal::parse("0.50", ty)
                .map_err(|e| format!("{e:?}"))
                .and_then(|d| {
                    tableprims::binary64::from_decimal(d).map_err(|e| format!("{e:?}"))
                })?;
            #[allow(clippy::float_cmp)]
            if stored != 0.5 {
                return Err(format!("stored {stored}"));
            }
            // Sixteen significant digits are refused.
            let wide = DecimalType::new(17, 16).map_err(|e| format!("{e:?}"))?;
            let refused = Decimal::parse("0.1234567890123456", wide)
                .map(tableprims::binary64::from_decimal)
                .map_err(|e| format!("{e:?}"))?;
            match refused {
                Err(Binary64Error::TooManyDigits) => Ok(()),
                other => Err(format!("16 digits: {other:?}")),
            }
        }
        (ADAPTER, XLSX_WRITE) => {
            let never = AtomicBool::new(false);
            let plan = Plan::new(
                &parse_schema(SCHEMA).map_err(|e| format!("{e:?}"))?,
                &parse_policy(POLICY).map_err(|e| format!("{e:?}"))?,
            )
            .map_err(|e| format!("{e:?}"))?;
            let rows = b"[\"1.25\"]\n";
            let scan = plan.scan(&rows[..], &never).map_err(|e| format!("{e:?}"))?;
            let mut bytes = Vec::new();
            scan.write(&rows[..], &mut bytes, None, &never)
                .map_err(|e| format!("{e:?}"))?;
            // Read the cell back with an independent reader.
            let mut workbook: Xlsx<_> =
                Xlsx::new(Cursor::new(bytes)).map_err(|e| format!("{e:?}"))?;
            let range = workbook
                .worksheet_range("Data")
                .map_err(|e| format!("{e:?}"))?;
            match range.get((1, 0)) {
                Some(Data::Float(value)) if value.to_string() == "1.2" => Ok(()),
                other => Err(format!("cell: {other:?}")),
            }
        }
        _ => Err(format!("no operation exercises {capability} on {surface}")),
    }
}

fn report(surface: &str) -> &'static [&'static str] {
    match surface {
        CORE => tableprims::capabilities(),
        ADAPTER => tableprims_xlsx::capabilities(),
        other => panic!("not a Rust surface: {other}"),
    }
}

fn require(surface: &str, capability: &str) -> Result<(), tableprims::CapabilityError> {
    match surface {
        CORE => tableprims::require(capability),
        ADAPTER => tableprims_xlsx::require(capability),
        other => panic!("not a Rust surface: {other}"),
    }
}

/// Every disagreement between the suite and the two Rust surfaces.
fn evaluate(suite: &Suite) -> Vec<String> {
    let mut out = Vec::new();
    for surface in [CORE, ADAPTER] {
        let expected: Vec<_> = suite.reports.iter().filter(|(s, _)| s == surface).collect();
        if expected.len() != 1 {
            out.push(format!("{} report entries for {surface}", expected.len()));
            continue;
        }
        let got: Vec<String> = report(surface).iter().map(|&id| id.to_owned()).collect();
        if got != expected[0].1 {
            out.push(format!(
                "{surface} report: got {got:?}, want {:?}",
                expected[0].1
            ));
        }
    }
    for case in suite
        .cases
        .iter()
        .filter(|c| c.surface.starts_with("rust/"))
    {
        let (surface, id) = (case.surface.as_str(), case.capability.as_str());
        let answer = require(surface, id);
        match (case.disposition.as_str(), answer) {
            ("supported", Ok(())) => {
                if let Err(e) = exercise(surface, id) {
                    out.push(format!("{surface} {id}: operation: {e}"));
                }
            }
            (want @ ("unsupported" | "unknown"), Err(err)) => {
                if err.disposition().as_str() != want || err.capability() != id {
                    out.push(format!(
                        "{surface} {id}: got {} {:?}, want {want}",
                        err.disposition().as_str(),
                        err.capability()
                    ));
                }
            }
            (want, got) => out.push(format!("{surface} {id}: want {want}, got {got:?}")),
        }
    }
    // Identifiers near a known one, and the empty one, are unknown on every
    // Rust surface.
    for surface in [CORE, ADAPTER] {
        for id in &suite.unknown {
            match require(surface, id) {
                Err(err) if err.disposition() == Disposition::Unknown && err.capability() == id => {
                }
                other => out.push(format!("{surface} {id:?}: got {other:?}, want unknown")),
            }
        }
    }
    out
}

#[test]
fn capability_suite() {
    let suite = load();
    assert_eq!(
        (
            suite.cases.len(),
            suite.reports.len(),
            suite.capabilities.len(),
            suite.unknown.len()
        ),
        (18, 3, 6, 9)
    );
    let rust = suite
        .cases
        .iter()
        .filter(|c| c.surface.starts_with("rust/"))
        .count();
    assert_eq!(rust, 12);
    assert_eq!(
        suite.capabilities,
        capability::KNOWN
            .iter()
            .map(|&id| id.to_owned())
            .collect::<Vec<_>>()
    );
    // Each surface has one case for every capability, in order, and no case
    // is an unknown control: those are listed once for every surface.
    for (surface, _) in &suite.reports {
        let cases: Vec<_> = suite
            .cases
            .iter()
            .filter(|c| &c.surface == surface)
            .collect();
        let named: Vec<String> = cases.iter().map(|c| c.capability.clone()).collect();
        assert_eq!(named, suite.capabilities, "{surface}");
        assert!(
            cases.iter().all(|c| c.disposition != "unknown"),
            "{surface}"
        );
    }
    let disagreements = evaluate(&suite);
    assert!(disagreements.is_empty(), "{disagreements:#?}");
}

type Mutation = Box<dyn Fn(&mut Suite)>;

#[test]
fn capability_suite_detects_changes() {
    let suite = load();
    let at = |surface: &str, id: &str| {
        suite
            .cases
            .iter()
            .position(|c| c.surface == surface && c.capability == id)
            .unwrap()
    };
    let mutations: Vec<(&str, Mutation)> = vec![
        ("supported as unsupported", {
            let i = at(CORE, BINARY64_DECISION);
            Box::new(move |s| s.cases[i].disposition = "unsupported".into())
        }),
        ("core writes a workbook", {
            let i = at(CORE, XLSX_WRITE);
            Box::new(move |s| s.cases[i].disposition = "supported".into())
        }),
        ("adapter capability as unknown", {
            let i = at(ADAPTER, XLSX_WRITE);
            Box::new(move |s| s.cases[i].disposition = "unknown".into())
        }),
        (
            "known identifier as an unknown control",
            Box::new(|s| s.unknown.push(QUANTIZE.into())),
        ),
        (
            "unsupported identifier as an unknown control",
            Box::new(|s| s.unknown.push(XLSX_WRITE.into())),
        ),
        (
            "report entry on the wrong surface",
            Box::new(|s| {
                let core = s.reports.iter_mut().find(|(n, _)| n == CORE).unwrap();
                core.1.push(XLSX_WRITE.into());
            }),
        ),
        (
            "report entry repeated",
            Box::new(|s| {
                let adapter = s.reports.iter_mut().find(|(n, _)| n == ADAPTER).unwrap();
                adapter.1.push(XLSX_WRITE.into());
            }),
        ),
        (
            "report entry missing",
            Box::new(|s| {
                let core = s.reports.iter_mut().find(|(n, _)| n == CORE).unwrap();
                core.1.pop();
            }),
        ),
    ];
    for (name, mutate) in mutations {
        let mut changed = suite.clone();
        mutate(&mut changed);
        assert!(!evaluate(&changed).is_empty(), "{name}: no disagreement");
    }
}

#[test]
fn adapter_answers_for_core_capabilities() {
    let err = tableprims_xlsx::require(QUANTIZE).unwrap_err();
    assert_eq!(err.disposition(), Disposition::Unsupported);
    assert_eq!(tableprims_xlsx::CAPABILITY, XLSX_WRITE);
}
