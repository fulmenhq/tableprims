# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-24

### Added

- Cargo workspace with the `tableprims` library crate and the `tableprims-cli`
  package, which builds the `tablebolt` binary.
- `tablebolt version` and `tablebolt capabilities`, each with `--json`.
- Library module `exact`: exact integers and fixed-scale decimals of up to 38
  digits with a strict textual grammar, checked addition and quantization
  with five rounding modes.
- Library module `table`: schemas of uniquely identified columns, typed values
  and a row check that locates the first violation by row index and column
  identifier.
- Library module `temporal`: calendar dates, local timestamps and
  fixed-offset timestamps with a declared fractional-second precision, kept
  as written.
- Library module `ndjson`: rows as newline-delimited JSON arrays of `null`,
  booleans and strings, read one line at a time under a line length limit.
  Bare numbers and nested values are refused.
- Library module `binary64`: decides whether an exact integer or decimal can
  be stored as a binary64 number and recovered, and returns the value to
  store. At most 15 significant digits; integers must be exact in binary.
- Library module `json`: a strict reader for small JSON documents that
  refuses repeated object keys, unpaired surrogates and any number other than
  a plain non-negative integer, under a nesting depth limit.
- Library module `documents`: schema and policy documents as strict JSON
  objects with a format identifier. Unknown members are refused at every
  depth. A policy names the exact-number representation and per-column
  quantization, and carries the destination sheet and optional limits under
  an extension namespace.
- `tableprims-xlsx` crate: writes a typed table to one worksheet in two reads
  of the input, so each column's representation is fixed before the first
  cell. Exact numbers become numeric cells only when the stored value reads
  back as the source value; otherwise the policy refuses the value or writes
  the column as text. Types, scale, nullability and empty strings are
  recorded on a hidden `_tableprims` worksheet.
- `tablebolt xlsx write`: converts newline-delimited JSON rows to a workbook
  under a schema and a policy document, and writes a result file with the
  status, digests of the inputs and the workbook, a digest of the resolved
  policy, per-column counts and, on a refusal, a category, rule and location.
  Existing files are never replaced. `tablebolt capabilities` lists the
  conversion. See `docs/xlsx-write.md`.
- Build, format, lint, test, dependency and minimum-Rust-version checks.
- `make check-platform` as the native gate: formatting, Clippy, tests, the
  `tablebolt` smoke run, the leaf-dependency rule, a content check for local
  paths, keys and tokens, and version sites. CI runs it on macOS and Windows
  through a local composite action.
- `make check` as the full gate CI runs on Linux: `check-platform` plus
  workflow digest pins, release tooling controls and cargo-deny.
- `make prepush` adds the minimum-Rust-version build, YAML and workflow lint
  and tool controls to `check`. `make pr-final` adds a required
  public-surface scan, including the PR title and body, to `check`.
- `VERSION` as the single source of truth, with `make version-set`,
  `version-patch`, `version-minor` and `version-major`.
- Release tag tooling and its controls: committed public signing pins and
  fingerprint anchors, a pinned tagger identity, signed-tag creation and
  verification, and a release checklist.
- Decision records for the library and language surfaces, the conversion
  result, exact values, the typed table and its documents, expected results,
  record keeping, and untrusted input and dependencies, under
  `docs/decisions/`.
- DDR-0004: the Tabular Exchange Standard governs the table model and the
  documents. It supersedes DDR-0002.
- The vectors, fixtures and JSON Schemas of `tabular-exchange/0.1.0`,
  vendored unedited from the fulmenhq/crucible release `v0.5.1` (CC0-1.0)
  under `third_party/crucible/`, with their origin, commit, licence and a
  SHA-256 for every file. A test compares every vendored file with its
  digest.
- Tests that run the standard's vectors through the `tablebolt` binary and
  compare status, category, rule and location: 200 value texts, 25 cells, 77
  quantize vectors, 76 row vectors, 31 document vectors, 36 table schema
  fixtures and 32 conversion policy fixtures, 477 of the 639 in the release.
  All agree. The 92 conversion result fixtures and the 70 matching vectors
  are not run, because `tablebolt` does not read results. Every vector is
  run or counted with a reason, the counts are exact, and each suite has a
  test that alters an expectation and requires a disagreement. See
  `docs/xlsx-write.md`.
- Fault controls for the checks in the tests: a changed digit, a dropped or
  added row, reordered or added columns, a changed representation, missing
  or altered metadata and a truncated workbook, each reported by an
  independent read-back of the workbook; a changed or truncated workbook,
  rows cut at a line end and a result for another request, each failing the
  match of a result; rows cut before the closing bracket of the last line
  refused as malformed or invalid UTF-8. Native decimals above scale 30,
  written without a number format, are read back exactly.
- `tablebolt xlsx write` stops on the first SIGINT or SIGTERM: it removes
  its temporary files, publishes a result with status `cancelled`, and then
  ends by that signal on Unix (a shell reports 130 or 143) or exits 130 on
  Windows. A second signal ends the process at once. Adds `signal-hook` to
  the CLI package. ADR-0003 records this and supersedes rule 7 of ADR-0002.
- `tablebolt xlsx write --runtime-dir DIR` (or `TABLEBOLT_RUNTIME_DIR`)
  reports the run through the process-run contract of 3leaps Crucible,
  telemetry only: a card and an event stream with `started`, `progress`,
  `heartbeat` and one terminal event. Off by default and fail-open; it never
  affects the result or the exit code. With a runtime directory, a run
  identifier ending in `.` or naming a Windows device is a usage error. The contract's schemas and examples
  are vendored unedited from 3leaps/crucible `v0.2.12` (CC0-1.0) under
  `third_party/3leaps-crucible/`, and the tests validate the card and every
  event against them with `jsonschema`, a new test-only dependency.
- Go package `github.com/fulmenhq/tableprims/table` (root `go.mod`, Go 1.26,
  standard library only, no cgo): reads schema and policy documents, values,
  quantization and rows of `tabular-exchange/0.1.0`, and reports every
  refusal as a `*table.Refusal` with its category, rule, row and column. Its
  tests run the same 477 vendored vectors and fixtures as the Rust tests and
  meet every expectation. It does not write workbooks and has no
  command-line tool; `docs/go.md` lists where it differs from `tablebolt`.
- `make check-go`, `make test-go` (both part of `make check-platform`) and
  `make test-go-race` (part of `make check`). Every CI cell installs or checks
  Go 1.26.10. `make doctor DOCTOR_SCOPE=dev` reports Go.
- `make licenses-check` (part of `make check`): every package in the
  resolved graph, dev-dependencies included, against the licence allow list
  in `deny.toml`, which now admits MIT-0.

### Changed

- NOTICE: a project name notice for `tableprims` and `tablebolt`, the 3 Leaps
  and Fulmen marks, and a link to the 3 Leaps OSS policies at a fixed
  revision.
- The rows, schema, policy and result now follow the Tabular Exchange
  Standard, contract revision `tabular-exchange/0.1.0`, published in
  fulmenhq/crucible. See `docs/xlsx-write.md`.
- Format identifiers are `tabular-exchange.schema/0`,
  `tabular-exchange.policy/0` and `tabular-exchange.result/0`. The earlier
  `tableprims.schema/0` and `tableprims.policy/0` are different formats and
  are refused with `schema-format-unsupported` and
  `policy-format-unsupported`; `tableprims.result/0` is no longer written.
- Policy: `sheet` and `limits` are members of `extensions.tableprims`. At
  the top level each is `policy-member-unknown`. A policy that carries any
  other extension namespace is refused with `invalid-input`,
  `policy-extension-unsupported`. `extensions` is held to the standard's
  bounds before any namespace is looked up: 8 namespaces, 256 values,
  strings and member names of 256 code points and numbers up to 2^53 - 1
  (`policy-member-invalid`), a namespace pattern (`policy-member-invalid`)
  and namespace values that are objects (`policy-not-an-object`). A policy
  nests at most six deep instead of three; a schema still nests three.
- Policy: `Policy::sheet` returns an `Option`. A policy that names no
  worksheet is read, and `tablebolt xlsx write` then refuses it with
  `unsupported-representation`, `sheet-name-missing` (exit code 62) instead
  of `invalid-input`, `policy-member-missing`.
- Policy: a worksheet named `_tableprims` in any letter case is read, and
  `tablebolt xlsx write` then refuses it with `unsupported-representation`,
  `sheet-name-reserved` (exit code 62) instead of `invalid-input`,
  `policy-member-invalid`. Letter case is judged by Unicode case mapping
  instead of ASCII only, so `_tableprimſ` (long s) is also reserved. The
  reserved name is
  `tableprims_xlsx::metadata::WORKSHEET`; the library constant
  `documents::RESERVED_SHEET` is removed.
- Result: required `"contract": "tabular-exchange/0.1.0"`. The source digest
  is `inputs.source` instead of `inputs.rows`. `tool`, `backend`, `limits`
  and `policy_digest` are under `extensions.tableprims` instead of at the top
  level.
- Result: the object that `policy_digest` covers carries `sheet` and
  `limits` under `extensions.tableprims`, as a policy does, so every policy
  digest differs from the one an earlier build computed. Its identifier
  stays `tableprims.resolved-policy/0`; it is not a document of the
  standard.
- A quantize `scale` above 38 is `invalid-input`, `policy-member-invalid`
  when the policy is read, instead of `schema-mismatch`,
  `quantize-scale-too-large`. A scale of at most 38 that exceeds the column's
  precision is still `quantize-scale-too-large`.
- Library module `json`: an integer above `u64::MAX` that is spelled as a
  plain non-negative integer is read as `Json::BigInteger` instead of being
  refused as `UnsupportedNumber`. A schema or policy member holding one is
  `schema-member-invalid` or `policy-member-invalid` instead of
  `schema-malformed` or `policy-malformed`.
- Library module `temporal`: a timestamp is judged as a well-formed
  timestamp of either type before it is compared with the declared type.
  New `TemporalError::OffsetMismatch`, reported as `schema-mismatch`,
  `temporal-offset-mismatch`, for a local timestamp column given a value
  with a well-formed offset (was `invalid-input`, `temporal-malformed`) and
  an offset timestamp column given a value with none (was `invalid-input`,
  `temporal-invalid-offset`). A point after the seconds with no digit is
  `invalid-input`, `temporal-malformed` (was `temporal-fraction-mismatch`).
  Trailing text that is not an offset is `temporal-invalid-offset`, and
  `-00:00` is `temporal-negative-zero-offset`, in a column of either type
  and whether or not the fraction fits (was `temporal-fraction-mismatch`
  when the fraction did not fit, and `temporal-malformed` in a local
  timestamp column when it did). The set of accepted timestamps is
  unchanged.
- Library module `ndjson`: a row element that begins with `-` or a digit and
  is not a JSON number, such as `-`, `01`, `1.`, `1e` or `1x`, is
  `invalid-input`, `malformed-row` instead of `numeric-literal`. A JSON
  number is still `numeric-literal`.
- `docs/xlsx-write.md` describes the two verdicts of matching a result to a
  workbook, `output-agreement` and `requested-run-completed`, and that a
  caller who omits `--run-id` cannot reach the second.
- Library module `exact`: the grammar and the 38-digit bound are checked
  before the declared type. A misplaced or repeated decimal point is
  `Malformed`, more than 38 significant digits is `DigitLimitExceeded`, and
  an integer written with a decimal point is `PointMismatch`.
- `make deps-check` allows the CLI package, and only it, to depend on
  `canonprims-core` and `canonprims-json` at exactly 0.1.5, requires the
  library crate to have no dependencies and refuses a profile that aborts
  on panic.
- CI runs at the minimum supported Rust version on Linux x86_64 and aarch64,
  macOS arm64 and Windows x86_64 and aarch64; `make msrv` is now
  `make msrv-check`.

[Unreleased]: https://github.com/fulmenhq/tableprims/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/fulmenhq/tableprims/releases/tag/v0.1.0
