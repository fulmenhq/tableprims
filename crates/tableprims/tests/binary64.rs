// SPDX-License-Identifier: MIT OR Apache-2.0

//! Runs conformance/binary64/v0/binary64.json, whose expectations are
//! derived independently of this crate, against the binary64 facts and the
//! `xlsx-write/r1` rule.

use std::path::PathBuf;

use tableprims::binary64::{self, Binary64Error, Facts};
use tableprims::exact::{Decimal, DecimalType, Integer};
use tableprims::json::{parse_document, Json};

/// Cases in the suite; a shorter suite fails.
const CASES: usize = 63;

struct Case {
    kind: String,
    text: String,
    precision: Option<u8>,
    scale: Option<u8>,
    coefficient_digits: Option<u8>,
    bits: u64,
    exact: bool,
    round_trips: bool,
    xlsx_write_r1: String,
}

fn member<'a>(value: &'a Json, name: &str) -> Option<&'a Json> {
    let Json::Object(members) = value else {
        panic!("not an object: {value:?}")
    };
    let found: Vec<_> = members.iter().filter(|(key, _)| key == name).collect();
    assert!(found.len() <= 1, "member {name} repeated");
    found.first().map(|(_, value)| value)
}

fn required<'a>(value: &'a Json, name: &str) -> &'a Json {
    member(value, name).unwrap_or_else(|| panic!("member {name} missing"))
}

fn text(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        other => panic!("not a string: {other:?}"),
    }
}

fn boolean(value: &Json) -> bool {
    match value {
        Json::Boolean(flag) => *flag,
        other => panic!("not a boolean: {other:?}"),
    }
}

fn small(value: &Json) -> u8 {
    match value {
        Json::Integer(n) => u8::try_from(*n).expect("small integer"),
        other => panic!("not an integer: {other:?}"),
    }
}

fn load() -> Vec<Case> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/binary64/v0/binary64.json");
    let document = parse_document(&std::fs::read_to_string(path).unwrap(), 4).unwrap();
    let Json::Object(members) = &document else {
        panic!("suite is not an object")
    };
    let names: Vec<&str> = members.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["suite", "description", "reasons", "cases"]);
    assert_eq!(text(required(&document, "suite")), "tableprims-binary64/0");
    let Json::Array(cases) = required(&document, "cases") else {
        panic!("cases is not an array")
    };
    cases
        .iter()
        .map(|c| {
            let Json::Object(fields) = c else {
                panic!("case is not an object")
            };
            for (name, _) in fields {
                assert!(
                    [
                        "kind",
                        "text",
                        "precision",
                        "scale",
                        "coefficient_digits",
                        "bits",
                        "exact",
                        "round_trips",
                        "xlsx_write_r1"
                    ]
                    .contains(&name.as_str()),
                    "unknown member {name}"
                );
            }
            let bits = text(required(c, "bits"));
            Case {
                kind: text(required(c, "kind")),
                text: text(required(c, "text")),
                precision: member(c, "precision").map(small),
                scale: member(c, "scale").map(small),
                coefficient_digits: member(c, "coefficient_digits").map(small),
                bits: u64::from_str_radix(bits.strip_prefix("0x").expect("hex bits"), 16)
                    .expect("hex bits"),
                exact: boolean(required(c, "exact")),
                round_trips: boolean(required(c, "round_trips")),
                xlsx_write_r1: text(required(c, "xlsx_write_r1")),
            }
        })
        .collect()
}

/// What this crate observes for one case: the facts and the verdict of the
/// `xlsx-write/r1` rule.
fn observe(case: &Case) -> (Facts, Result<f64, Binary64Error>) {
    match case.kind.as_str() {
        "integer" => {
            assert!(
                case.precision.is_none()
                    && case.scale.is_none()
                    && case.coefficient_digits.is_none(),
                "{}: decimal members on an integer",
                case.text
            );
            let value = Integer::parse(&case.text).expect("integer");
            (
                binary64::integer_facts(value),
                binary64::from_integer(value),
            )
        }
        "decimal" => {
            let ty = DecimalType::new(case.precision.unwrap(), case.scale.unwrap()).unwrap();
            let value = Decimal::parse(&case.text, ty).expect("decimal");
            assert_eq!(
                Some(value.significant_digits()),
                case.coefficient_digits,
                "{}: coefficient digits",
                case.text
            );
            (
                binary64::decimal_facts(value),
                binary64::from_decimal(value),
            )
        }
        other => panic!("unknown kind {other}"),
    }
}

/// Every disagreement between the suite and this crate, as text.
fn disagreements(cases: &[Case]) -> Vec<String> {
    let mut out = Vec::new();
    for case in cases {
        let (facts, verdict) = observe(case);
        let id = format!("{} {}", case.kind, case.text);
        if facts.value().to_bits() != case.bits {
            out.push(format!("{id}: bits {:#018x}", facts.value().to_bits()));
        }
        if facts.is_exact() != case.exact {
            out.push(format!("{id}: exact {}", facts.is_exact()));
        }
        if facts.round_trips() != case.round_trips {
            out.push(format!("{id}: round_trips {}", facts.round_trips()));
        }
        match (&verdict, case.xlsx_write_r1.as_str()) {
            (Ok(stored), "ok") if stored.to_bits() == case.bits => {}
            (Err(error), code) if error.code() == code => {}
            _ => out.push(format!("{id}: xlsx_write_r1 {verdict:?}")),
        }
    }
    out
}

#[test]
fn the_crate_agrees_with_every_case() {
    let cases = load();
    assert_eq!(cases.len(), CASES);
    assert_eq!(disagreements(&cases), Vec::<String>::new());
}

#[test]
fn the_suite_covers_each_fact_and_verdict() {
    let cases = load();
    for kind in ["integer", "decimal"] {
        let of_kind: Vec<_> = cases.iter().filter(|c| c.kind == kind).collect();
        for (exact, round_trips) in [(true, true), (true, false), (false, true), (false, false)] {
            assert!(
                of_kind
                    .iter()
                    .any(|c| c.exact == exact && c.round_trips == round_trips),
                "{kind}: no case with exact={exact} round_trips={round_trips}"
            );
        }
    }
    // `not-round-trip` cannot occur under the rule: a value of at most 15
    // significant digits always round-trips.
    for code in ["ok", "too-many-digits", "not-exact-integer"] {
        assert!(cases.iter().any(|c| c.xlsx_write_r1 == code), "{code}");
    }
    assert!(cases.iter().all(|c| c.xlsx_write_r1 != "not-round-trip"));
    // A scaled value whose coefficient has more digits than are significant.
    assert!(cases
        .iter()
        .any(|c| c.xlsx_write_r1 == "ok"
            && c.coefficient_digits.is_some_and(|d| usize::from(d) > 15)));
}

type Mutation = Box<dyn Fn(&mut Case)>;

#[test]
fn an_altered_expectation_is_a_disagreement() {
    let mutations: Vec<(&str, Mutation)> = vec![
        ("bits", Box::new(|c| c.bits ^= 1)),
        ("exact", Box::new(|c| c.exact = !c.exact)),
        ("round_trips", Box::new(|c| c.round_trips = !c.round_trips)),
        (
            "xlsx_write_r1",
            Box::new(|c| {
                c.xlsx_write_r1 = if c.xlsx_write_r1 == "ok" {
                    "not-round-trip".into()
                } else {
                    "ok".into()
                }
            }),
        ),
    ];
    for (name, mutate) in &mutations {
        for at in 0..CASES {
            let mut cases = load();
            mutate(&mut cases[at]);
            assert!(
                !disagreements(&cases[at..=at]).is_empty(),
                "{name} at {at} was not detected"
            );
        }
    }
}

/// The reason codes and error texts of the suite, which the Go tests check
/// too, so the two implementations give the same text for each reason.
#[test]
fn reason_codes_and_texts_match_the_suite() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/binary64/v0/binary64.json");
    let document = parse_document(&std::fs::read_to_string(path).unwrap(), 4).unwrap();
    let Json::Array(reasons) = required(&document, "reasons") else {
        panic!("reasons is not an array")
    };
    let errors = [
        Binary64Error::TooManyDigits,
        Binary64Error::NotRoundTrip,
        Binary64Error::NotExactInteger,
    ];
    assert_eq!(reasons.len(), errors.len());
    for (reason, error) in reasons.iter().zip(errors) {
        assert_eq!(text(required(reason, "code")), error.code());
        assert_eq!(text(required(reason, "message")), error.to_string());
    }
}
